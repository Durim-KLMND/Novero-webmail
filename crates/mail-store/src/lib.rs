//! Local SQLite cache and message files.
//!
//! Migrations are plain SQL files applied in order and tracked with
//! `PRAGMA user_version`, so a database records which ones it has.

mod calendar;
mod calendar_sync;
mod categories;
mod contacts;
mod mailbox;
mod outgoing;
mod reactions;
mod rules;
mod signatures;
mod snooze;
mod store;
mod todo;

use std::path::Path;

pub use calendar::Reminder;
pub use calendar_sync::{Pending, PendingSave, SyncedCalendar};
pub use categories::{CleanupItem, CleanupReport, CleanupScope, SenderGroup, Unchecked, sender_of};
pub use contacts::Contact;
pub use mail_core::{Error, Result};
pub use mailbox::{MAX_RESULTS, Mailbox, MessagePage, MessageSummary, SearchQuery};
pub use outgoing::{DraftUpload, NewDraft, OutboxItem, OutboxState};
pub use reactions::MessageReaction;
pub use rules::RuleMessage;
use rusqlite::Connection;
pub use snooze::Woken;
pub use store::{Folder, FolderState, MessageLocation, MessageSource, QueuedOp, Store};
pub use todo::{Board, Note, Task, TaskColumn};

/// Migrations in order. Never edit one that has shipped; add a new file.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_account_username.sql"),
    include_str!("../migrations/0003_drafts_and_outbox.sql"),
    include_str!("../migrations/0004_oauth_provider.sql"),
    include_str!("../migrations/0005_search_threads_settings.sql"),
    include_str!("../migrations/0006_contacts_signatures.sql"),
    include_str!("../migrations/0007_calendar.sql"),
    include_str!("../migrations/0008_send_later_snooze.sql"),
    include_str!("../migrations/0009_signatures_templates.sql"),
    include_str!("../migrations/0010_calendar_sync.sql"),
    include_str!("../migrations/0011_notes_tasks.sql"),
    include_str!("../migrations/0012_categories.sql"),
    include_str!("../migrations/0013_reactions.sql"),
];

/// Schema version a fully migrated database has.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Opens (or creates) the database at `path` and brings it up to date.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).map_err(storage)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(storage)?;
    prepare(conn)
}

/// Opens an in-memory database with the full schema, for tests.
pub fn open_in_memory() -> Result<Connection> {
    prepare(Connection::open_in_memory().map_err(storage)?)
}

fn prepare(mut conn: Connection) -> Result<Connection> {
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(storage)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Applies every migration newer than the database's `user_version`.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage)?;
    if current > SCHEMA_VERSION {
        return Err(Error::Storage(format!(
            "database schema {current} is newer than this app supports ({SCHEMA_VERSION})"
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction().map_err(storage)?;
        tx.execute_batch(sql).map_err(storage)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn storage(err: rusqlite::Error) -> Error {
    Error::Storage(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(conn: &Connection) -> u32 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_database_reaches_the_latest_schema() {
        let conn = open_in_memory().unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN \
                 ('accounts', 'folders', 'messages', 'op_queue', 'outbox', 'drafts')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 6);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let mut conn = open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
    }

    #[test]
    fn an_old_signature_becomes_the_first_of_several() {
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..8] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, display_name, email, imap_host, imap_port, imap_security, \
                 smtp_host, smtp_port, smtp_security, auth_kind, signature) \
             VALUES (1, 'Alice', 'alice@example.test', 'imap.example.test', 993, 'implicit_tls', \
                 'smtp.example.test', 465, 'implicit_tls', 'password', 'Alice'), \
                 (2, 'Bob', 'bob@example.test', 'imap.example.test', 993, 'implicit_tls', \
                 'smtp.example.test', 465, 'implicit_tls', 'password', '');",
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let rows: Vec<(i64, Option<String>, Option<i64>)> = conn
            .prepare(
                "SELECT a.id, s.text, a.signature_reply FROM accounts a \
                 LEFT JOIN signatures s ON s.id = a.signature_new ORDER BY a.id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows, [(1, Some("Alice".into()), Some(1)), (2, None, None)]);
    }

    #[test]
    fn newer_database_is_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        assert!(matches!(migrate(&mut conn), Err(Error::Storage(_))));
    }

    #[test]
    fn deleting_an_account_removes_its_folders_and_messages() {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, display_name, email, imap_host, imap_port, imap_security, \
                 smtp_host, smtp_port, smtp_security, auth_kind) \
             VALUES (1, 'Alice', 'alice@example.test', 'imap.example.test', 993, 'implicit_tls', \
                 'smtp.example.test', 465, 'implicit_tls', 'password'); \
             INSERT INTO folders (id, account_id, name, role) VALUES (1, 1, 'INBOX', 'inbox'); \
             INSERT INTO messages (folder_id, uid, subject) VALUES (1, 42, 'hello'); \
             DELETE FROM accounts WHERE id = 1;",
        )
        .unwrap();
        let left: i64 = conn
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn full_text_search_finds_a_subject() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO messages_fts (rowid, subject, from_addr, to_addrs, body) \
             VALUES (1, 'Quarterly invoice', 'billing@example.test', 'alice@example.test', '')",
            [],
        )
        .unwrap();
        let hit: i64 = conn
            .query_row(
                "SELECT rowid FROM messages_fts WHERE messages_fts MATCH 'invoice'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1);
    }
}
