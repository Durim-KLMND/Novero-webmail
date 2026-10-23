//! Typed access to the local cache.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use std::collections::HashSet;

use mail_core::{
    Account, AccountId, AuthMethod, Endpoint, Error, Flags, FolderId, FolderRole, MessageHeader,
    NewAccount, OAuthProvider, Op, RemoteFolder, Result, Security,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use crate::mailbox::{Mailbox, MessagePage, RowCache};
use crate::storage;

/// The local cache: one SQLite database plus raw messages as `.eml` files.
pub struct Store {
    pub(crate) conn: Connection,
    pub(crate) files: PathBuf,
    /// The conversation list last built, reused while nothing changed.
    pub(crate) rows: RefCell<Option<RowCache>>,
}

/// A folder in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: FolderId,
    pub account_id: AccountId,
    pub name: String,
    pub delimiter: Option<String>,
    pub role: FolderRole,
    pub total: u32,
    pub unread: u32,
}

/// What the sync engine needs to know about a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderState {
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub min_uid: u32,
    pub max_uid: u32,
    pub count: u32,
}

/// Where a message lives, as the operation queue needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLocation {
    pub id: i64,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub folder_name: String,
    pub folder_role: FolderRole,
    pub uid_validity: Option<u32>,
    pub uid: u32,
    pub flags: Flags,
}

/// A queued operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedOp {
    pub id: i64,
    pub op: Op,
    pub attempts: u32,
}

/// Where a message lives on the server and on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSource {
    pub account_id: AccountId,
    pub folder_name: String,
    pub uid: u32,
    pub raw_path: Option<PathBuf>,
}

