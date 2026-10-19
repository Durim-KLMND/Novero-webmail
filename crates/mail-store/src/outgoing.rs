//! Drafts and the outbox: mail the user is writing or has sent but the
//! server has not taken yet.
//!
//! A draft's attachments are files under `<account>/drafts/<draft>/`, named by
//! their position. An outgoing message is two files under `<account>/outbox/`:
//! what goes over SMTP, and the copy (Bcc kept) for the Sent folder.

use std::fs;
use std::path::PathBuf;

use mail_core::{AccountId, Draft, DraftAttachment, DraftFields, Error, Result};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::storage;
use crate::store::{Store, io, remove_dir};

/// What a new draft starts with.
#[derive(Debug, Clone)]
pub struct NewDraft {
    pub account: AccountId,
    pub message_id: String,
    pub fields: DraftFields,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub replying_to: Option<i64>,
    pub attachments: Vec<(DraftAttachment, Vec<u8>)>,
    /// The draft was opened from the Drafts folder, so the server has it already.
    pub on_server: bool,
}

impl NewDraft {
    /// An empty draft.
    pub fn new(account: AccountId, message_id: String) -> Self {
        Self {
            account,
            message_id,
            fields: DraftFields::default(),
            in_reply_to: None,
            references: Vec::new(),
            replying_to: None,
            attachments: Vec::new(),
            on_server: false,
        }
    }
}

/// A draft the server's Drafts folder is behind on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftUpload {
    pub draft: Draft,
    pub version: i64,
    /// Thrown away or sent: remove the server copy, then forget the draft.
    pub discarded: bool,
    /// The server may have an older copy that has to be replaced.
    pub on_server: bool,
}

/// Where an outgoing message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutboxState {
    /// Waiting to send, or retrying after a connection problem.
    Queued,
    /// The server refused it; it waits for the user to retry, edit or delete it.
    Failed,
    /// Sent; only the copy for the Sent folder is left to save.
    Sent,
}

/// One message in the outbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxItem {
    pub id: i64,
    pub account_id: AccountId,
    pub message_id: String,
    pub subject: String,
    pub recipients: Vec<String>,
    pub state: OutboxState,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: i64,
}

/// The parts of a draft kept as JSON.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Content {
    fields: DraftFields,
    attachments: Vec<DraftAttachment>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    replying_to: Option<i64>,
}

const DRAFT_SELECT: &str = "SELECT id, account_id, message_id, content, version, \
     uploaded_version, discarded FROM drafts";

impl Store {
    // Drafts

