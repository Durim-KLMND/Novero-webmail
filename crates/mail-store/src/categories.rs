//! Mail cleanup: every message's category, from what the user chose for it
//! or its sender, or else guessed from its headers (`mail_core::categories`),
//! grouped by sender for the cleanup view.

use std::collections::{HashMap, HashSet};

use mail_core::address::parse_mailbox;
use mail_core::categories::{Category, CategoryInput, Reason, guess};
use mail_core::{AccountId, Error, Flags, FolderId, Result};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::storage;
use crate::store::Store;

/// Which mail the cleanup view looks through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupScope {
    /// Every account's inbox.
    Inbox,
    /// The inbox, Archive and the user's own folders; never Sent, Drafts,
    /// Trash or Junk.
    AllMail,
}

impl CleanupScope {
    fn condition(self) -> &'static str {
        match self {
            CleanupScope::Inbox => "f.role = 'inbox'",
            CleanupScope::AllMail => "(f.role IS NULL OR f.role IN ('inbox', 'archive'))",
        }
    }
}

/// One sender's mail of one category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SenderGroup {
    /// The sender's address in lower case; empty when the mail has none.
    pub email: String,
    pub name: Option<String>,
    pub category: Category,
    /// The user chose this category, for the sender or each message.
    pub confirmed: bool,
    /// The category the user gave the sender, if any.
    pub sender_category: Option<Category>,
    /// Most of the guesses in the group were sure ones.
    pub sure: bool,
    /// Why the guesses were made, most common first.
    pub reasons: Vec<Reason>,
    /// The messages, newest first.
    pub ids: Vec<i64>,
    pub unread: u32,
    pub latest: Option<i64>,
    pub subject: Option<String>,
    pub accounts: Vec<AccountId>,
    /// The newest message's link to leave the list.
    pub unsubscribe: Option<String>,
}

/// Every sender group in a scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub groups: Vec<SenderGroup>,
    /// Messages whose list headers haven't been looked at yet.
    pub unchecked: u32,
}

/// One message of a group, when the group is opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItem {
    pub id: i64,
    pub account_id: AccountId,
    pub subject: Option<String>,
    pub date: Option<i64>,
    pub unread: bool,
    pub category: Category,
    pub confirmed: bool,
    pub sure: bool,
    pub reasons: Vec<Reason>,
}

/// A message whose list headers are still to be read: from its cached file
/// when it has one, else from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unchecked {
    pub id: i64,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub folder_name: String,
    pub uid: u32,
    pub raw_path: Option<String>,
}

struct Row {
    id: i64,
    account: AccountId,
    message_id: Option<String>,
    from: Option<String>,
    subject: Option<String>,
    date: Option<i64>,
    flags: Flags,
    signals: Option<u32>,
    unsubscribe: Option<String>,
    chosen: Option<Category>,
}

/// A group's guesses: how many, how many were sure, and how often each
/// reason came up.
#[derive(Default)]
struct Tally {
    guesses: u32,
    sure: u32,
    reasons: Vec<(Reason, u32)>,
}

/// What the guesses need besides the message itself.
struct Context {
    senders: HashMap<String, Category>,
    contacts: HashSet<String>,
    own: HashSet<String>,
}

/// A row's category, whether the user chose it, and the guess behind it.
struct Decided {
    email: String,
    name: Option<String>,
    category: Category,
    confirmed: bool,
    sure: bool,
    reasons: Vec<Reason>,
}

impl Context {
    fn decide(&self, row: &Row) -> Decided {
        let (email, name) = sender_of(row.from.as_deref());
        let saved = self.senders.get(&email).copied();
        let g = guess(&CategoryInput {
            email: &email,
            name: name.as_deref(),
            subject: row.subject.as_deref(),
            signals: row.signals,
            contact: self.contacts.contains(&email),
            own: self.own.contains(&email),
            answered: row.flags.answered,
        });
        match row.chosen.or(saved) {
            Some(category) => Decided {
                email,
                name,
                category,
                confirmed: true,
                sure: true,
                reasons: Vec::new(),
            },
            None => Decided {
                email,
                name,
                category: g.category,
                confirmed: false,
                sure: g.sure,
                reasons: g.reasons,
            },
        }
    }
}

/// A From field's address in lower case, and its display name.
pub fn sender_of(from: Option<&str>) -> (String, Option<String>) {
    let Some(from) = from.map(str::trim).filter(|f| !f.is_empty()) else {
        return (String::new(), None);
    };
    match parse_mailbox(from) {
        Some(m) => (m.email.to_lowercase(), m.name),
        None => (from.to_lowercase(), None),
    }
}

