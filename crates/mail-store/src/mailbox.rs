//! What the message list shows: a folder, every inbox at once, or search
//! results, one row per message or per conversation.
//!
//! Conversations are grouped as headers arrive: a message joins the
//! conversation of any cached message it names in References or
//! In-Reply-To, or of any message that names it, merging conversations a
//! late message turns out to connect.

use mail_core::{AccountId, Flags, FolderId, FolderRole, MessageHeader, Result};
use rusqlite::{OptionalExtension, Row, Transaction, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

use crate::storage;
use crate::store::Store;

/// Which messages the list shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Mailbox {
    Folder {
        id: FolderId,
    },
    /// The inboxes of every account, merged.
    AllInboxes,
    /// Snoozed messages of every account, until they come back.
    Snoozed,
    /// Messages matching a search (see [`SearchQuery::parse`]).
    Search {
        query: String,
    },
}

/// One row in the message list: a message, or a conversation shown by its
/// newest message in this mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: i64,
    pub uid: u32,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub folder_name: String,
    pub folder_role: FolderRole,
    pub from: Option<String>,
    /// Shown instead of the sender in Sent and Drafts.
    pub to: Option<String>,
    pub subject: Option<String>,
    pub date: Option<i64>,
    /// The newest message's flags, except that `seen` is false while any
    /// message of the row is unread and `flagged` is true if any is flagged.
    pub flags: Flags,
    pub has_attachments: bool,
    pub thread_id: Option<i64>,
    /// The messages this row stands for in this mailbox, newest first.
    /// Actions on the row apply to all of them.
    pub ids: Vec<i64>,
    /// Messages in the whole conversation, in any folder but Trash and Junk.
    pub count: u32,
}

/// A window of the message list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub total: u32,
    pub offset: u32,
    pub items: Vec<MessageSummary>,
}

/// A conversation list kept between page loads.
pub(crate) struct RowCache {
    mailbox: Mailbox,
    /// `total_changes()` of the connection when it was built.
    generation: u64,
    rows: std::sync::Arc<Vec<Vec<i64>>>,
}

/// Search results are capped: nobody pages through more, and counting every
/// match of a one-letter search would cost more than showing them.
pub const MAX_RESULTS: u32 = 5000;

/// A search typed into the search box.
///
/// Words match the subject, addresses and text of a message, as prefixes
/// ("inv" finds "invoice"), ignoring case and accents. "Quoted words" match
/// as a phrase. Filters: `from:`, `to:` (To and Cc), `subject:`, `is:unread`,
/// `is:read`, `is:flagged`, `has:attachment` and `in:` with a folder name or
/// role (`in:sent`). Trash and Junk are left out unless `in:` names them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// The FTS5 expression, if any words were given.
    pub fts: Option<String>,
    pub unread: Option<bool>,
    pub flagged: bool,
    pub attachments: bool,
    pub folder: Option<String>,
}