    pub fn create_draft(&mut self, new: NewDraft) -> Result<Draft> {
        let content = Content {
            fields: new.fields,
            attachments: new.attachments.iter().map(|(a, _)| a.clone()).collect(),
            in_reply_to: new.in_reply_to,
            references: new.references,
            replying_to: new.replying_to,
        };
        self.conn
            .execute(
                "INSERT INTO drafts (account_id, message_id, content, uploaded_version) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    new.account.0,
                    new.message_id,
                    to_json(&content)?,
                    if new.on_server { 1 } else { 0 }
                ],
            )
            .map_err(storage)?;
        let id = self.conn.last_insert_rowid();
        let dir = self.draft_dir(new.account, id);
        if !new.attachments.is_empty() {
            fs::create_dir_all(&dir).map_err(io)?;
        }
        for (index, (_, contents)) in new.attachments.iter().enumerate() {
            if let Err(err) = fs::write(dir.join(index.to_string()), contents) {
                self.forget_draft(id)?;
                return Err(io(err));
            }
        }
        self.draft(id)?
            .ok_or_else(|| Error::Storage("the new draft was not saved".into()))
    }

    /// A draft that has not been sent or thrown away.
    pub fn draft(&self, id: i64) -> Result<Option<Draft>> {
        Ok(self
            .draft_row(id)?
            .filter(|r| !r.discarded)
            .map(|r| r.draft))
    }

    /// The open draft with this Message-ID, e.g. when it is opened from the
    /// Drafts folder while a newer version is still saved only locally.
    pub fn draft_by_message_id(
        &self,
        account: AccountId,
        message_id: &str,
    ) -> Result<Option<Draft>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM drafts WHERE account_id = ?1 AND message_id = ?2 AND discarded = 0 \
                 ORDER BY id DESC LIMIT 1",
                params![account.0, message_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        match id {
            Some(id) => self.draft(id),
            None => Ok(None),
        }
    }

    /// Saves what the user typed. The Drafts folder copy is updated later.
    pub fn save_draft_fields(&self, id: i64, fields: &DraftFields) -> Result<Draft> {
        let mut content = self.open_content(id)?;
        content.fields = fields.clone();
        self.write_content(id, &content)?;
        self.open_draft(id)
    }

    pub fn add_draft_attachment(
        &self,
        id: i64,
        attachment: DraftAttachment,
        contents: &[u8],
    ) -> Result<Draft> {
        let draft = self.open_draft(id)?;
        let mut content = self.open_content(id)?;
        let dir = self.draft_dir(draft.account, id);
        fs::create_dir_all(&dir).map_err(io)?;
        fs::write(dir.join(content.attachments.len().to_string()), contents).map_err(io)?;
        content.attachments.push(attachment);
        self.write_content(id, &content)?;
        self.open_draft(id)
    }

    pub fn remove_draft_attachment(&self, id: i64, index: usize) -> Result<Draft> {
        let draft = self.open_draft(id)?;
        let mut content = self.open_content(id)?;
        if index >= content.attachments.len() {
            return Ok(draft);
        }
        let dir = self.draft_dir(draft.account, id);
        fs::remove_file(dir.join(index.to_string())).map_err(io)?;
        // Later files move down one place, so names keep matching positions.
        for later in index + 1..content.attachments.len() {
            fs::rename(
                dir.join(later.to_string()),
                dir.join((later - 1).to_string()),
            )
            .map_err(io)?;
        }
        content.attachments.remove(index);
        self.write_content(id, &content)?;
        self.open_draft(id)
    }

    /// The contents of a draft's attachments, in order.
    pub fn draft_files(&self, id: i64) -> Result<Vec<Vec<u8>>> {
        let draft = self.open_draft(id)?;
        let dir = self.draft_dir(draft.account, id);
        (0..draft.attachments.len())
            .map(|index| {
                fs::read(dir.join(index.to_string())).map_err(|e| {
                    Error::Storage(format!(
                        "attachment {} of this draft could not be read: {e}",
                        draft.attachments[index].name
                    ))
                })
            })
            .collect()
    }

    /// Throws a draft away. A copy on the server is removed by the next push.
    pub fn discard_draft(&self, id: i64) -> Result<()> {
        let Some(row) = self.draft_row(id)? else {
            return Ok(());
        };
        remove_dir(&self.draft_dir(row.draft.account, id))?;
        if row.uploaded_version == 0 {
            return self.forget_draft(id);
        }
        self.conn
            .execute("UPDATE drafts SET discarded = 1 WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Drafts whose server copy has to be written or removed.
    pub fn drafts_to_upload(&self, account: AccountId) -> Result<Vec<DraftUpload>> {
        let sql = format!(
            "{DRAFT_SELECT} WHERE account_id = ?1 AND (discarded = 1 OR version > uploaded_version) \
             ORDER BY id"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(storage)?;
        let rows = stmt
            .query_map([account.0], draft_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        rows.into_iter()
            .map(|row| {
                let row = row?;
                Ok(DraftUpload {
                    on_server: row.uploaded_version > 0,
                    version: row.version,
                    discarded: row.discarded,
                    draft: row.draft,
                })
            })
            .collect()
    }

    /// Records that the server has `version` of the draft. Edits made while it
    /// was being written leave the draft behind again, so it uploads once more.
    pub fn draft_uploaded(&self, id: i64, version: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE drafts SET uploaded_version = max(uploaded_version, ?2) WHERE id = ?1",
                params![id, version],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Deletes a draft and its files, once the server copy is gone.
    pub fn forget_draft(&self, id: i64) -> Result<()> {
        if let Some(row) = self.draft_row(id)? {
            remove_dir(&self.draft_dir(row.draft.account, id))?;
        }
        self.conn
            .execute("DELETE FROM drafts WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    fn draft_row(&self, id: i64) -> Result<Option<DraftRow>> {
        self.conn
            .query_row(&format!("{DRAFT_SELECT} WHERE id = ?1"), [id], draft_row)
            .optional()
            .map_err(storage)?
            .transpose()
    }

    fn open_draft(&self, id: i64) -> Result<Draft> {
        self.draft(id)?
            .ok_or_else(|| Error::Storage("this draft was sent or deleted".into()))
    }

    fn open_content(&self, id: i64) -> Result<Content> {
        let json: String = self
            .conn
            .query_row(
                "SELECT content FROM drafts WHERE id = ?1 AND discarded = 0",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| Error::Storage("this draft was sent or deleted".into()))?;
        from_json(&json)
    }

    fn write_content(&self, id: i64, content: &Content) -> Result<()> {
        self.conn
            .execute(
                "UPDATE drafts SET content = ?2, version = version + 1, updated_at = unixepoch() \
                 WHERE id = ?1",
                params![id, to_json(content)?],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn draft_dir(&self, account: AccountId, draft: i64) -> PathBuf {
        self.files
            .join(account.0.to_string())
            .join("drafts")
            .join(draft.to_string())
    }

    // Outbox

    /// Queues a built message and closes its draft, in one step, so a message
    /// is never both queued and still open for editing.
    pub fn queue_outgoing(
        &mut self,
        draft: &Draft,
        raw: &[u8],
        copy: &[u8],
        recipients: &[String],
    ) -> Result<i64> {
        let tx = self.conn.transaction().map_err(storage)?;
        tx.execute(
            "INSERT INTO outbox (account_id, message_id, subject, recipients) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                draft.account.0,
                draft.message_id,
                draft.fields.subject.trim(),
                to_json(&recipients)?
            ],
        )
        .map_err(storage)?;
        let id = tx.last_insert_rowid();
        let dir = self.files.join(draft.account.0.to_string()).join("outbox");
        fs::create_dir_all(&dir).map_err(io)?;
        fs::write(dir.join(format!("{id}.eml")), raw).map_err(io)?;
        fs::write(dir.join(format!("{id}.copy.eml")), copy).map_err(io)?;
        tx.commit().map_err(storage)?;
        self.discard_draft(draft.id)?;
        Ok(id)
    }

    /// Every message in the account's outbox, oldest first.
    pub fn outbox(&self, account: AccountId) -> Result<Vec<OutboxItem>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, account_id, message_id, subject, recipients, state, attempts, \
                 last_error, created_at FROM outbox WHERE account_id = ?1 ORDER BY id",
            )
            .map_err(storage)?;
        let rows = stmt
            .query_map([account.0], outbox_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        rows.into_iter().collect()
    }

    pub fn outbox_item(&self, id: i64) -> Result<Option<OutboxItem>> {
        self.conn
            .query_row(
                "SELECT id, account_id, message_id, subject, recipients, state, attempts, \
                 last_error, created_at FROM outbox WHERE id = ?1",
                [id],
                outbox_row,
            )
            .optional()
            .map_err(storage)?
            .transpose()
    }

    /// Messages waiting to send, and messages the server refused.
    pub fn outbox_counts(&self, account: AccountId) -> Result<(u32, u32)> {
        self.conn
            .query_row(
                "SELECT count(*) FILTER (WHERE state = 'queued'), \
                 count(*) FILTER (WHERE state = 'failed') FROM outbox WHERE account_id = ?1",
                [account.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage)
    }

    /// The message as sent over SMTP (`copy = false`) or the copy for Sent.
    pub fn outbox_file(&self, item: &OutboxItem, copy: bool) -> Result<Vec<u8>> {
        fs::read(self.outbox_path(item, copy)).map_err(io)
    }

    pub fn outbox_sent(&self, id: i64) -> Result<()> {
        self.set_outbox(id, "sent", None)
    }

    /// The server refused the message; it waits for the user.
    pub fn outbox_failed(&self, id: i64, error: &str) -> Result<()> {
        self.set_outbox(id, "failed", Some(error))
    }

    /// A connection problem; the message stays queued and is tried again.
    pub fn outbox_attempt_failed(&self, id: i64, error: &str) -> Result<()> {
        self.set_outbox(id, "queued", Some(error))
    }

    /// Puts a refused message back in line.
    pub fn outbox_retry(&self, id: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE outbox SET state = 'queued', last_error = NULL \
                 WHERE id = ?1 AND state = 'failed'",
                [id],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Removes a message and its files from the outbox.
    pub fn outbox_remove(&self, id: i64) -> Result<()> {
        if let Some(item) = self.outbox_item(id)? {
            for copy in [false, true] {
                let _ = fs::remove_file(self.outbox_path(&item, copy));
            }
        }
        self.conn
            .execute("DELETE FROM outbox WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    fn set_outbox(&self, id: i64, state: &str, error: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE outbox SET state = ?2, last_error = ?3, \
                 attempts = attempts + (?3 IS NOT NULL) WHERE id = ?1",
                params![id, state, error],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn outbox_path(&self, item: &OutboxItem, copy: bool) -> PathBuf {
        self.files
            .join(item.account_id.0.to_string())
            .join("outbox")
            .join(if copy {
                format!("{}.copy.eml", item.id)
            } else {
                format!("{}.eml", item.id)
            })
    }
}

struct DraftRow {
    draft: Draft,
    version: i64,
    uploaded_version: i64,
    discarded: bool,
}

fn draft_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<DraftRow>> {
    let id: i64 = r.get(0)?;
    let json: String = r.get(3)?;
    let content: Result<Content> = from_json(&json);
    let (account, message_id, version, uploaded_version, discarded): (i64, String, i64, i64, bool) =
        (r.get(1)?, r.get(2)?, r.get(4)?, r.get(5)?, r.get(6)?);
    Ok(content.map(|c| DraftRow {
        draft: Draft {
            id,
            account: AccountId(account),
            message_id,
            fields: c.fields,
            attachments: c.attachments,
            in_reply_to: c.in_reply_to,
            references: c.references,
            replying_to: c.replying_to,
        },
        version,
        uploaded_version,
        discarded,
    }))
}

fn outbox_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<OutboxItem>> {
    let recipients: String = r.get(4)?;
    let state: String = r.get(5)?;
    let state = match state.as_str() {
        "failed" => OutboxState::Failed,
        "sent" => OutboxState::Sent,
        _ => OutboxState::Queued,
    };
    let item = OutboxItem {
        id: r.get(0)?,
        account_id: AccountId(r.get(1)?),
        message_id: r.get(2)?,
        subject: r.get(3)?,
        recipients: Vec::new(),
        state,
        attempts: r.get(6)?,
        last_error: r.get(7)?,
        created_at: r.get(8)?,
    };
    Ok(from_json(&recipients).map(|recipients| OutboxItem { recipients, ..item }))
}

fn to_json(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Storage(e.to_string()))
}

fn from_json<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T> {
    serde_json::from_str(json).map_err(|e| Error::Storage(format!("unreadable saved data: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::{Endpoint, NewAccount, Security};

    fn store(dir: &std::path::Path) -> (Store, AccountId) {
        let store = Store::open_in_memory(dir).unwrap();
        let endpoint = |host: &str| Endpoint {
            host: host.into(),
            port: 993,
            security: Security::ImplicitTls,
        };
        let account = store
            .add_account(&NewAccount {
                display_name: "Alice".into(),
                email: "alice@example.test".into(),
                username: "alice".into(),
                imap: endpoint("imap.example.test"),
                smtp: endpoint("smtp.example.test"),
            })
            .unwrap();
        (store, account.id)
    }

    fn attachment(name: &str, size: u64) -> DraftAttachment {
        DraftAttachment {
            name: name.into(),
            mime_type: "text/plain".into(),
            size,
        }
    }

    #[test]
    fn drafts_keep_their_fields_and_attachments_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, account) = store(dir.path());
        let draft = store
            .create_draft(NewDraft {
                fields: DraftFields {
                    subject: "Hi".into(),
                    ..DraftFields::default()
                },
                in_reply_to: Some("m@example.test".into()),
                replying_to: Some(7),
                attachments: vec![(attachment("a.txt", 1), b"a".to_vec())],
                ..NewDraft::new(account, "d1@example.test".into())
            })
            .unwrap();
        assert_eq!(draft.fields.subject, "Hi");
        assert_eq!(draft.replying_to, Some(7));

        store
            .add_draft_attachment(draft.id, attachment("b.txt", 1), b"b")
            .unwrap();
        store
            .add_draft_attachment(draft.id, attachment("c.txt", 1), b"c")
            .unwrap();
        let after = store.remove_draft_attachment(draft.id, 0).unwrap();
        let names: Vec<_> = after.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["b.txt", "c.txt"]);
        assert_eq!(
            store.draft_files(draft.id).unwrap(),
            [b"b".to_vec(), b"c".to_vec()]
        );

        let fields = DraftFields {
            to: "bob@example.test".into(),
            body: "Hello".into(),
            ..DraftFields::default()
        };
        store.save_draft_fields(draft.id, &fields).unwrap();
        let reloaded = store.draft(draft.id).unwrap().unwrap();
        assert_eq!(reloaded.fields, fields);
        assert_eq!(reloaded.in_reply_to.as_deref(), Some("m@example.test"));
        assert_eq!(
            store
                .draft_by_message_id(account, "d1@example.test")
                .unwrap()
                .map(|d| d.id),
            Some(draft.id)
        );
    }

    #[test]
    fn drafts_upload_after_edits_and_leave_after_the_server_copy_goes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, account) = store(dir.path());
        let draft = store
            .create_draft(NewDraft::new(account, "d1@example.test".into()))
            .unwrap();
        let pending = store.drafts_to_upload(account).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(!pending[0].on_server);

        // An edit during the upload leaves the draft behind again.
        let version = pending[0].version;
        store
            .save_draft_fields(draft.id, &DraftFields::default())
            .unwrap();
        store.draft_uploaded(draft.id, version).unwrap();
        let again = store.drafts_to_upload(account).unwrap();
        assert_eq!(again.len(), 1);
        assert!(again[0].on_server);
        store.draft_uploaded(draft.id, again[0].version).unwrap();
        assert!(store.drafts_to_upload(account).unwrap().is_empty());

        store.discard_draft(draft.id).unwrap();
        assert!(store.draft(draft.id).unwrap().is_none());
        let removal = store.drafts_to_upload(account).unwrap();
        assert!(removal[0].discarded);
        store.forget_draft(draft.id).unwrap();
        assert!(store.drafts_to_upload(account).unwrap().is_empty());

        // A draft the server never saw is simply deleted.
        let local = store
            .create_draft(NewDraft::new(account, "d2@example.test".into()))
            .unwrap();
        store.discard_draft(local.id).unwrap();
        assert!(store.drafts_to_upload(account).unwrap().is_empty());
    }

    #[test]
    fn outgoing_mail_moves_through_the_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, account) = store(dir.path());
        let draft = store
            .create_draft(NewDraft {
                fields: DraftFields {
                    subject: " Plan ".into(),
                    ..DraftFields::default()
                },
                attachments: vec![(attachment("a.txt", 1), b"a".to_vec())],
                ..NewDraft::new(account, "d1@example.test".into())
            })
            .unwrap();
        let id = store
            .queue_outgoing(&draft, b"raw", b"copy", &["bob@example.test".into()])
            .unwrap();
        assert!(store.draft(draft.id).unwrap().is_none());
        assert!(!dir.path().join("1/drafts/1").exists());

        let item = store.outbox_item(id).unwrap().unwrap();
        assert_eq!(item.subject, "Plan");
        assert_eq!(item.recipients, ["bob@example.test"]);
        assert_eq!(item.state, OutboxState::Queued);
        assert_eq!(store.outbox_file(&item, false).unwrap(), b"raw");
        assert_eq!(store.outbox_file(&item, true).unwrap(), b"copy");
        assert_eq!(store.outbox_counts(account).unwrap(), (1, 0));

        store.outbox_attempt_failed(id, "offline").unwrap();
        let retried = store.outbox_item(id).unwrap().unwrap();
        assert_eq!((retried.state, retried.attempts), (OutboxState::Queued, 1));
        store.outbox_failed(id, "550 no such user").unwrap();
        assert_eq!(store.outbox_counts(account).unwrap(), (0, 1));
        store.outbox_retry(id).unwrap();
        store.outbox_sent(id).unwrap();
        let sent = store.outbox_item(id).unwrap().unwrap();
        assert_eq!((sent.state, sent.last_error), (OutboxState::Sent, None));
        assert_eq!(store.outbox_counts(account).unwrap(), (0, 0));

        store.outbox_remove(id).unwrap();
        assert!(store.outbox(account).unwrap().is_empty());
        assert!(!dir.path().join("1/outbox/1.eml").exists());
    }
}