fn category(text: Option<String>) -> Option<Category> {
    text.as_deref().and_then(Category::parse)
}

impl Store {
    fn cleanup_context(&self) -> Result<Context> {
        let senders = self
            .conn
            .prepare_cached("SELECT lower(email), category FROM sender_categories")
            .map_err(storage)?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(storage)?
            .filter_map(|r| r.ok())
            .filter_map(|(email, c)| Category::parse(&c).map(|c| (email, c)))
            .collect();
        let list = |sql: &str| -> Result<HashSet<String>> {
            self.conn
                .prepare_cached(sql)
                .map_err(storage)?
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<rusqlite::Result<_>>()
                .map_err(storage)
        };
        Ok(Context {
            senders,
            contacts: list("SELECT lower(email) FROM contacts WHERE sent > 0")?,
            own: list("SELECT lower(email) FROM accounts")?,
        })
    }

    /// The mail in a scope, newest first; with `AllMail`, each mail once
    /// even when it is in several folders (Gmail's labels), preferring the
    /// inbox copy.
    fn cleanup_rows(&self, scope: CleanupScope, account: Option<AccountId>) -> Result<Vec<Row>> {
        let sql = format!(
            "SELECT m.id, f.account_id, m.message_id, m.from_addr, m.subject, m.date, m.flags, \
                    m.signals, m.unsubscribe, mc.category \
             FROM messages m JOIN folders f ON f.id = m.folder_id \
             LEFT JOIN message_categories mc ON mc.message_id = m.id \
             WHERE {} AND (m.flags & 16) = 0 AND (?1 IS NULL OR f.account_id = ?1) \
             ORDER BY f.role = 'inbox' DESC, m.date DESC, m.id DESC",
            scope.condition()
        );
        let mut stmt = self.conn.prepare(&sql).map_err(storage)?;
        let rows = stmt
            .query_map([account.map(|a| a.0)], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    account: AccountId(r.get(1)?),
                    message_id: r.get(2)?,
                    from: r.get(3)?,
                    subject: r.get(4)?,
                    date: r.get(5)?,
                    flags: Flags::from_bits(r.get(6)?),
                    signals: r.get(7)?,
                    unsubscribe: r.get(8)?,
                    chosen: category(r.get(9)?),
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<Row>>>()
            .map_err(storage)?;
        let mut seen = HashSet::new();
        let mut rows: Vec<Row> = rows
            .into_iter()
            .filter(|r| match r.message_id.as_deref() {
                Some(id) if !id.is_empty() => seen.insert((r.account.0, id.to_owned())),
                _ => true,
            })
            .collect();
        rows.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
        Ok(rows)
    }

    /// Every sender's mail in a scope, grouped by sender and category,
    /// biggest groups first.
    pub fn cleanup(
        &self,
        scope: CleanupScope,
        account: Option<AccountId>,
    ) -> Result<CleanupReport> {
        let context = self.cleanup_context()?;
        let rows = self.cleanup_rows(scope, account)?;
        let mut report = CleanupReport::default();
        let mut index: HashMap<(String, Category), usize> = HashMap::new();
        let mut tally: Vec<Tally> = Vec::new();
        for row in &rows {
            if row.signals.is_none() {
                report.unchecked += 1;
            }
            let d = context.decide(row);
            let at = *index
                .entry((d.email.clone(), d.category))
                .or_insert_with(|| {
                    report.groups.push(SenderGroup {
                        sender_category: context.senders.get(&d.email).copied(),
                        email: d.email.clone(),
                        name: d.name.clone(),
                        category: d.category,
                        confirmed: true,
                        sure: false,
                        reasons: Vec::new(),
                        ids: Vec::new(),
                        unread: 0,
                        latest: row.date,
                        subject: row.subject.clone(),
                        accounts: Vec::new(),
                        unsubscribe: row.unsubscribe.clone(),
                    });
                    tally.push(Tally::default());
                    report.groups.len() - 1
                });
            let group = &mut report.groups[at];
            group.ids.push(row.id);
            group.unread += u32::from(!row.flags.seen);
            group.confirmed &= d.confirmed;
            if group.name.is_none() {
                group.name = d.name;
            }
            if group.unsubscribe.is_none() {
                group.unsubscribe = row.unsubscribe.clone();
            }
            if !group.accounts.contains(&row.account) {
                group.accounts.push(row.account);
            }
            if !d.confirmed {
                let t = &mut tally[at];
                t.guesses += 1;
                t.sure += u32::from(d.sure);
                for reason in d.reasons {
                    match t.reasons.iter_mut().find(|(r, _)| *r == reason) {
                        Some(entry) => entry.1 += 1,
                        None => t.reasons.push((reason, 1)),
                    }
                }
            }
        }
        for (group, mut t) in report.groups.iter_mut().zip(tally) {
            group.sure = t.guesses == 0 || t.sure * 2 > t.guesses;
            t.reasons.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
            group.reasons = t.reasons.into_iter().take(3).map(|(r, _)| r).collect();
        }
        report
            .groups
            .sort_by(|a, b| b.ids.len().cmp(&a.ids.len()).then(b.latest.cmp(&a.latest)));
        Ok(report)
    }

    /// The messages of a group, newest first, each with its own category.
    pub fn cleanup_items(&self, ids: &[i64]) -> Result<Vec<CleanupItem>> {
        let context = self.cleanup_context()?;
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT m.id, f.account_id, m.message_id, m.from_addr, m.subject, m.date, m.flags, \
                        m.signals, m.unsubscribe, mc.category \
                 FROM messages m JOIN folders f ON f.id = m.folder_id \
                 LEFT JOIN message_categories mc ON mc.message_id = m.id WHERE m.id = ?1",
            )
            .map_err(storage)?;
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            let row = stmt
                .query_row([id], |r| {
                    Ok(Row {
                        id: r.get(0)?,
                        account: AccountId(r.get(1)?),
                        message_id: r.get(2)?,
                        from: r.get(3)?,
                        subject: r.get(4)?,
                        date: r.get(5)?,
                        flags: Flags::from_bits(r.get(6)?),
                        signals: r.get(7)?,
                        unsubscribe: r.get(8)?,
                        chosen: category(r.get(9)?),
                    })
                })
                .optional()
                .map_err(storage)?;
            let Some(row) = row else { continue };
            let d = context.decide(&row);
            items.push(CleanupItem {
                id: row.id,
                account_id: row.account,
                subject: row.subject,
                date: row.date,
                unread: !row.flags.seen,
                category: d.category,
                confirmed: d.confirmed,
                sure: d.sure,
                reasons: d.reasons,
            });
        }
        items.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
        Ok(items)
    }

    /// The category the user gave a sender, if any.
    pub fn sender_category(&self, email: &str) -> Result<Option<Category>> {
        self.conn
            .query_row(
                "SELECT category FROM sender_categories WHERE email = ?1",
                [email.trim()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map(category)
            .map_err(storage)
    }

    /// Puts everything from a sender in `category`, or (`None`) goes back to
    /// guessing. Either way, the sender's messages lose any category chosen
    /// for them one by one.
    pub fn set_sender_category(&self, email: &str, category: Option<Category>) -> Result<()> {
        let email = email.trim().to_lowercase();
        if email.is_empty() {
            return Err(Error::Invalid("This mail has no sender to sort by.".into()));
        }
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        match category {
            Some(c) => tx.execute(
                "INSERT INTO sender_categories (email, category) VALUES (?1, ?2) \
                 ON CONFLICT (email) DO UPDATE SET category = ?2, updated = unixepoch()",
                params![email, c.as_str()],
            ),
            None => tx.execute("DELETE FROM sender_categories WHERE email = ?1", [&email]),
        }
        .map_err(storage)?;
        let chosen: Vec<(i64, Option<String>)> = tx
            .prepare(
                "SELECT mc.message_id, m.from_addr FROM message_categories mc \
                 JOIN messages m ON m.id = mc.message_id",
            )
            .map_err(storage)?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        for (id, from) in chosen {
            if sender_of(from.as_deref()).0 == email {
                tx.execute("DELETE FROM message_categories WHERE message_id = ?1", [id])
                    .map_err(storage)?;
            }
        }
        tx.commit().map_err(storage)
    }

    /// Puts messages in `category`, or (`None`) back to their sender's or
    /// the guessed one.
    pub fn set_message_category(&self, ids: &[i64], category: Option<Category>) -> Result<()> {
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        for id in ids {
            match category {
                Some(c) => tx.execute(
                    "INSERT INTO message_categories (message_id, category) \
                     SELECT id, ?2 FROM messages WHERE id = ?1 \
                     ON CONFLICT (message_id) DO UPDATE SET category = ?2",
                    params![id, c.as_str()],
                ),
                None => tx.execute("DELETE FROM message_categories WHERE message_id = ?1", [id]),
            }
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }

    /// Messages in a scope whose list headers haven't been read yet, by
    /// folder.
    pub fn unchecked(
        &self,
        scope: CleanupScope,
        account: Option<AccountId>,
    ) -> Result<Vec<Unchecked>> {
        let sql = format!(
            "SELECT m.id, f.account_id, f.id, f.name, m.uid, b.raw_path \
             FROM messages m JOIN folders f ON f.id = m.folder_id \
             LEFT JOIN message_bodies b ON b.message_id = m.id \
             WHERE m.signals IS NULL AND {} AND (?1 IS NULL OR f.account_id = ?1) \
             ORDER BY f.account_id, f.id, m.uid DESC",
            scope.condition()
        );
        let mut stmt = self.conn.prepare(&sql).map_err(storage)?;
        stmt.query_map([account.map(|a| a.0)], |r| {
            Ok(Unchecked {
                id: r.get(0)?,
                account_id: AccountId(r.get(1)?),
                folder_id: FolderId(r.get(2)?),
                folder_name: r.get(3)?,
                uid: r.get(4)?,
                raw_path: r.get(5)?,
            })
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
    }

    /// Saves the list-header hints read for messages: `(id, signals, unsubscribe link)`.
    pub fn set_signals(&self, found: &[(i64, u32, Option<String>)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        for (id, signals, unsubscribe) in found {
            tx.prepare_cached("UPDATE messages SET signals = ?2, unsubscribe = ?3 WHERE id = ?1")
                .map_err(storage)?
                .execute(params![id, signals, unsubscribe])
                .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use mail_core::categories::signals;
    use mail_core::{Endpoint, FolderRole, MessageHeader, NewAccount, RemoteFolder, Security};

    use super::*;

    fn setup() -> (tempfile::TempDir, Store, Vec<FolderId>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let endpoint = Endpoint {
            host: "mail.example.test".into(),
            port: 993,
            security: Security::ImplicitTls,
        };
        let me = store
            .add_account(&NewAccount {
                display_name: "Me".into(),
                email: "me@example.test".into(),
                username: "me@example.test".into(),
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
        let folders = store
            .sync_folders(
                me,
                &[
                    remote("INBOX", FolderRole::Inbox),
                    remote("Archive", FolderRole::Archive),
                    remote("Sent", FolderRole::Sent),
                ],
            )
            .unwrap()
            .into_iter()
            .map(|f| f.0)
            .collect();
        (dir, store, folders)
    }

    fn mail(uid: u32, from: &str, subject: &str, signals: u32) -> MessageHeader {
        MessageHeader {
            uid,
            message_id: Some(format!("<{uid}@example.test>")),
            from: Some(from.into()),
            subject: Some(subject.into()),
            date: Some(i64::from(uid)),
            signals,
            unsubscribe: (signals & signals::UNSUBSCRIBE != 0)
                .then(|| format!("https://shop.test/leave/{uid}")),
            ..MessageHeader::default()
        }
    }

    fn summary(report: &CleanupReport) -> Vec<(String, Category, usize, bool)> {
        report
            .groups
            .iter()
            .map(|g| (g.email.clone(), g.category, g.ids.len(), g.confirmed))
            .collect()
    }

    #[test]
    fn mail_is_grouped_by_sender_and_category() {
        let (_dir, mut store, folders) = setup();
        let list = signals::UNSUBSCRIBE | signals::CAMPAIGN;
        store
            .insert_headers(
                folders[0],
                &[
                    mail(1, "Shop <deals@shop.test>", "40% off everything", list),
                    mail(2, "Shop <deals@shop.test>", "Flash sale today only", list),
                    mail(
                        3,
                        "Shop <no-reply@shop.test>",
                        "Your order has shipped",
                        list,
                    ),
                    mail(4, "Ann <ann@home.test>", "Lunch on Friday?", 0),
                ],
            )
            .unwrap();
        // The archived copy of an inbox mail counts once; Sent is never looked at.
        store
            .insert_headers(
                folders[1],
                &[
                    mail(4, "Ann <ann@home.test>", "Lunch on Friday?", 0),
                    mail(5, "Ann <ann@home.test>", "Old photos", 0),
                ],
            )
            .unwrap();
        store
            .insert_headers(folders[2], &[mail(6, "me@example.test", "Sent", 0)])
            .unwrap();

        let inbox = store.cleanup(CleanupScope::Inbox, None).unwrap();
        assert_eq!(
            summary(&inbox),
            [
                ("deals@shop.test".into(), Category::Promotions, 2, false),
                ("ann@home.test".into(), Category::Personal, 1, false),
                ("no-reply@shop.test".into(), Category::Updates, 1, false),
            ]
        );
        let deals = &inbox.groups[0];
        assert_eq!(deals.name.as_deref(), Some("Shop"));
        assert_eq!(deals.subject.as_deref(), Some("Flash sale today only"));
        assert_eq!(deals.unread, 2);
        assert_eq!(
            deals.unsubscribe.as_deref(),
            Some("https://shop.test/leave/2")
        );
        assert!(deals.reasons.contains(&Reason::PromoWords));
        assert_eq!(inbox.unchecked, 0);

        let all = store.cleanup(CleanupScope::AllMail, None).unwrap();
        let ann = all
            .groups
            .iter()
            .find(|g| g.email == "ann@home.test")
            .unwrap();
        assert_eq!(ann.ids.len(), 2);
    }

    #[test]
    fn choices_for_senders_and_messages_win_over_guesses() {
        let (_dir, mut store, folders) = setup();
        let list = signals::UNSUBSCRIBE;
        let ids = store
            .insert_headers(
                folders[0],
                &[
                    mail(1, "Shop <deals@shop.test>", "40% off everything", list),
                    mail(2, "Shop <deals@shop.test>", "Our story", list),
                ],
            )
            .unwrap();
        let (first, second) = (ids[0].unwrap(), ids[1].unwrap());

        store
            .set_message_category(&[second], Some(Category::Newsletters))
            .unwrap();
        let report = store.cleanup(CleanupScope::Inbox, None).unwrap();
        assert_eq!(
            summary(&report),
            [
                ("deals@shop.test".into(), Category::Newsletters, 1, true),
                ("deals@shop.test".into(), Category::Promotions, 1, false),
            ]
        );

        // Choosing for the sender replaces the one-by-one choice.
        store
            .set_sender_category("DEALS@shop.test", Some(Category::Spam))
            .unwrap();
        assert_eq!(
            store.sender_category("deals@shop.test").unwrap(),
            Some(Category::Spam)
        );
        let report = store.cleanup(CleanupScope::Inbox, None).unwrap();
        assert_eq!(
            summary(&report),
            [("deals@shop.test".into(), Category::Spam, 2, true)]
        );
        let items = store.cleanup_items(&[first, second]).unwrap();
        assert!(
            items
                .iter()
                .all(|i| i.category == Category::Spam && i.confirmed)
        );
        assert_eq!(items[0].id, second);

        store.set_sender_category("deals@shop.test", None).unwrap();
        let report = store.cleanup(CleanupScope::Inbox, None).unwrap();
        assert!(report.groups.iter().all(|g| !g.confirmed));
        assert!(
            store
                .set_sender_category(" ", Some(Category::Spam))
                .is_err()
        );
    }

    #[test]
    fn people_the_user_wrote_to_are_personal() {
        let (_dir, mut store, folders) = setup();
        store
            .insert_headers(folders[0], &[mail(1, "news@paper.test", "Hello", 0)])
            .unwrap();
        store
            .insert_headers(
                folders[2],
                &[MessageHeader {
                    to: Some("news@paper.test".into()),
                    ..mail(2, "me@example.test", "Hi", 0)
                }],
            )
            .unwrap();
        let report = store.cleanup(CleanupScope::Inbox, None).unwrap();
        assert_eq!(report.groups[0].category, Category::Personal);
        assert!(report.groups[0].reasons.contains(&Reason::Contact));
    }

    #[test]
    fn unread_headers_are_listed_until_their_hints_are_saved() {
        let (_dir, mut store, folders) = setup();
        let ids = store
            .insert_headers(folders[0], &[mail(1, "a@b.test", "x", 0)])
            .unwrap();
        let id = ids[0].unwrap();
        store
            .conn
            .execute("UPDATE messages SET signals = NULL WHERE id = ?1", [id])
            .unwrap();
        let waiting = store.unchecked(CleanupScope::Inbox, None).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!((waiting[0].id, waiting[0].uid), (id, 1));
        assert_eq!(
            store.cleanup(CleanupScope::Inbox, None).unwrap().unchecked,
            1
        );
        store
            .set_signals(&[(id, signals::UNSUBSCRIBE, Some("mailto:leave@b.test".into()))])
            .unwrap();
        assert!(
            store
                .unchecked(CleanupScope::Inbox, None)
                .unwrap()
                .is_empty()
        );
        let report = store.cleanup(CleanupScope::Inbox, None).unwrap();
        assert_eq!(
            report.groups[0].unsubscribe.as_deref(),
            Some("mailto:leave@b.test")
        );
    }
}
