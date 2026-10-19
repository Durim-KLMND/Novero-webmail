//! People the user writes to and hears from, suggested while typing an
//! address. Learned from cached headers (senders of received mail,
//! recipients of sent mail) and from mail sent from the app.

use std::collections::HashMap;

use mail_core::address::{Mailbox, parse_lenient};
use mail_core::{FolderRole, MessageHeader, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::storage;
use crate::store::Store;

/// A suggested address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub email: String,
    pub name: Option<String>,
}

/// Marks a database whose existing headers have been read for contacts.
const BUILT: &str = "contactsBuilt";

/// What one message says about the people in it.
#[derive(Default)]
struct Seen {
    name: Option<String>,
    sent: i64,
    received: i64,
    last: Option<i64>,
}

/// The people a message adds: whoever it went to when the user sent it,
/// otherwise its sender. Junk, drafts and trash teach nothing.
fn people(
    role: FolderRole,
    from: Option<&str>,
    to: Option<&str>,
    cc: Option<&str>,
) -> (Vec<Mailbox>, bool) {
    match role {
        FolderRole::Junk | FolderRole::Drafts | FolderRole::Trash => (Vec::new(), false),
        FolderRole::Sent => {
            let mut list = parse_lenient(to.unwrap_or_default());
            list.extend(parse_lenient(cc.unwrap_or_default()));
            (list, true)
        }
        _ => (from.map(parse_lenient).unwrap_or_default(), false),
    }
}

fn record(conn: &Connection, email: &str, seen: &Seen) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO contacts (email, name, sent, received, last_seen) VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (email) DO UPDATE SET \
           name = COALESCE(excluded.name, name), \
           sent = sent + excluded.sent, \
           received = received + excluded.received, \
           last_seen = MAX(COALESCE(last_seen, 0), COALESCE(excluded.last_seen, 0))",
    )
    .map_err(storage)?
    .execute(params![
        email,
        seen.name,
        seen.sent,
        seen.received,
        seen.last
    ])
    .map_err(storage)?;
    Ok(())
}

/// Learns from a header just cached in a folder with `role`.
pub(crate) fn note_header(
    conn: &Connection,
    role: FolderRole,
    header: &MessageHeader,
) -> Result<()> {
    let (list, sent) = people(
        role,
        header.from.as_deref(),
        header.to.as_deref(),
        header.cc.as_deref(),
    );
    for person in list {
        let seen = Seen {
            name: person.name,
            sent: i64::from(sent),
            received: i64::from(!sent),
            last: header.date,
        };
        record(conn, &person.email, &seen)?;
    }
    Ok(())
}