impl SearchQuery {
    pub fn parse(input: &str) -> Self {
        let mut query = SearchQuery::default();
        let mut terms = Vec::new();
        for token in tokens(input) {
            let (key, value) = match token.split_once(':') {
                Some((key, value)) if !token.quoted && !value.is_empty() => {
                    (Some(key.to_ascii_lowercase()), unquote(value))
                }
                _ => (None, token.text.clone()),
            };
            let column = match key.as_deref() {
                Some("from") => Some("from_addr"),
                Some("to") | Some("cc") => Some("to_addrs"),
                Some("subject") => Some("subject"),
                Some("is") => {
                    match value.to_ascii_lowercase().as_str() {
                        "unread" => query.unread = Some(true),
                        "read" => query.unread = Some(false),
                        "flagged" | "starred" => query.flagged = true,
                        _ => {}
                    }
                    continue;
                }
                Some("has") => {
                    if value.to_ascii_lowercase().starts_with("attachment") {
                        query.attachments = true;
                    }
                    continue;
                }
                Some("in") => {
                    query.folder = Some(value);
                    continue;
                }
                // Not a filter, e.g. a time like 10:30: search for it as typed.
                Some(_) => {
                    terms.push(term(&token.text, false));
                    continue;
                }
                None => None,
            };
            let quoted = token.quoted || value.contains(char::is_whitespace);
            let t = term(&value, !quoted);
            if t.is_empty() {
                continue;
            }
            terms.push(match column {
                Some(column) => format!("{column} : {t}"),
                None => t,
            });
        }
        terms.retain(|t| !t.is_empty());
        if !terms.is_empty() {
            query.fts = Some(terms.join(" "));
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        *self == SearchQuery::default()
    }
}

struct Token {
    text: String,
    quoted: bool,
}

impl std::ops::Deref for Token {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}

/// Splits on whitespace, keeping "quoted phrases" and `key:"quoted values"`
/// together.
fn tokens(input: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quoted = false;
    for c in input.chars() {
        match c {
            // A quote opens a phrase at the start of a word or after `key:`
            // and closes it; elsewhere it is just a character.
            '"' if in_quotes => in_quotes = false,
            '"' if current.is_empty() || current.ends_with(':') => {
                in_quotes = true;
                quoted |= current.is_empty();
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    out.push(Token {
                        text: std::mem::take(&mut current),
                        quoted,
                    });
                }
                quoted = false;
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(Token {
            text: current,
            quoted,
        });
    }
    out
}

fn unquote(value: &str) -> String {
    value.trim_matches('"').to_owned()
}

/// One FTS5 string term, matched as a prefix when `prefix` is set. Text is
/// always quoted, so nothing typed is read as FTS5 syntax.
fn term(text: &str, prefix: bool) -> String {
    let text = text.trim_matches('"').trim();
    // Punctuation is not indexed; a term of only punctuation matches nothing.
    if !text.chars().any(char::is_alphanumeric) {
        return String::new();
    }
    let escaped = text.replace('"', "\"\"");
    if prefix {
        format!("\"{escaped}\"*")
    } else {
        format!("\"{escaped}\"")
    }
}

const SUMMARY_COLUMNS: &str = "m.id, m.uid, f.account_id, m.folder_id, f.name, f.role, \
     m.from_addr, m.to_addrs, m.subject, m.date, m.flags, m.has_attachments, m.thread_id";

/// A summary from the columns above, standing for itself alone.
fn summary(r: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    let id = r.get(0)?;
    Ok(MessageSummary {
        id,
        uid: r.get(1)?,
        account_id: AccountId(r.get(2)?),
        folder_id: FolderId(r.get(3)?),
        folder_name: r.get(4)?,
        folder_role: FolderRole::parse(r.get::<_, Option<String>>(5)?.as_deref()),
        from: r.get(6)?,
        to: r.get(7)?,
        subject: r.get(8)?,
        date: r.get(9)?,
        flags: Flags::from_bits(r.get(10)?),
        has_attachments: r.get(11)?,
        thread_id: r.get(12)?,
        ids: vec![id],
        count: 1,
    })
}

impl Store {
    /// A page of `mailbox`, newest first. With `threaded`, each row is a
    /// conversation; search results are always single messages.
    pub fn list(
        &self,
        mailbox: &Mailbox,
        threaded: bool,
        offset: u32,
        limit: u32,
    ) -> Result<MessagePage> {
        let (filter, args): (&str, Vec<Value>) = match mailbox {
            // Snoozed messages are away from their folder until they wake.
            Mailbox::Folder { id } => (
                "m.folder_id = ? AND m.id NOT IN (SELECT message FROM snoozes)",
                vec![Value::Integer(id.0)],
            ),
            Mailbox::AllInboxes => (
                "f.role = 'inbox' AND m.id NOT IN (SELECT message FROM snoozes)",
                Vec::new(),
            ),
            Mailbox::Snoozed => ("m.id IN (SELECT message FROM snoozes)", Vec::new()),
            Mailbox::Search { query } => {
                return self.search(&SearchQuery::parse(query), offset, limit);
            }
        };
        if !threaded {
            let total: u32 = self
                .conn
                .query_row(
                    &format!(
                        "SELECT count(*) FROM messages m JOIN folders f ON f.id = m.folder_id \
                         WHERE {filter}"
                    ),
                    params_from_iter(&args),
                    |r| r.get(0),
                )
                .map_err(storage)?;
            let mut stmt = self
                .conn
                .prepare_cached(&format!(
                    "SELECT {SUMMARY_COLUMNS} FROM messages m JOIN folders f ON f.id = m.folder_id \
                     WHERE {filter} ORDER BY m.date DESC, m.uid DESC LIMIT ? OFFSET ?"
                ))
                .map_err(storage)?;
            let mut all = args.clone();
            all.push(Value::Integer(limit.into()));
            all.push(Value::Integer(offset.into()));
            let mut items: Vec<MessageSummary> = stmt
                .query_map(params_from_iter(&all), summary)
                .map_err(storage)?
                .collect::<rusqlite::Result<_>>()
                .map_err(storage)?;
            self.count_conversations(&mut items)?;
            return Ok(MessagePage {
                total,
                offset,
                items,
            });
        }

        let rows = self.conversation_rows(mailbox, filter, &args)?;
        let start = (offset as usize).min(rows.len());
        let end = (start + limit as usize).min(rows.len());
        let page = &rows[start..end];
        let ids: Vec<Value> = page
            .iter()
            .flatten()
            .map(|id| Value::Integer(*id))
            .collect();
        let mut found = std::collections::HashMap::with_capacity(ids.len());
        if !ids.is_empty() {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT {SUMMARY_COLUMNS} FROM messages m JOIN folders f ON f.id = m.folder_id \
                     WHERE m.id IN ({})",
                    vec!["?"; ids.len()].join(",")
                ))
                .map_err(storage)?;
            for row in stmt
                .query_map(params_from_iter(&ids), summary)
                .map_err(storage)?
            {
                let row = row.map_err(storage)?;
                found.insert(row.id, row);
            }
        }
        let mut items: Vec<MessageSummary> = page
            .iter()
            .filter_map(|row| {
                let mut head = found.get(&row[0])?.clone();
                let members: Vec<&MessageSummary> =
                    row.iter().filter_map(|id| found.get(id)).collect();
                head.flags.seen = members.iter().all(|m| m.flags.seen);
                head.flags.flagged = members.iter().any(|m| m.flags.flagged);
                head.has_attachments = members.iter().any(|m| m.has_attachments);
                head.ids = members.iter().map(|m| m.id).collect();
                Some(head)
            })
            .collect();
        self.count_conversations(&mut items)?;
        Ok(MessagePage {
            total: rows.len() as u32,
            offset,
            items,
        })
    }