impl Store {
    /// Opens the cache in `dir`, creating it if needed.
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).map_err(io)?;
        let store = Self {
            conn: crate::open(&dir.join("mail.db"))?,
            files: dir.join("messages"),
            rows: RefCell::default(),
        };
        store.build_contacts()?;
        Ok(store)
    }

    /// An in-memory database that writes message files under `files`. For tests.
    pub fn open_in_memory(files: &Path) -> Result<Self> {
        Ok(Self {
            conn: crate::open_in_memory()?,
            files: files.to_path_buf(),
            rows: RefCell::default(),
        })
    }

    // Accounts

    pub fn add_account(&self, new: &NewAccount) -> Result<Account> {
        self.conn
            .execute(
                "INSERT INTO accounts (display_name, email, username, imap_host, imap_port, \
                 imap_security, smtp_host, smtp_port, smtp_security, auth_kind, oauth_provider) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    new.display_name,
                    new.email,
                    new.username,
                    new.imap.host,
                    new.imap.port,
                    new.imap.security.as_str(),
                    new.smtp.host,
                    new.smtp.port,
                    new.smtp.security.as_str(),
                    match new.auth {
                        AuthMethod::Password => "password",
                        AuthMethod::OAuth2 { .. } => "oauth2",
                    },
                    match new.auth {
                        AuthMethod::Password => None,
                        AuthMethod::OAuth2 { provider } => Some(provider.as_str()),
                    },
                ],
            )
            .map_err(storage)?;
        let id = AccountId(self.conn.last_insert_rowid());
        self.account(id)?
            .ok_or_else(|| Error::Storage("account vanished after insert".into()))
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{ACCOUNT_SELECT} ORDER BY id"))
            .map_err(storage)?;
        stmt.query_map([], account_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    pub fn account(&self, id: AccountId) -> Result<Option<Account>> {
        self.conn
            .query_row(
                &format!("{ACCOUNT_SELECT} WHERE id = ?1"),
                [id.0],
                account_row,
            )
            .optional()
            .map_err(storage)
    }

    /// Changes where an account's mail comes from and goes to, and the name
    /// it signs in with. Cached mail stays; the next sync sorts out any
    /// folder the new server doesn't have.
    pub fn update_servers(
        &self,
        id: AccountId,
        username: &str,
        imap: &Endpoint,
        smtp: &Endpoint,
    ) -> Result<Account> {
        let changed = self
            .conn
            .execute(
                "UPDATE accounts SET username = ?2, imap_host = ?3, imap_port = ?4, \
                 imap_security = ?5, smtp_host = ?6, smtp_port = ?7, smtp_security = ?8 \
                 WHERE id = ?1",
                params![
                    id.0,
                    username,
                    imap.host,
                    imap.port,
                    imap.security.as_str(),
                    smtp.host,
                    smtp.port,
                    smtp.security.as_str(),
                ],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err(Error::Storage("that account no longer exists".into()));
        }
        self.account(id)?
            .ok_or_else(|| Error::Storage("that account no longer exists".into()))
    }

    /// Deletes the account, its folders and messages, and its message files.
    pub fn remove_account(&self, id: AccountId) -> Result<()> {
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?1", [id.0])
            .map_err(storage)?;
        remove_dir(&self.files.join(id.0.to_string()))
    }

    // Settings

    /// A stored preference, as JSON.
    pub fn setting(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?;
        Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
    }

    pub fn set_setting(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) \
                 ON CONFLICT (key) DO UPDATE SET value = ?2",
                params![key, value.to_string()],
            )
            .map_err(storage)?;
        Ok(())
    }

    // Folders

    /// Makes the local folder list match the server's and returns the folder ids.
    /// Local folders the server no longer lists are deleted with their messages.
    pub fn sync_folders(
        &mut self,
        account: AccountId,
        remote: &[RemoteFolder],
    ) -> Result<Vec<(FolderId, RemoteFolder)>> {
        let tx = self.conn.transaction().map_err(storage)?;
        let mut ids = Vec::with_capacity(remote.len());
        for folder in remote {
            tx.execute(
                "INSERT INTO folders (account_id, name, delimiter, role) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (account_id, name) DO UPDATE SET delimiter = ?3, role = ?4",
                params![
                    account.0,
                    folder.name,
                    folder.delimiter,
                    folder.role.as_str()
                ],
            )
            .map_err(storage)?;
            let id: i64 = tx
                .query_row(
                    "SELECT id FROM folders WHERE account_id = ?1 AND name = ?2",
                    params![account.0, folder.name],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            ids.push((FolderId(id), folder.clone()));
        }
        let keep: Vec<i64> = ids.iter().map(|(id, _)| id.0).collect();
        let mut gone = Vec::new();
        {
            let mut stmt = tx
                .prepare("SELECT id FROM folders WHERE account_id = ?1")
                .map_err(storage)?;
            for id in stmt
                .query_map([account.0], |r| r.get::<_, i64>(0))
                .map_err(storage)?
            {
                let id = id.map_err(storage)?;
                if !keep.contains(&id) {
                    gone.push(id);
                }
            }
        }
        for id in &gone {
            tx.execute("DELETE FROM folders WHERE id = ?1", [id])
                .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        for id in gone {
            remove_dir(&self.folder_dir(account, FolderId(id)))?;
        }
        Ok(ids)
    }

    /// Folders of an account in sidebar order, with message counts.
    pub fn list_folders(&self, account: AccountId) -> Result<Vec<Folder>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT f.id, f.account_id, f.name, f.delimiter, f.role, \
                 count(m.id), coalesce(sum(m.flags & 1 = 0), 0) \
                 FROM folders f LEFT JOIN messages m ON m.folder_id = f.id \
                 WHERE f.account_id = ?1 GROUP BY f.id",
            )
            .map_err(storage)?;
        let mut folders: Vec<Folder> = stmt
            .query_map([account.0], |r| {
                Ok(Folder {
                    id: FolderId(r.get(0)?),
                    account_id: AccountId(r.get(1)?),
                    name: r.get(2)?,
                    delimiter: r.get(3)?,
                    role: FolderRole::parse(r.get::<_, Option<String>>(4)?.as_deref()),
                    total: r.get(5)?,
                    unread: r.get(6)?,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        sort_folders(&mut folders);
        Ok(folders)
    }

    /// Unread messages in the inboxes of every account, leaving out snoozed
    /// ones: the number on the taskbar icon.
    pub fn inbox_unread(&self) -> Result<u32> {
        self.conn
            .query_row(
                "SELECT count(*) FROM messages m JOIN folders f ON f.id = m.folder_id \
                 WHERE f.role = 'inbox' AND m.flags & 1 = 0 \
                 AND m.id NOT IN (SELECT message FROM snoozes)",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
    }

    pub fn folder_state(&self, folder: FolderId) -> Result<FolderState> {
        self.conn
            .query_row(
                "SELECT f.uidvalidity, f.uidnext, f.highestmodseq, coalesce(min(m.uid), 0), \
                 coalesce(max(m.uid), 0), count(m.id) \
                 FROM folders f LEFT JOIN messages m ON m.folder_id = f.id WHERE f.id = ?1",
                [folder.0],
                |r| {
                    Ok(FolderState {
                        uid_validity: r.get(0)?,
                        uid_next: r.get(1)?,
                        highest_modseq: r.get::<_, Option<i64>>(2)?.map(|m| m as u64),
                        min_uid: r.get(3)?,
                        max_uid: r.get(4)?,
                        count: r.get(5)?,
                    })
                },
            )
            .map_err(storage)
    }

    /// One folder with its counts.
    pub fn folder(&self, id: FolderId) -> Result<Option<Folder>> {
        let account: Option<i64> = self
            .conn
            .query_row(
                "SELECT account_id FROM folders WHERE id = ?1",
                [id.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        Ok(match account {
            Some(account) => self
                .list_folders(AccountId(account))?
                .into_iter()
                .find(|f| f.id == id),
            None => None,
        })
    }

    /// A folder's role, without counting its messages.
    pub fn folder_role(&self, id: FolderId) -> Result<Option<FolderRole>> {
        self.conn
            .query_row("SELECT role FROM folders WHERE id = ?1", [id.0], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()
            .map(|role| role.map(|r| FolderRole::parse(r.as_deref())))
            .map_err(storage)
    }

    /// Looks up a folder of an account by its server name.
    pub fn folder_id(&self, account: AccountId, name: &str) -> Result<Option<FolderId>> {
        self.conn
            .query_row(
                "SELECT id FROM folders WHERE account_id = ?1 AND name = ?2",
                params![account.0, name],
                |r| r.get(0).map(FolderId),
            )
            .optional()
            .map_err(storage)
    }

    /// The server name of the account's folder with `role`, if it has one.
    pub fn folder_with_role(&self, account: AccountId, role: FolderRole) -> Result<Option<String>> {
        let Some(role) = role.as_str() else {
            return Ok(None);
        };
        self.conn
            .query_row(
                "SELECT name FROM folders WHERE account_id = ?1 AND role = ?2 ORDER BY id LIMIT 1",
                params![account.0, role],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)
    }

    /// Renames a folder and its subfolders in the cache after the server did,
    /// so their messages stay instead of downloading again.
    pub fn rename_folder(&mut self, account: AccountId, from: &str, to: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(storage)?;
        let delimiter: Option<String> = tx
            .query_row(
                "SELECT delimiter FROM folders WHERE account_id = ?1 AND name = ?2",
                params![account.0, from],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| Error::Storage(format!("folder {from} is not cached")))?;
        let mut renames = vec![(from.to_owned(), to.to_owned())];
        if let Some(delimiter) = delimiter.filter(|d| !d.is_empty()) {
            let prefix = format!("{from}{delimiter}");
            let mut stmt = tx
                .prepare("SELECT name FROM folders WHERE account_id = ?1")
                .map_err(storage)?;
            for name in stmt
                .query_map([account.0], |r| r.get::<_, String>(0))
                .map_err(storage)?
            {
                let name = name.map_err(storage)?;
                if let Some(rest) = name.strip_prefix(&prefix) {
                    renames.push((name.clone(), format!("{to}{delimiter}{rest}")));
                }
            }
        }
        for (old, new) in renames {
            tx.execute(
                "UPDATE folders SET name = ?3 WHERE account_id = ?1 AND name = ?2",
                params![account.0, old, new],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }

    /// Whether queued changes still refer to `name` or a folder inside it,
    /// so it cannot be renamed or deleted until the server has them.
    pub fn folder_has_pending(
        &self,
        account: AccountId,
        name: &str,
        delimiter: Option<&str>,
    ) -> Result<bool> {
        let within = |folder: &str| {
            folder == name
                || delimiter.is_some_and(|d| {
                    folder
                        .strip_prefix(name)
                        .is_some_and(|rest| rest.starts_with(d))
                })
        };
        Ok(self
            .pending_ops(account)?
            .iter()
            .any(|q| within(q.op.folder()) || matches!(&q.op, Op::Move { to, .. } if within(to))))
    }

    /// The messages of a folder, or only its unread ones.
    pub fn folder_messages(&self, folder: FolderId, unread_only: bool) -> Result<Vec<i64>> {
        let sql = if unread_only {
            "SELECT id FROM messages WHERE folder_id = ?1 AND flags & 1 = 0"
        } else {
            "SELECT id FROM messages WHERE folder_id = ?1"
        };
        let mut stmt = self.conn.prepare(sql).map_err(storage)?;
        stmt.query_map([folder.0], |r| r.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// Discards a folder's cached messages and sync state, for when the server's
    /// UIDVALIDITY changed.
    pub fn reset_folder(&self, account: AccountId, folder: FolderId) -> Result<()> {
        self.conn
            .execute("DELETE FROM messages WHERE folder_id = ?1", [folder.0])
            .map_err(storage)?;
        self.conn
            .execute(
                "UPDATE folders SET uidnext = NULL, highestmodseq = NULL WHERE id = ?1",
                [folder.0],
            )
            .map_err(storage)?;
        remove_dir(&self.folder_dir(account, folder))
    }

    pub fn set_folder_synced(
        &self,
        folder: FolderId,
        uid_validity: Option<u32>,
        uid_next: Option<u32>,
        highest_modseq: Option<u64>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE folders SET uidvalidity = ?2, uidnext = ?3, highestmodseq = ?4, \
                 last_sync = unixepoch() WHERE id = ?1",
                params![
                    folder.0,
                    uid_validity,
                    uid_next,
                    highest_modseq.map(|m| m as i64)
                ],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// UIDs cached for a folder, ascending.
    pub fn local_uids(&self, folder: FolderId) -> Result<Vec<u32>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT uid FROM messages WHERE folder_id = ?1 ORDER BY uid")
            .map_err(storage)?;
        stmt.query_map([folder.0], |r| r.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// Drops messages the server no longer has, with their files.
    pub fn remove_uids(
        &mut self,
        account: AccountId,
        folder: FolderId,
        uids: &[u32],
    ) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction().map_err(storage)?;
        {
            let mut stmt = tx
                .prepare("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2")
                .map_err(storage)?;
            for uid in uids {
                stmt.execute(params![folder.0, uid]).map_err(storage)?;
            }
        }
        tx.commit().map_err(storage)?;
        let dir = self.folder_dir(account, folder);
        for uid in uids {
            let _ = fs::remove_file(dir.join(format!("{uid}.eml")));
        }
        Ok(())
    }

    /// Replaces the flags of cached messages, as the server reported them.
    pub fn update_flags(&mut self, folder: FolderId, flags: &[(u32, Flags)]) -> Result<()> {
        let tx = self.conn.transaction().map_err(storage)?;
        {
            let mut stmt = tx
                .prepare("UPDATE messages SET flags = ?3 WHERE folder_id = ?1 AND uid = ?2")
                .map_err(storage)?;
            for (uid, f) in flags {
                stmt.execute(params![folder.0, uid, f.to_bits()])
                    .map_err(storage)?;
            }
        }
        tx.commit().map_err(storage)
    }

    // Messages

    /// Inserts headers, or updates flags of messages already cached. New
    /// messages join their conversation. Returns the new messages' ids,
    /// matching `headers` (None for those already cached).
    pub fn insert_headers(
        &mut self,
        folder: FolderId,
        headers: &[MessageHeader],
    ) -> Result<Vec<Option<i64>>> {
        let tx = self.conn.transaction().map_err(storage)?;
        let (account, role) = tx
            .query_row(
                "SELECT account_id, role FROM folders WHERE id = ?1",
                [folder.0],
                |r| Ok((AccountId(r.get(0)?), r.get::<_, Option<String>>(1)?)),
            )
            .map_err(storage)?;
        let role = FolderRole::parse(role.as_deref());
        let mut inserted = Vec::with_capacity(headers.len());
        for h in headers {
            let updated = tx
                .prepare_cached("UPDATE messages SET flags = ?3 WHERE folder_id = ?1 AND uid = ?2")
                .map_err(storage)?
                .execute(params![folder.0, h.uid, h.flags.to_bits()])
                .map_err(storage)?;
            if updated > 0 {
                inserted.push(None);
                continue;
            }
            tx.prepare_cached(
                "INSERT INTO messages (folder_id, uid, message_id, from_addr, to_addrs, cc_addrs, \
                 subject, date, size, flags, has_attachments) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            .map_err(storage)?
            .execute(params![
                folder.0,
                h.uid,
                h.message_id,
                h.from,
                h.to,
                h.cc,
                h.subject,
                h.date,
                h.size,
                h.flags.to_bits(),
                h.has_attachments,
            ])
            .map_err(storage)?;
            let id = tx.last_insert_rowid();
            crate::mailbox::assign_thread(&tx, account, id, h)?;
            crate::contacts::note_header(&tx, role, h)?;
            inserted.push(Some(id));
        }
        tx.commit().map_err(storage)?;
        Ok(inserted)
    }

    /// Messages in a folder, newest first, one row each.
    pub fn list_messages(&self, folder: FolderId, offset: u32, limit: u32) -> Result<MessagePage> {
        self.list(&Mailbox::Folder { id: folder }, false, offset, limit)
    }

    pub fn message_source(&self, message: i64) -> Result<MessageSource> {
        self.conn
            .query_row(
                "SELECT f.account_id, f.name, m.uid, b.raw_path FROM messages m \
                 JOIN folders f ON f.id = m.folder_id \
                 LEFT JOIN message_bodies b ON b.message_id = m.id WHERE m.id = ?1",
                [message],
                |r| {
                    Ok(MessageSource {
                        account_id: AccountId(r.get(0)?),
                        folder_name: r.get(1)?,
                        uid: r.get(2)?,
                        raw_path: r.get::<_, Option<String>>(3)?.map(PathBuf::from),
                    })
                },
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| Error::Storage(format!("message {message} not found")))
    }

    /// Saves a fetched message to disk and records where it is.
    pub fn save_raw(&self, message: i64, raw: &[u8]) -> Result<PathBuf> {
        let (account, folder, uid): (i64, i64, u32) = self
            .conn
            .query_row(
                "SELECT f.account_id, m.folder_id, m.uid FROM messages m \
                 JOIN folders f ON f.id = m.folder_id WHERE m.id = ?1",
                [message],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(storage)?;
        let dir = self.folder_dir(AccountId(account), FolderId(folder));
        fs::create_dir_all(&dir).map_err(io)?;
        let path = dir.join(format!("{uid}.eml"));
        fs::write(&path, raw).map_err(io)?;
        self.conn
            .execute(
                "INSERT INTO message_bodies (message_id, raw_path) VALUES (?1, ?2) \
                 ON CONFLICT (message_id) DO UPDATE SET raw_path = ?2",
                params![message, path.to_string_lossy()],
            )
            .map_err(storage)?;
        self.conn
            .execute(
                "UPDATE messages SET body_state = 'fetched' WHERE id = ?1",
                [message],
            )
            .map_err(storage)?;
        Ok(path)
    }

    /// Where each message lives, for building operations. Unknown ids are skipped.
    pub fn locate(&self, messages: &[i64]) -> Result<Vec<MessageLocation>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT m.id, f.account_id, f.id, f.name, f.role, f.uidvalidity, m.uid, m.flags \
                 FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.id = ?1",
            )
            .map_err(storage)?;
        let mut out = Vec::with_capacity(messages.len());
        for id in messages {
            let found = stmt
                .query_row([id], |r| {
                    Ok(MessageLocation {
                        id: r.get(0)?,
                        account_id: AccountId(r.get(1)?),
                        folder_id: FolderId(r.get(2)?),
                        folder_name: r.get(3)?,
                        folder_role: FolderRole::parse(r.get::<_, Option<String>>(4)?.as_deref()),
                        uid_validity: r.get(5)?,
                        uid: r.get(6)?,
                        flags: Flags::from_bits(r.get(7)?),
                    })
                })
                .optional()
                .map_err(storage)?;
            out.extend(found);
        }
        Ok(out)
    }

    /// Applies `op` to the cache right away and queues it for the server.
    /// Moved and deleted messages leave the cache; a move shows up in the
    /// target folder after the server has done it and that folder syncs.
    pub fn apply_op(&mut self, account: AccountId, op: &Op) -> Result<i64> {
        let folder = self
            .folder_id(account, op.folder())?
            .ok_or_else(|| Error::Storage(format!("folder {} is not cached", op.folder())))?;
        let payload = serde_json::to_string(op).map_err(|e| Error::Storage(e.to_string()))?;
        let tx = self.conn.transaction().map_err(storage)?;
        {
            let mut stmt = match op {
                Op::SetFlag { flag, value, .. } => {
                    let sql = if *value {
                        format!("UPDATE messages SET flags = flags | {} WHERE folder_id = ?1 AND uid = ?2", flag.bit())
                    } else {
                        format!("UPDATE messages SET flags = flags & ~{} WHERE folder_id = ?1 AND uid = ?2", flag.bit())
                    };
                    tx.prepare(&sql)
                }
                Op::Move { .. } | Op::Expunge { .. } => {
                    tx.prepare("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2")
                }
            }
            .map_err(storage)?;
            for uid in op.uids() {
                stmt.execute(params![folder.0, uid]).map_err(storage)?;
            }
        }
        tx.execute(
            "INSERT INTO op_queue (account_id, kind, payload) VALUES (?1, ?2, ?3)",
            params![account.0, op.kind(), payload],
        )
        .map_err(storage)?;
        let id = tx.last_insert_rowid();
        tx.commit().map_err(storage)?;
        if matches!(op, Op::Move { .. } | Op::Expunge { .. }) {
            let dir = self.folder_dir(account, folder);
            for uid in op.uids() {
                let _ = fs::remove_file(dir.join(format!("{uid}.eml")));
            }
        }
        Ok(id)
    }

    /// Operations still waiting for the server, oldest first.
    pub fn pending_ops(&self, account: AccountId) -> Result<Vec<QueuedOp>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, payload, attempts FROM op_queue WHERE account_id = ?1 ORDER BY id",
            )
            .map_err(storage)?;
        let rows: Vec<(i64, String, u32)> = stmt
            .query_map([account.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        rows.into_iter()
            .map(|(id, payload, attempts)| {
                let op = serde_json::from_str(&payload).map_err(|e| {
                    Error::Storage(format!("queued change {id} is unreadable: {e}"))
                })?;
                Ok(QueuedOp { id, op, attempts })
            })
            .collect()
    }

    pub fn pending_count(&self, account: AccountId) -> Result<u32> {
        self.conn
            .query_row(
                "SELECT count(*) FROM op_queue WHERE account_id = ?1",
                [account.0],
                |r| r.get(0),
            )
            .map_err(storage)
    }

    /// UIDs in `folder` that queued operations still refer to. Sync leaves
    /// these alone so it does not undo a change the server has not seen yet.
    pub fn pending_uids(&self, account: AccountId, folder: &str) -> Result<HashSet<u32>> {
        Ok(self
            .pending_ops(account)?
            .into_iter()
            .filter(|q| q.op.folder() == folder)
            .flat_map(|q| q.op.uids().to_vec())
            .collect())
    }

    /// Removes an operation the server has applied, or refused.
    pub fn finish_op(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM op_queue WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Records a failed attempt that will be retried.
    pub fn op_failed(&self, id: i64, error: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE op_queue SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
                params![id, error],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn folder_dir(&self, account: AccountId, folder: FolderId) -> PathBuf {
        self.files
            .join(account.0.to_string())
            .join(folder.0.to_string())
    }
}

/// Sidebar order: the special folders first, then the others by name, each
/// followed by its subfolders. Folders inside the inbox follow it.
fn sort_folders(folders: &mut [Folder]) {
    let inbox = folders
        .iter()
        .find(|f| f.role == FolderRole::Inbox)
        .map(|f| f.name.to_lowercase());
    folders.sort_by_cached_key(|f| {
        let path: Vec<String> = match f.delimiter.as_deref().filter(|d| !d.is_empty()) {
            Some(d) => f.name.to_lowercase().split(d).map(str::to_owned).collect(),
            None => vec![f.name.to_lowercase()],
        };
        let rank =
            if f.role == FolderRole::Other && path.len() > 1 && inbox.as_ref() == path.first() {
                FolderRole::Inbox.sort_rank()
            } else {
                f.role.sort_rank()
            };
        // A special folder sorts as one name, wherever the server keeps it.
        let path = if f.role == FolderRole::Other {
            path
        } else {
            vec![f.name.to_lowercase()]
        };
        (rank, path)
    });
}

const ACCOUNT_SELECT: &str = "SELECT id, display_name, email, username, imap_host, imap_port, \
     imap_security, smtp_host, smtp_port, smtp_security, oauth_provider, \
     COALESCE((SELECT text FROM signatures WHERE id = signature_new), ''), \
     COALESCE((SELECT html FROM signatures WHERE id = signature_new), ''), \
     COALESCE((SELECT text FROM signatures WHERE id = signature_reply), ''), \
     COALESCE((SELECT html FROM signatures WHERE id = signature_reply), ''), \
     signature_new, signature_reply FROM accounts";

fn account_row(r: &Row<'_>) -> rusqlite::Result<Account> {
    let security = |i: usize| -> rusqlite::Result<Security> {
        let value: String = r.get(i)?;
        Security::parse(&value).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                i,
                rusqlite::types::Type::Text,
                format!("unknown security {value}").into(),
            )
        })
    };
    Ok(Account {
        id: AccountId(r.get(0)?),
        display_name: r.get(1)?,
        email: r.get(2)?,
        username: r.get(3)?,
        imap: Endpoint {
            host: r.get(4)?,
            port: r.get(5)?,
            security: security(6)?,
        },
        smtp: Endpoint {
            host: r.get(7)?,
            port: r.get(8)?,
            security: security(9)?,
        },
        auth: match r.get::<_, Option<String>>(10)? {
            None => AuthMethod::Password,
            Some(value) => AuthMethod::OAuth2 {
                provider: OAuthProvider::parse(&value).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        format!("unknown sign-in provider {value}").into(),
                    )
                })?,
            },
        },
        signature: r.get(11)?,
        signature_html: r.get(12)?,
        reply_signature: r.get(13)?,
        reply_signature_html: r.get(14)?,
        signature_new: r.get(15)?,
        signature_reply: r.get(16)?,
    })
}

pub(crate) fn remove_dir(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
        _ => Ok(()),
    }
}

pub(crate) fn io(err: std::io::Error) -> Error {
    Error::Storage(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(host: &str, port: u16) -> Endpoint {
        Endpoint {
            host: host.into(),
            port,
            security: Security::ImplicitTls,
        }
    }

    fn new_account(email: &str) -> NewAccount {
        NewAccount {
            auth: Default::default(),
            display_name: "Alice".into(),
            email: email.into(),
            username: email.into(),
            imap: endpoint("imap.example.test", 993),
            smtp: endpoint("smtp.example.test", 465),
        }
    }

    fn folder(name: &str, role: FolderRole) -> RemoteFolder {
        RemoteFolder {
            name: name.into(),
            delimiter: Some("/".into()),
            role,
            selectable: true,
        }
    }

    fn header(uid: u32, date: i64, seen: bool) -> MessageHeader {
        MessageHeader {
            uid,
            subject: Some(format!("Message {uid}")),
            date: Some(date),
            flags: Flags {
                seen,
                ..Flags::default()
            },
            ..MessageHeader::default()
        }
    }

    #[test]
    fn accounts_round_trip_and_several_can_coexist() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory(dir.path()).unwrap();
        let a = store.add_account(&new_account("a@example.test")).unwrap();
        let b = store
            .add_account(&NewAccount {
                auth: AuthMethod::OAuth2 {
                    provider: OAuthProvider::Google,
                },
                ..new_account("b@example.test")
            })
            .unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(a.imap.port, 993);
        assert_eq!(a.smtp.security, Security::ImplicitTls);
        assert_eq!(a.auth, AuthMethod::Password);
        assert_eq!(
            b.auth,
            AuthMethod::OAuth2 {
                provider: OAuthProvider::Google
            }
        );
        let all = store.list_accounts().unwrap();
        assert_eq!(all, vec![a.clone(), b]);
        store.remove_account(a.id).unwrap();
        assert_eq!(store.list_accounts().unwrap().len(), 1);
    }

    #[test]
    fn server_settings_can_be_changed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory(dir.path()).unwrap();
        let a = store.add_account(&new_account("a@example.test")).unwrap();
        let imap = Endpoint {
            host: "mail.example.test".into(),
            port: 143,
            security: Security::StartTls,
        };
        let smtp = Endpoint {
            host: "out.example.test".into(),
            port: 587,
            security: Security::StartTls,
        };
        let changed = store.update_servers(a.id, "alice", &imap, &smtp).unwrap();
        assert_eq!((changed.imap, changed.smtp), (imap.clone(), smtp.clone()));
        assert_eq!(changed.username, "alice");
        assert_eq!((changed.email, changed.auth), (a.email, a.auth));
        assert_eq!(store.account(a.id).unwrap().unwrap().username, "alice");
        store.remove_account(a.id).unwrap();
        assert!(store.update_servers(a.id, "alice", &imap, &smtp).is_err());
    }

    #[test]
    fn folders_follow_the_server_and_sort_by_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        store
            .sync_folders(
                acc,
                &[
                    folder("Zeta", FolderRole::Other),
                    folder("Trash", FolderRole::Trash),
                    folder("INBOX", FolderRole::Inbox),
                    folder("Old", FolderRole::Other),
                ],
            )
            .unwrap();
        let ids = store
            .sync_folders(
                acc,
                &[
                    folder("Zeta", FolderRole::Other),
                    folder("Trash", FolderRole::Trash),
                    folder("INBOX", FolderRole::Inbox),
                ],
            )
            .unwrap();
        assert_eq!(ids.len(), 3);
        let names: Vec<_> = store
            .list_folders(acc)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["INBOX", "Trash", "Zeta"]);
    }

    #[test]
    fn subfolders_follow_their_parent() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let remote = [
            folder("Projects-old", FolderRole::Other),
            folder("Projects/Beta", FolderRole::Other),
            folder("INBOX", FolderRole::Inbox),
            folder("Projects", FolderRole::Other),
            folder("INBOX/Receipts", FolderRole::Other),
            folder("Projects/Alpha", FolderRole::Other),
            folder("Sent", FolderRole::Sent),
        ];
        store.sync_folders(acc, &remote).unwrap();
        let names: Vec<_> = store
            .list_folders(acc)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(
            names,
            [
                "INBOX",
                "INBOX/Receipts",
                "Sent",
                "Projects",
                "Projects/Alpha",
                "Projects/Beta",
                "Projects-old"
            ]
        );
    }

    #[test]
    fn renamed_folders_keep_their_messages_and_subfolders() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let remote = [
            folder("INBOX", FolderRole::Inbox),
            folder("Work", FolderRole::Other),
            folder("Work/2026", FolderRole::Other),
            folder("Workshop", FolderRole::Other),
        ];
        let ids = store.sync_folders(acc, &remote).unwrap();
        let work = ids[1].0;
        store
            .insert_headers(work, &[header(1, 100, false), header(2, 200, true)])
            .unwrap();
        store.rename_folder(acc, "Work", "Jobs").unwrap();
        let folders = store.list_folders(acc).unwrap();
        let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["INBOX", "Jobs", "Jobs/2026", "Workshop"]);
        let jobs = folders.iter().find(|f| f.name == "Jobs").unwrap();
        assert_eq!((jobs.id, jobs.total, jobs.unread), (work, 2, 1));
        assert_eq!(store.folder_messages(work, true).unwrap().len(), 1);
        assert_eq!(store.folder_messages(work, false).unwrap().len(), 2);
    }

    #[test]
    fn folders_with_queued_changes_are_busy() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let ids = store
            .sync_folders(
                acc,
                &[
                    folder("INBOX", FolderRole::Inbox),
                    folder("Work", FolderRole::Other),
                ],
            )
            .unwrap();
        store
            .insert_headers(ids[0].0, &[header(1, 100, false)])
            .unwrap();
        assert!(!store.folder_has_pending(acc, "Work", Some("/")).unwrap());
        store
            .apply_op(
                acc,
                &Op::Move {
                    folder: "INBOX".into(),
                    uid_validity: None,
                    uids: vec![1],
                    to: "Work/Sub".into(),
                },
            )
            .unwrap();
        assert!(store.folder_has_pending(acc, "Work", Some("/")).unwrap());
        assert!(
            !store
                .folder_has_pending(acc, "Workshop", Some("/"))
                .unwrap()
        );
    }

    #[test]
    fn headers_list_newest_first_with_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let (inbox, _) = store
            .sync_folders(acc, &[folder("INBOX", FolderRole::Inbox)])
            .unwrap()[0]
            .clone();
        store
            .insert_headers(
                inbox,
                &[
                    header(1, 100, true),
                    header(2, 300, false),
                    header(3, 200, false),
                ],
            )
            .unwrap();
        // Re-inserting updates flags instead of duplicating.
        store
            .insert_headers(inbox, &[header(2, 300, true)])
            .unwrap();

        let page = store.list_messages(inbox, 0, 2).unwrap();
        assert_eq!(page.total, 3);
        let uids: Vec<_> = page.items.iter().map(|m| m.uid).collect();
        assert_eq!(uids, [2, 3]);
        assert!(page.items[0].flags.seen);

        let state = store.folder_state(inbox).unwrap();
        assert_eq!((state.min_uid, state.max_uid, state.count), (1, 3, 3));
        let folders = store.list_folders(acc).unwrap();
        assert_eq!((folders[0].total, folders[0].unread), (3, 1));
    }

    #[test]
    fn raw_messages_are_saved_to_disk_and_removed_with_the_account() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let (inbox, _) = store
            .sync_folders(acc, &[folder("INBOX", FolderRole::Inbox)])
            .unwrap()[0]
            .clone();
        store.insert_headers(inbox, &[header(7, 1, false)]).unwrap();
        let id = store.list_messages(inbox, 0, 1).unwrap().items[0].id;

        assert_eq!(store.message_source(id).unwrap().raw_path, None);
        let path = store.save_raw(id, b"Subject: hi\r\n\r\nbody").unwrap();
        let source = store.message_source(id).unwrap();
        assert_eq!(source.raw_path.as_deref(), Some(path.as_path()));
        assert_eq!((source.folder_name.as_str(), source.uid), ("INBOX", 7));
        assert!(path.exists());

        store.remove_account(acc).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn reset_folder_drops_cached_messages() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let (inbox, _) = store
            .sync_folders(acc, &[folder("INBOX", FolderRole::Inbox)])
            .unwrap()[0]
            .clone();
        store.insert_headers(inbox, &[header(1, 1, false)]).unwrap();
        store
            .set_folder_synced(inbox, Some(42), Some(2), Some(9))
            .unwrap();
        let state = store.folder_state(inbox).unwrap();
        assert_eq!((state.uid_next, state.highest_modseq), (Some(2), Some(9)));
        assert_eq!(store.folder_state(inbox).unwrap().uid_validity, Some(42));
        store.reset_folder(acc, inbox).unwrap();
        assert_eq!(store.folder_state(inbox).unwrap().count, 0);
    }

    #[test]
    fn local_changes_apply_at_once_and_queue_for_the_server() {
        use mail_core::FlagKind;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let folders = store
            .sync_folders(
                acc,
                &[
                    folder("INBOX", FolderRole::Inbox),
                    folder("Archive", FolderRole::Archive),
                ],
            )
            .unwrap();
        let inbox = folders[0].0;
        store
            .insert_headers(
                inbox,
                &[
                    header(1, 1, false),
                    header(2, 2, false),
                    header(3, 3, false),
                ],
            )
            .unwrap();
        store
            .set_folder_synced(inbox, Some(5), Some(4), None)
            .unwrap();
        let ids: Vec<i64> = store
            .list_messages(inbox, 0, 10)
            .unwrap()
            .items
            .iter()
            .map(|m| m.id)
            .collect();
        let located = store.locate(&ids).unwrap();
        assert_eq!(located.len(), 3);
        assert_eq!(located[0].uid_validity, Some(5));
        assert_eq!(
            store
                .folder_with_role(acc, FolderRole::Archive)
                .unwrap()
                .as_deref(),
            Some("Archive")
        );

        store
            .apply_op(
                acc,
                &Op::SetFlag {
                    folder: "INBOX".into(),
                    uid_validity: Some(5),
                    uids: vec![3],
                    flag: FlagKind::Seen,
                    value: true,
                },
            )
            .unwrap();
        store
            .apply_op(
                acc,
                &Op::Move {
                    folder: "INBOX".into(),
                    uid_validity: Some(5),
                    uids: vec![1],
                    to: "Archive".into(),
                },
            )
            .unwrap();

        assert_eq!(store.local_uids(inbox).unwrap(), [2, 3]);
        assert_eq!(store.list_folders(acc).unwrap()[0].unread, 1);
        assert_eq!(store.pending_count(acc).unwrap(), 2);
        assert_eq!(
            store.pending_uids(acc, "INBOX").unwrap(),
            HashSet::from([1, 3])
        );

        let queued = store.pending_ops(acc).unwrap();
        assert!(matches!(queued[0].op, Op::SetFlag { .. }));
        store.op_failed(queued[0].id, "offline").unwrap();
        assert_eq!(store.pending_ops(acc).unwrap()[0].attempts, 1);
        store.finish_op(queued[0].id).unwrap();
        store.finish_op(queued[1].id).unwrap();
        assert_eq!(store.pending_count(acc).unwrap(), 0);
    }

    #[test]
    fn server_flags_and_deletions_update_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory(dir.path()).unwrap();
        let acc = store
            .add_account(&new_account("a@example.test"))
            .unwrap()
            .id;
        let (inbox, _) = store
            .sync_folders(acc, &[folder("INBOX", FolderRole::Inbox)])
            .unwrap()[0]
            .clone();
        store
            .insert_headers(inbox, &[header(1, 1, false), header(2, 2, false)])
            .unwrap();
        let flagged = Flags {
            seen: true,
            flagged: true,
            ..Flags::default()
        };
        store.update_flags(inbox, &[(2, flagged)]).unwrap();
        store.remove_uids(acc, inbox, &[1]).unwrap();
        let page = store.list_messages(inbox, 0, 10).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].flags, flagged);
    }

    #[test]
    fn opening_on_disk_creates_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("cache")).unwrap();
        store.add_account(&new_account("a@example.test")).unwrap();
        drop(store);
        let store = Store::open(&dir.path().join("cache")).unwrap();
        assert_eq!(store.list_accounts().unwrap().len(), 1);
    }
}