/// Escapes `%`, `_` and `\` for a LIKE pattern with `ESCAPE '\'`.
fn like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Store {
    /// Reads the headers cached before contacts were kept, once.
    pub(crate) fn build_contacts(&self) -> Result<()> {
        if self.setting(BUILT)?.is_some() {
            return Ok(());
        }
        let mut people_seen: HashMap<String, Seen> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT f.role, m.from_addr, m.to_addrs, m.cc_addrs, m.date \
                     FROM messages m JOIN folders f ON f.id = m.folder_id",
                )
                .map_err(storage)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                    ))
                })
                .map_err(storage)?;
            for row in rows {
                let (role, from, to, cc, date) = row.map_err(storage)?;
                let role = FolderRole::parse(role.as_deref());
                let (list, sent) = people(role, from.as_deref(), to.as_deref(), cc.as_deref());
                for person in list {
                    let entry = people_seen.entry(person.email.to_lowercase()).or_default();
                    if person.name.is_some() && date >= entry.last {
                        entry.name = person.name;
                    }
                    entry.sent += i64::from(sent);
                    entry.received += i64::from(!sent);
                    entry.last = entry.last.max(date);
                }
            }
        }
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        for (email, seen) in &people_seen {
            record(&tx, email, seen)?;
        }
        tx.commit().map_err(storage)?;
        self.set_setting(BUILT, &serde_json::Value::Bool(true))
    }

    /// Counts a message the user sent to `recipients`.
    pub fn note_sent(&self, recipients: &[Mailbox], when: i64) -> Result<()> {
        for person in recipients {
            let seen = Seen {
                name: person.name.clone(),
                sent: 1,
                received: 0,
                last: Some(when),
            };
            record(&self.conn, &person.email, &seen)?;
        }
        Ok(())
    }

    /// Addresses starting with `query`, or with a name or a word of one that
    /// does. People the user has written to come first, then whoever they
    /// hear from most. The user's own addresses are left out.
    pub fn suggest_contacts(&self, query: &str, limit: u32) -> Result<Vec<Contact>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let prefix = format!("{}%", like(query));
        let word = format!("% {}%", like(query));
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT email, name FROM contacts \
                 WHERE hidden = 0 \
                   AND (email LIKE ?1 ESCAPE '\\' OR name LIKE ?1 ESCAPE '\\' OR name LIKE ?2 ESCAPE '\\') \
                   AND email NOT IN (SELECT email FROM accounts) \
                 ORDER BY sent > 0 DESC, sent * 3 + received DESC, last_seen DESC \
                 LIMIT ?3",
            )
            .map_err(storage)?;
        let rows = stmt
            .query_map(params![prefix, word, limit], |r| {
                Ok(Contact {
                    email: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(storage)?;
        rows.collect::<std::result::Result<_, _>>().map_err(storage)
    }

    /// Stops suggesting `email`. Mail from or to it later doesn't bring it back.
    pub fn hide_contact(&self, email: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO contacts (email, hidden) VALUES (?1, 1) \
                 ON CONFLICT (email) DO UPDATE SET hidden = 1",
                [email],
            )
            .map_err(storage)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_come_from_the_right_fields() {
        let from = Some("Ana <ana@example.test>");
        let to = Some("Bo <bo@example.test>, cy@example.test");
        let (list, sent) = people(FolderRole::Inbox, from, to, None);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "ana@example.test");
        assert!(!sent);
        let (list, sent) = people(FolderRole::Sent, from, to, Some("di@example.test"));
        let emails: Vec<_> = list.iter().map(|m| m.email.as_str()).collect();
        assert_eq!(
            emails,
            ["bo@example.test", "cy@example.test", "di@example.test"]
        );
        assert!(sent);
        assert!(people(FolderRole::Junk, from, to, None).0.is_empty());
    }

    use mail_core::{AccountId, Endpoint, FolderId, NewAccount, RemoteFolder, Security};

    fn setup(store: &mut Store) -> (FolderId, FolderId) {
        let endpoint = Endpoint {
            host: "mail.example.test".into(),
            port: 993,
            security: Security::ImplicitTls,
        };
        let account: AccountId = store
            .add_account(&NewAccount {
                display_name: "Me".into(),
                email: "me@example.test".into(),
                username: "me".into(),
                imap: endpoint.clone(),
                smtp: endpoint,
                auth: Default::default(),
            })
            .unwrap()
            .id;
        let remote = |name: &str, role| RemoteFolder {
            name: name.into(),
            delimiter: Some("/".into()),
            role,
            selectable: true,
        };
        let ids = store
            .sync_folders(
                account,
                &[
                    remote("INBOX", FolderRole::Inbox),
                    remote("Sent", FolderRole::Sent),
                ],
            )
            .unwrap();
        (ids[0].0, ids[1].0)
    }

    fn header(uid: u32, from: &str, to: &str, date: i64) -> MessageHeader {
        MessageHeader {
            uid,
            from: Some(from.into()),
            to: Some(to.into()),
            date: Some(date),
            ..MessageHeader::default()
        }
    }

    fn emails(list: Vec<Contact>) -> Vec<String> {
        list.into_iter().map(|c| c.email).collect()
    }

    #[test]
    fn suggestions_come_from_mail_and_put_people_written_to_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let (inbox, sent) = setup(&mut store);
        store
            .insert_headers(
                inbox,
                &[
                    header(1, "Bea News <bnews@shop.test>", "me@example.test", 10),
                    header(2, "Bea News <bnews@shop.test>", "me@example.test", 11),
                    header(3, "Me <me@example.test>", "me@example.test", 12),
                ],
            )
            .unwrap();
        store
            .insert_headers(
                sent,
                &[header(1, "me@example.test", "Bo Berg <bo@example.test>", 5)],
            )
            .unwrap();
        assert_eq!(
            emails(store.suggest_contacts("b", 10).unwrap()),
            ["bo@example.test", "bnews@shop.test"]
        );
        // A word of the name, any case; the user's own address is never offered.
        let found = store.suggest_contacts("BERG", 10).unwrap();
        assert_eq!(
            found,
            [Contact {
                email: "bo@example.test".into(),
                name: Some("Bo Berg".into())
            }]
        );
        assert!(store.suggest_contacts("me", 10).unwrap().is_empty());
        assert!(store.suggest_contacts("%", 10).unwrap().is_empty());

        store
            .note_sent(&parse_lenient("Bea <bnews@shop.test>"), 20)
            .unwrap();
        store
            .note_sent(&parse_lenient("bnews@shop.test"), 21)
            .unwrap();
        assert_eq!(
            emails(store.suggest_contacts("b", 10).unwrap()),
            ["bnews@shop.test", "bo@example.test"]
        );

        // Hidden addresses stay hidden when more mail arrives.
        store.hide_contact("BO@example.test").unwrap();
        store
            .insert_headers(
                inbox,
                &[header(4, "Bo <bo@example.test>", "me@example.test", 30)],
            )
            .unwrap();
        assert_eq!(
            emails(store.suggest_contacts("b", 10).unwrap()),
            ["bnews@shop.test"]
        );
    }

    #[test]
    fn mail_cached_before_contacts_existed_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = Store::open(dir.path()).unwrap();
            let (inbox, _) = setup(&mut store);
            store
                .insert_headers(
                    inbox,
                    &[header(1, "Ana <ana@example.test>", "me@example.test", 1)],
                )
                .unwrap();
            // As a database from before this version would be.
            store.conn.execute("DELETE FROM contacts", []).unwrap();
            store
                .conn
                .execute("DELETE FROM settings WHERE key = ?1", [BUILT])
                .unwrap();
        }
        let store = Store::open(dir.path()).unwrap();
        let found = store.suggest_contacts("an", 10).unwrap();
        assert_eq!(
            found,
            [Contact {
                email: "ana@example.test".into(),
                name: Some("Ana".into())
            }]
        );
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let received: i64 = store
            .conn
            .query_row(
                "SELECT received FROM contacts WHERE email = 'ana@example.test'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(received, 1);
    }

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(like("50%_a\\b"), "50\\%\\_a\\\\b");
    }
}