    /// Every row of a conversation list: the ids of each conversation's
    /// messages in the mailbox, newest first, ordered by the newest message.
    /// Grouping a large folder takes tens of milliseconds, so the result is
    /// kept until the database next changes and pages are cut from it.
    fn conversation_rows(
        &self,
        mailbox: &Mailbox,
        filter: &str,
        args: &[Value],
    ) -> Result<std::sync::Arc<Vec<Vec<i64>>>> {
        let generation = self.conn.total_changes();
        if let Some(cache) = &*self.rows.borrow()
            && cache.mailbox == *mailbox
            && cache.generation == generation
        {
            return Ok(cache.rows.clone());
        }
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "SELECT m.id, m.thread_id FROM messages m JOIN folders f ON f.id = m.folder_id \
                 WHERE {filter} ORDER BY m.date DESC, m.uid DESC"
            ))
            .map_err(storage)?;
        let mut rows: Vec<Vec<i64>> = Vec::new();
        let mut index: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        let mut query = stmt.query(params_from_iter(args)).map_err(storage)?;
        while let Some(r) = query.next().map_err(storage)? {
            let id: i64 = r.get(0).map_err(storage)?;
            let thread: Option<i64> = r.get(1).map_err(storage)?;
            match thread.and_then(|t| index.get(&t).copied()) {
                Some(i) => rows[i].push(id),
                None => {
                    if let Some(t) = thread {
                        index.insert(t, rows.len());
                    }
                    rows.push(vec![id]);
                }
            }
        }
        let rows = std::sync::Arc::new(rows);
        *self.rows.borrow_mut() = Some(RowCache {
            mailbox: mailbox.clone(),
            generation,
            rows: rows.clone(),
        });
        Ok(rows)
    }

    /// Fills in how many messages each row's conversation has.
    fn count_conversations(&self, items: &mut [MessageSummary]) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT count(DISTINCT coalesce(m.message_id, m.id)) FROM messages m \
                 JOIN folders f ON f.id = m.folder_id \
                 WHERE m.thread_id = ?1 AND coalesce(f.role, '') NOT IN ('trash', 'junk')",
            )
            .map_err(storage)?;
        for item in items {
            item.count = match item.thread_id {
                Some(thread) => stmt
                    .query_row([thread], |r| r.get::<_, u32>(0))
                    .map_err(storage)?
                    .max(1),
                None => 1,
            };
        }
        Ok(())
    }

    /// Messages matching `query`, newest first. Copies of one message in
    /// several folders (Gmail's All Mail) show once.
    pub fn search(&self, query: &SearchQuery, offset: u32, limit: u32) -> Result<MessagePage> {
        if query.is_empty() {
            return Ok(MessagePage {
                total: 0,
                offset,
                items: Vec::new(),
            });
        }
        let mut conditions = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        let from = match &query.fts {
            Some(fts) => {
                conditions.push("messages_fts MATCH ?".to_owned());
                args.push(Value::Text(fts.clone()));
                "messages_fts JOIN messages m ON m.id = messages_fts.rowid \
                 JOIN folders f ON f.id = m.folder_id"
            }
            None => "messages m JOIN folders f ON f.id = m.folder_id",
        };
        match query.unread {
            Some(true) => conditions.push("m.flags & 1 = 0".into()),
            Some(false) => conditions.push("m.flags & 1 != 0".into()),
            None => {}
        }
        if query.flagged {
            conditions.push("m.flags & 2 != 0".into());
        }
        if query.attachments {
            conditions.push("m.has_attachments".into());
        }
        match &query.folder {
            Some(folder) => {
                conditions.push("(f.role = lower(?) OR f.name = ? COLLATE NOCASE)".into());
                args.push(Value::Text(folder.clone()));
                args.push(Value::Text(folder.clone()));
            }
            None => conditions.push("coalesce(f.role, '') NOT IN ('trash', 'junk')".into()),
        }
        let filter = conditions.join(" AND ");
        let matching = format!(
            "SELECT {SUMMARY_COLUMNS}, max(m.date) AS newest FROM {from} WHERE {filter} \
             GROUP BY coalesce(m.message_id, m.id)"
        );
        let total: u32 = self
            .conn
            .query_row(
                &format!("SELECT count(*) FROM ({matching} LIMIT {MAX_RESULTS})"),
                params_from_iter(&args),
                |r| r.get(0),
            )
            .map_err(search_error)?;
        let limit = limit.min(MAX_RESULTS.saturating_sub(offset));
        let mut all = args;
        all.push(Value::Integer(limit.into()));
        all.push(Value::Integer(offset.into()));
        let mut stmt = self
            .conn
            .prepare(&format!(
                "{matching} ORDER BY newest DESC, m.uid DESC LIMIT ? OFFSET ?"
            ))
            .map_err(storage)?;
        let mut items: Vec<MessageSummary> = stmt
            .query_map(params_from_iter(&all), summary)
            .map_err(search_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(search_error)?;
        self.count_conversations(&mut items)?;
        Ok(MessagePage {
            total,
            offset,
            items,
        })
    }

    /// The conversation `message` belongs to, oldest first: its messages in
    /// any folder of the account, one copy each. Trash and Junk are left out
    /// unless the message itself is there. When a message is in several
    /// folders, the copy that was opened, or one beside it, is shown.
    pub fn conversation(&self, message: i64) -> Result<Vec<MessageSummary>> {
        let own = self
            .conn
            .query_row(
                &format!(
                    "SELECT {SUMMARY_COLUMNS} FROM messages m JOIN folders f ON f.id = m.folder_id \
                     WHERE m.id = ?1"
                ),
                [message],
                summary,
            )
            .optional()
            .map_err(storage)?;
        let Some(own) = own else {
            return Ok(Vec::new());
        };
        let Some(thread) = own.thread_id else {
            return Ok(vec![own]);
        };
        let hidden = matches!(own.folder_role, FolderRole::Trash | FolderRole::Junk);
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "SELECT {SUMMARY_COLUMNS}, m.message_id FROM messages m \
                 JOIN folders f ON f.id = m.folder_id \
                 WHERE m.thread_id = ?1 AND (?2 OR coalesce(f.role, '') NOT IN ('trash', 'junk')) \
                 ORDER BY m.date, m.id"
            ))
            .map_err(storage)?;
        let rows: Vec<(MessageSummary, Option<String>)> = stmt
            .query_map(params![thread, hidden], |r| Ok((summary(r)?, r.get(13)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        let mut out: Vec<(MessageSummary, Option<String>)> = Vec::with_capacity(rows.len());
        for (row, message_id) in rows {
            let duplicate = message_id
                .as_ref()
                .and_then(|id| out.iter().position(|(_, m)| m.as_ref() == Some(id)));
            match duplicate {
                Some(i) => {
                    // Prefer the copy that was opened, then one in the same folder.
                    let keep = &out[i].0;
                    if keep.id != own.id && (row.id == own.id || row.folder_id == own.folder_id) {
                        out[i] = (row, message_id);
                    }
                }
                None => out.push((row, message_id)),
            }
        }
        Ok(out.into_iter().map(|(row, _)| row).collect())
    }

    /// Adds a message's text to the search index once its body is cached.
    pub fn index_body(&self, message: i64, text: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM messages_fts WHERE rowid = ?1", [message])
            .map_err(storage)?;
        self.conn
            .execute(
                "INSERT INTO messages_fts (rowid, subject, from_addr, to_addrs, body) \
                 SELECT id, coalesce(subject, ''), coalesce(from_addr, ''), \
                 trim(coalesce(to_addrs, '') || ' ' || coalesce(cc_addrs, '')), ?2 \
                 FROM messages WHERE id = ?1",
                params![message, text],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Recent messages whose bodies are not cached yet, newest first, for
    /// downloading ahead of time: those dated after `since` and no larger
    /// than `max_size`, in folders other than Trash and Junk.
    pub fn bodies_to_prefetch(
        &self,
        folder: FolderId,
        since: i64,
        max_size: u32,
        limit: u32,
    ) -> Result<Vec<(i64, u32)>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT m.id, m.uid FROM messages m JOIN folders f ON f.id = m.folder_id \
                 WHERE m.folder_id = ?1 AND m.body_state = 'none' AND m.date >= ?2 \
                 AND coalesce(m.size, 0) <= ?3 \
                 AND coalesce(f.role, '') NOT IN ('trash', 'junk') \
                 ORDER BY m.date DESC LIMIT ?4",
            )
            .map_err(storage)?;
        stmt.query_map(params![folder.0, since, max_size, limit], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
    }
}

/// A search the index could not run (rare, as every term is quoted) reads as
/// no matches rather than a storage failure.
fn search_error(err: rusqlite::Error) -> mail_core::Error {
    mail_core::Error::Invalid(format!("that search could not run: {err}"))
}

/// Puts a newly cached message into its conversation, creating or merging
/// conversations as needed.
pub(crate) fn assign_thread(
    tx: &Transaction<'_>,
    account: AccountId,
    message: i64,
    header: &MessageHeader,
) -> Result<()> {
    let mut threads: Vec<i64> = Vec::new();
    let mut add = |t: i64| {
        if !threads.contains(&t) {
            threads.push(t);
        }
    };
    {
        // Messages this one names, and copies of itself in other folders.
        let mut by_id = tx
            .prepare_cached(
                "SELECT DISTINCT m.thread_id FROM messages m JOIN folders f ON f.id = m.folder_id \
                 WHERE m.message_id = ?1 AND f.account_id = ?2 AND m.thread_id IS NOT NULL",
            )
            .map_err(storage)?;
        for id in header.references.iter().chain(header.message_id.as_ref()) {
            for t in by_id
                .query_map(params![id, account.0], |r| r.get::<_, i64>(0))
                .map_err(storage)?
            {
                add(t.map_err(storage)?);
            }
        }
        // Messages that name this one (replies that arrived first), or name
        // a message this one names (siblings whose parent is missing).
        let mut by_ref = tx
            .prepare_cached(
                "SELECT DISTINCT m.thread_id FROM message_refs r \
                 JOIN messages m ON m.id = r.message JOIN folders f ON f.id = m.folder_id \
                 WHERE r.ref = ?1 AND f.account_id = ?2 AND m.thread_id IS NOT NULL",
            )
            .map_err(storage)?;
        for id in header.references.iter().chain(header.message_id.as_ref()) {
            for t in by_ref
                .query_map(params![id, account.0], |r| r.get::<_, i64>(0))
                .map_err(storage)?
            {
                add(t.map_err(storage)?);
            }
        }
    }
    threads.sort_unstable();
    let thread = match threads.first() {
        Some(t) => *t,
        None => {
            tx.execute("INSERT INTO threads (account_id) VALUES (?1)", [account.0])
                .map_err(storage)?;
            tx.last_insert_rowid()
        }
    };
    for other in threads.iter().skip(1) {
        tx.execute(
            "UPDATE messages SET thread_id = ?1 WHERE thread_id = ?2",
            params![thread, other],
        )
        .map_err(storage)?;
        tx.execute("DELETE FROM threads WHERE id = ?1", [other])
            .map_err(storage)?;
    }
    tx.execute(
        "UPDATE messages SET thread_id = ?1 WHERE id = ?2",
        params![thread, message],
    )
    .map_err(storage)?;
    let mut refs = tx
        .prepare_cached("INSERT INTO message_refs (message, ref) VALUES (?1, ?2)")
        .map_err(storage)?;
    for r in &header.references {
        refs.execute(params![message, r]).map_err(storage)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use mail_core::{Endpoint, NewAccount, RemoteFolder, Security};

    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory(dir.path()).unwrap();
        (dir, store)
    }

    fn account(store: &Store, email: &str) -> AccountId {
        let endpoint = Endpoint {
            host: "mail.example.test".into(),
            port: 993,
            security: Security::ImplicitTls,
        };
        store
            .add_account(&NewAccount {
                display_name: email.into(),
                email: email.into(),
                username: email.into(),
                imap: endpoint.clone(),
                smtp: endpoint,
                auth: Default::default(),
            })
            .unwrap()
            .id
    }

    /// Creates folders, returning their ids in order.
    fn folders(
        store: &mut Store,
        account: AccountId,
        list: &[(&str, FolderRole)],
    ) -> Vec<FolderId> {
        let remote: Vec<RemoteFolder> = list
            .iter()
            .map(|(name, role)| RemoteFolder {
                name: (*name).into(),
                delimiter: Some("/".into()),
                role: *role,
                selectable: true,
            })
            .collect();
        store
            .sync_folders(account, &remote)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    fn msg(uid: u32, id: &str, refs: &[&str], date: i64) -> MessageHeader {
        MessageHeader {
            uid,
            message_id: Some(format!("<{id}>")),
            references: refs.iter().map(|r| format!("<{r}>")).collect(),
            subject: Some(format!("Subject {id}")),
            from: Some(format!("{id} <{id}@example.test>")),
            date: Some(date),
            ..MessageHeader::default()
        }
    }

    fn insert(store: &mut Store, folder: FolderId, headers: &[MessageHeader]) -> Vec<i64> {
        store
            .insert_headers(folder, headers)
            .unwrap()
            .into_iter()
            .flatten()
            .collect()
    }

    fn thread_of(store: &Store, id: i64) -> Option<i64> {
        store
            .conn
            .query_row("SELECT thread_id FROM messages WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    #[test]
    fn snoozed_mail_leaves_the_inbox_until_it_wakes() {
        let (_dir, mut store) = store();
        let acc = account(&store, "a@example.test");
        let inbox = folders(&mut store, acc, &[("INBOX", FolderRole::Inbox)])[0];
        let ids = insert(
            &mut store,
            inbox,
            &[msg(1, "a", &[], 1), msg(2, "b", &[], 2)],
        );
        let shown = |store: &Store, mailbox: &Mailbox, threaded: bool| -> Vec<i64> {
            store
                .list(mailbox, threaded, 0, 50)
                .unwrap()
                .items
                .into_iter()
                .map(|m| m.id)
                .collect()
        };
        let folder = Mailbox::Folder { id: inbox };

        store.snooze(&[ids[1]], 100).unwrap();
        for threaded in [false, true] {
            assert_eq!(shown(&store, &folder, threaded), [ids[0]]);
            assert_eq!(shown(&store, &Mailbox::AllInboxes, threaded), [ids[0]]);
            assert_eq!(shown(&store, &Mailbox::Snoozed, threaded), [ids[1]]);
        }
        assert_eq!(store.snoozed_until(ids[1]).unwrap(), Some(100));
        assert_eq!(store.snoozed_count().unwrap(), 1);

        assert!(store.wake_snoozed(99).unwrap().is_empty());
        let woken = store.wake_snoozed(100).unwrap();
        assert_eq!(woken.len(), 1);
        assert_eq!((woken[0].id, woken[0].account), (ids[1], acc));
        assert_eq!(woken[0].subject.as_deref(), Some("Subject b"));
        assert_eq!(shown(&store, &folder, true), [ids[1], ids[0]]);
        assert_eq!(store.snoozed_count().unwrap(), 0);

        store.snooze(&ids, 500).unwrap();
        store.unsnooze(&[ids[0]]).unwrap();
        assert_eq!(shown(&store, &folder, false), [ids[0]]);
    }

    #[test]
    fn replies_join_the_conversation_they_answer() {
        let (_dir, mut store) = store();
        let acc = account(&store, "a@example.test");
        let inbox = folders(&mut store, acc, &[("INBOX", FolderRole::Inbox)])[0];
        let ids = insert(
            &mut store,
            inbox,
            &[
                msg(1, "a", &[], 1),
                msg(2, "b", &["a"], 2),
                msg(3, "c", &["a", "b"], 3),
                msg(4, "other", &[], 4),
            ],
        );
        let t = thread_of(&store, ids[0]);
        assert!(t.is_some());
        assert_eq!(thread_of(&store, ids[1]), t);
        assert_eq!(thread_of(&store, ids[2]), t);
        assert_ne!(thread_of(&store, ids[3]), t);
        // Inserting again only updates flags.
        assert_eq!(
            store.insert_headers(inbox, &[msg(1, "a", &[], 1)]).unwrap(),
            [None]
        );
    }

    #[test]
    fn a_late_message_merges_the_conversations_it_connects() {
        let (_dir, mut store) = store();
        let acc = account(&store, "a@example.test");
        let inbox = folders(&mut store, acc, &[("INBOX", FolderRole::Inbox)])[0];
        // Two replies arrive before the message they both answer, and name
        // only their direct parents.
        let ids = insert(
            &mut store,
            inbox,
            &[
                msg(1, "reply1", &["root"], 2),
                msg(2, "reply2", &["root"], 3),
            ],
        );
        assert_eq!(thread_of(&store, ids[0]), thread_of(&store, ids[1]));
        let ids2 = insert(
            &mut store,
            inbox,
            &[msg(3, "x", &["unrelated"], 4), msg(4, "y", &["x"], 5)],
        );
        assert_ne!(thread_of(&store, ids[0]), thread_of(&store, ids2[0]));
        // A message naming both "root" and "x" joins all four.
        let late = insert(&mut store, inbox, &[msg(5, "z", &["root", "x"], 6)]);
        let t = thread_of(&store, late[0]);
        for id in ids.iter().chain(&ids2) {
            assert_eq!(thread_of(&store, *id), t);
        }
        let threads: i64 = store
            .conn
            .query_row("SELECT count(*) FROM threads", [], |r| r.get(0))
            .unwrap();
        assert_eq!(threads, 1);
    }

    #[test]
    fn conversations_stay_within_one_account() {
        let (_dir, mut store) = store();
        let a = account(&store, "a@example.test");
        let b = account(&store, "b@example.test");
        let ia = folders(&mut store, a, &[("INBOX", FolderRole::Inbox)])[0];
        let ib = folders(&mut store, b, &[("INBOX", FolderRole::Inbox)])[0];
        let x = insert(&mut store, ia, &[msg(1, "same", &[], 1)]);
        let y = insert(&mut store, ib, &[msg(1, "same", &[], 1)]);
        assert_ne!(thread_of(&store, x[0]), thread_of(&store, y[0]));
    }

    #[test]
    fn threaded_lists_show_one_row_per_conversation() {
        let (_dir, mut store) = store();
        let acc = account(&store, "a@example.test");
        let f = folders(
            &mut store,
            acc,
            &[("INBOX", FolderRole::Inbox), ("Sent", FolderRole::Sent)],
        );
        let (inbox, sent) = (f[0], f[1]);
        let mut first = msg(1, "a", &[], 10);
        first.flags.seen = true;
        let mut flagged = msg(2, "c", &["a", "b"], 30);
        flagged.flags.flagged = true;
        let ids = insert(
            &mut store,
            inbox,
            &[first, flagged, msg(3, "solo", &[], 20)],
        );
        // The user's reply sits in Sent.
        let mut reply = msg(1, "b", &["a"], 25);
        reply.flags.seen = true;
        insert(&mut store, sent, &[reply]);

        let page = store
            .list(&Mailbox::Folder { id: inbox }, true, 0, 10)
            .unwrap();
        assert_eq!(page.total, 2);
        let row = &page.items[0];
        assert_eq!(row.id, ids[1], "the newest message leads its conversation");
        assert_eq!(row.ids, [ids[1], ids[0]]);
        assert_eq!(row.count, 3, "the reply in Sent counts too");
        assert!(!row.flags.seen && row.flags.flagged);
        assert_eq!(page.items[1].ids, [ids[2]]);
        assert_eq!(page.items[1].count, 1);

        let flat = store
            .list(&Mailbox::Folder { id: inbox }, false, 0, 10)
            .unwrap();
        assert_eq!(flat.total, 3);
        assert_eq!(flat.items[0].count, 3);
        assert_eq!(flat.items[0].ids, [ids[1]]);

        let conversation = store.conversation(ids[0]).unwrap();
        let subjects: Vec<_> = conversation
            .iter()
            .map(|m| m.subject.clone().unwrap())
            .collect();
        assert_eq!(subjects, ["Subject a", "Subject b", "Subject c"]);
        assert_eq!(conversation[1].folder_role, FolderRole::Sent);
    }

    #[test]
    fn a_conversation_shows_each_message_once() {
        let (_dir, mut store) = store();
        let acc = account(&store, "a@example.test");
        let f = folders(
            &mut store,
            acc,
            &[
                ("INBOX", FolderRole::Inbox),
                ("All Mail", FolderRole::Archive),
                ("Trash", FolderRole::Trash),
            ],
        );
        let inbox_ids = insert(&mut store, f[0], &[msg(1, "a", &[], 1)]);
        let all_ids = insert(
            &mut store,
            f[1],
            &[msg(1, "a", &[], 1), msg(2, "b", &["a"], 2)],
        );
        insert(&mut store, f[2], &[msg(1, "c", &["a"], 3)]);
        let from_inbox: Vec<i64> = store
            .conversation(inbox_ids[0])
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(from_inbox, [inbox_ids[0], all_ids[1]], "no copy, no trash");
        let from_all: Vec<i64> = store
            .conversation(all_ids[0])
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(from_all, [all_ids[0], all_ids[1]]);
        assert_eq!(
            store
                .list(&Mailbox::Folder { id: f[0] }, true, 0, 1)
                .unwrap()
                .items[0]
                .count,
            2
        );
    }

    #[test]
    fn all_inboxes_merge_every_account() {
        let (_dir, mut store) = store();
        let a = account(&store, "a@example.test");
        let b = account(&store, "b@example.test");
        let fa = folders(
            &mut store,
            a,
            &[("INBOX", FolderRole::Inbox), ("Sent", FolderRole::Sent)],
        );
        let fb = folders(&mut store, b, &[("INBOX", FolderRole::Inbox)]);
        insert(
            &mut store,
            fa[0],
            &[msg(1, "a1", &[], 1), msg(2, "a2", &[], 3)],
        );
        insert(&mut store, fa[1], &[msg(1, "sent", &[], 5)]);
        insert(&mut store, fb[0], &[msg(1, "b1", &[], 2)]);
        let page = store.list(&Mailbox::AllInboxes, true, 0, 10).unwrap();
        let rows: Vec<_> = page
            .items
            .iter()
            .map(|m| (m.subject.clone().unwrap(), m.account_id))
            .collect();
        assert_eq!(
            rows,
            [
                ("Subject a2".to_owned(), a),
                ("Subject b1".to_owned(), b),
                ("Subject a1".to_owned(), a)
            ]
        );
    }

    #[test]
    fn search_words_become_quoted_prefix_terms() {
        let q = SearchQuery::parse(r#"inv "big deal" from:bob is:unread has:attachment"#);
        assert_eq!(
            q.fts.as_deref(),
            Some(r#""inv"* "big deal" from_addr : "bob"*"#)
        );
        assert_eq!(q.unread, Some(true));
        assert!(q.attachments);
        let q = SearchQuery::parse(r#"to:"Ann Lee" in:Sent a"b NEAR( 10:30"#);
        assert_eq!(
            q.fts.as_deref(),
            Some(r#"to_addrs : "Ann Lee" "a""b"* "NEAR("* "10:30""#)
        );
        assert_eq!(q.folder.as_deref(), Some("Sent"));
        assert!(SearchQuery::parse("  ").is_empty());
        assert!(SearchQuery::parse("--- !!").is_empty());
    }

    #[test]
    fn search_finds_headers_and_downloaded_text() {
        let (_dir, mut store) = store();
        let acc = account(&store, "a@example.test");
        let f = folders(
            &mut store,
            acc,
            &[("INBOX", FolderRole::Inbox), ("Trash", FolderRole::Trash)],
        );
        let mut invoice = msg(1, "x", &[], 3);
        invoice.subject = Some("Your invoice for Café Noir".into());
        invoice.from = Some("Billing <billing@shop.test>".into());
        invoice.has_attachments = true;
        let mut lunch = msg(2, "y", &[], 2);
        lunch.subject = Some("Lunch".into());
        lunch.from = Some("Bob <bob@example.test>".into());
        lunch.flags.seen = true;
        let ids = insert(&mut store, f[0], &[invoice, lunch]);
        let mut old = msg(1, "z", &[], 1);
        old.subject = Some("Old invoice".into());
        insert(&mut store, f[1], &[old]);

        fn found_in(store: &Store, q: &str) -> Vec<i64> {
            store
                .search(&SearchQuery::parse(q), 0, 50)
                .unwrap()
                .items
                .iter()
                .map(|m| m.id)
                .collect()
        }
        assert_eq!(
            found_in(&store, "invo"),
            [ids[0]],
            "prefix match, trash left out"
        );
        assert_eq!(found_in(&store, "cafe"), [ids[0]], "accents ignored");
        assert_eq!(found_in(&store, "from:bob"), [ids[1]]);
        assert_eq!(found_in(&store, "is:unread"), [ids[0]]);
        assert_eq!(found_in(&store, "has:attachment billing"), [ids[0]]);
        assert_eq!(found_in(&store, "in:trash invoice").len(), 1);
        assert!(found_in(&store, "pistachio").is_empty());

        store
            .index_body(ids[1], "Shall we try the new pistachio place?")
            .unwrap();
        assert_eq!(found_in(&store, "pistachio"), [ids[1]]);
        assert_eq!(found_in(&store, "lunch"), [ids[1]], "headers stay indexed");
        assert_eq!(found_in(&store, "\"new pistachio\""), [ids[1]]);
        assert!(found_in(&store, "\"pistachio new\"").is_empty());

        // Deleted messages leave the index.
        store.remove_uids(acc, f[0], &[2]).unwrap();
        assert!(found_in(&store, "pistachio").is_empty());
    }

    #[test]
    fn settings_and_signatures_round_trip() {
        let (_dir, store) = store();
        assert_eq!(store.setting("theme").unwrap(), None);
        store
            .set_setting("theme", &serde_json::json!("dark"))
            .unwrap();
        store
            .set_setting("theme", &serde_json::json!("light"))
            .unwrap();
        assert_eq!(
            store.setting("theme").unwrap(),
            Some(serde_json::json!("light"))
        );
        let acc = account(&store, "a@example.test");
        assert_eq!(store.account(acc).unwrap().unwrap().signature, "");
        store
            .set_signature(acc, "Alice\nNovero", "<p><b>Alice</b></p>")
            .unwrap();
        let account = store.account(acc).unwrap().unwrap();
        assert_eq!(account.signature, "Alice\nNovero");
        assert_eq!(account.signature_html, "<p><b>Alice</b></p>");
    }

    /// The phase 5 target: search answers in under 200 ms across 100,000
    /// messages. Run in release: `cargo test -p mail-store --release -- --ignored`.
    #[test]
    #[ignore = "slow in debug builds; CI runs it in release"]
    fn search_and_lists_stay_fast_with_100_000_messages() {
        const WORDS: &[&str] = &[
            "invoice", "meeting", "lunch", "report", "budget", "travel", "review", "update",
            "project", "contract", "holiday", "party", "launch", "design", "hiring", "support",
        ];
        let (_dir, mut store) = store();
        let acc = account(&store, "a@example.test");
        let f = folders(
            &mut store,
            acc,
            &[
                ("INBOX", FolderRole::Inbox),
                ("Archive", FolderRole::Archive),
            ],
        );
        let mut seed = 7u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let start = Instant::now();
        for (n, folder) in [(60_000u32, f[0]), (40_000, f[1])] {
            let headers: Vec<MessageHeader> = (1..=n)
                .map(|uid| {
                    let r = next();
                    let w = |i: u64| WORDS[(r >> (i * 4)) as usize % WORDS.len()];
                    let thread = r % 20_000;
                    let mut h = msg(
                        uid,
                        &format!("m{uid}-{}@x", folder.0),
                        &[&format!("t{thread}@x")],
                        1_700_000_000 + i64::from(uid) * 60,
                    );
                    h.subject = Some(format!("{} {} {}", w(0), w(1), w(2)));
                    h.from = Some(format!("Person {} <p{}@example.test>", r % 500, r % 500));
                    h.flags.seen = r % 5 != 0;
                    h
                })
                .collect();
            for chunk in headers.chunks(500) {
                store.insert_headers(folder, chunk).unwrap();
            }
        }
        let rows: i64 = store
            .conn
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 100_000);
        println!("inserted 100,000 headers in {:?}", start.elapsed());
        // Bodies for a fifth of them, as prefetch would leave it.
        let ids: Vec<i64> = store
            .conn
            .prepare("SELECT id FROM messages WHERE id % 5 = 0")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for id in ids {
            store
                .index_body(
                    id,
                    &format!(
                        "Hello, about the {} next week. Regards",
                        WORDS[id as usize % 16]
                    ),
                )
                .unwrap();
        }

        // Both the first run and a repeat (a list's next page) must be quick.
        let timed = |label: &str, run: &dyn Fn() -> MessagePage| {
            let start = Instant::now();
            run();
            let first = start.elapsed();
            let start = Instant::now();
            let page = run();
            let again = start.elapsed();
            println!(
                "{label}: {} results, {first:?}, again {again:?}",
                page.total
            );
            assert!(first.max(again).as_millis() < 200, "{label} took {first:?}");
            page
        };
        let store = &store;
        let search =
            |q: &'static str| move || store.search(&SearchQuery::parse(q), 0, 100).unwrap();
        assert!(timed("common word", &search("invoice")).total > 1000);
        timed("prefix", &search("me"));
        timed("two words", &search("budget travel"));
        timed("sender", &search("from:p42"));
        timed("body text", &search("regards holiday"));
        timed("filter only", &search("is:unread"));
        assert_eq!(timed("no match", &search("pistachio")).total, 0);
        let list = |mailbox: Mailbox, threaded: bool, offset: u32| {
            move || store.list(&mailbox, threaded, offset, 100).unwrap()
        };
        timed(
            "inbox, conversations",
            &list(Mailbox::Folder { id: f[0] }, true, 0),
        );
        timed(
            "inbox, conversations, deep page",
            &list(Mailbox::Folder { id: f[0] }, true, 15_000),
        );
        timed(
            "inbox, messages",
            &list(Mailbox::Folder { id: f[0] }, false, 30_000),
        );
        timed(
            "all inboxes, conversations",
            &list(Mailbox::AllInboxes, true, 0),
        );
    }
}
