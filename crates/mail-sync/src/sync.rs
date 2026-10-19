//! Bringing the local cache and the server in step, in both directions.
//!
//! Local changes (flags, moves, deletes) are applied to the cache at once and
//! queued; [`push_changes`] sends them to the server. [`sync_account`] pulls in
//! what changed on the server: new and vanished messages, and flag changes
//! (incrementally with CONDSTORE, or for the newest messages without it).
//!
//! Sync never touches messages that queued changes refer to, so it cannot undo
//! a change the server has not seen yet. Only one task per account should push
//! at a time, or a change could be sent twice.

use std::collections::HashSet;
use std::sync::Mutex;

use mail_core::{Account, AccountId, Error, Event, FolderId, FolderRole, Op, Result, Secret};
use mail_store::Store;

use crate::ImapSession;
use crate::drafts::upload_drafts;
use crate::imap::uid_set;

/// Headers fetched per round trip.
const BATCH: usize = 500;

/// Without CONDSTORE, flags are refreshed for this many of the newest messages.
const FLAG_WINDOW: usize = 2000;

/// Which folders a sync covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope<'a> {
    All,
    Folder(&'a str),
}

/// What happened to the queued changes during a replay.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReplayOutcome {
    /// Changes the server applied.
    pub applied: u32,
    /// Changes the server refused, with its reason. They are dropped and the
    /// affected folders resync, which undoes them locally.
    pub rejected: Vec<String>,
    /// Folders whose contents changed on the server because of the replay or
    /// a draft upload.
    pub touched: Vec<String>,
}

/// Syncs the folders in `scope`, inbox first.
pub async fn sync_account(
    store: &Mutex<Store>,
    account: &Account,
    secret: &Secret,
    scope: Scope<'_>,
    on_event: impl Fn(Event),
) -> Result<()> {
    let mut session = ImapSession::connect(&account.imap, &account.username, secret).await?;
    let remote = session.list_folders().await?;
    let mut folders = store.lock().unwrap().sync_folders(account.id, &remote)?;
    folders.retain(|(_, f)| f.selectable);
    if let Scope::Folder(name) = scope {
        folders.retain(|(_, f)| f.name == name);
    }
    folders.sort_by_key(|(_, f)| (f.role != FolderRole::Inbox, f.role.sort_rank()));

    for (folder_id, folder) in folders {
        let result = sync_folder(
            store,
            &mut session,
            account.id,
            folder_id,
            &folder.name,
            &on_event,
        )
        .await;
        match result {
            // Deleted on the server since LIST; the next sync drops it.
            Err(Error::Rejected(_)) => continue,
            other => other?,
        }
    }
    session.logout().await
}

/// Sends queued changes and saved drafts, then syncs the folders they
/// touched: a move's target, the Drafts folder, and the folder of a refused
/// change so the cache shows the server's state.
pub async fn push_changes(
    store: &Mutex<Store>,
    account: &Account,
    secret: &Secret,
    on_event: impl Fn(Event),
) -> Result<ReplayOutcome> {
    let idle = {
        let store = store.lock().unwrap();
        store.pending_count(account.id)? == 0 && store.drafts_to_upload(account.id)?.is_empty()
    };
    if idle {
        return Ok(ReplayOutcome::default());
    }
    let mut session = ImapSession::connect(&account.imap, &account.username, secret).await?;
    let mut outcome = replay_ops(store, &mut session, account.id).await?;
    if let Some(drafts) = upload_drafts(store, &mut session, account).await? {
        touch(&mut outcome.touched, &drafts);
    }
    for name in &outcome.touched {
        let id = store.lock().unwrap().folder_id(account.id, name)?;
        if let Some(id) = id {
            match sync_folder(store, &mut session, account.id, id, name, &on_event).await {
                Err(Error::Rejected(_)) => {}
                other => other?,
            }
        }
    }
    session.logout().await?;
    Ok(outcome)
}

/// Sends every queued change for the account, oldest first.
///
/// A lost connection stops the replay and leaves the rest queued, so it
/// resumes on the next attempt. A change the server refuses is dropped, and
/// its folder is listed in `touched` so a sync restores the server's state.
pub async fn replay_ops(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    account: AccountId,
) -> Result<ReplayOutcome> {
    let queued = store.lock().unwrap().pending_ops(account)?;
    let mut outcome = ReplayOutcome::default();
    for item in queued {
        match apply(session, &item.op).await {
            Ok(applied) => {
                store.lock().unwrap().finish_op(item.id)?;
                if applied {
                    outcome.applied += 1;
                    if let Op::Move { to, .. } = &item.op {
                        touch(&mut outcome.touched, to);
                    }
                }
            }
            Err(Error::Rejected(reason)) => {
                store.lock().unwrap().finish_op(item.id)?;
                outcome.rejected.push(reason);
                touch(&mut outcome.touched, item.op.folder());
                if let Op::SetFlag { folder, uids, .. } = &item.op {
                    restore_flags(store, session, account, folder, uids).await?;
                }
            }
            Err(err) => {
                store.lock().unwrap().op_failed(item.id, &err.to_string())?;
                return Err(err);
            }
        }
    }
    Ok(outcome)
}

/// Runs one change on the server. Returns false when it no longer applies
/// because the folder was rebuilt (UIDVALIDITY changed) since it was queued.
async fn apply(session: &mut ImapSession, op: &Op) -> Result<bool> {
    let status = session.select(op.folder()).await?;
    if op.uid_validity().is_some() && op.uid_validity() != status.uid_validity {
        return Ok(false);
    }
    let set = uid_set(op.uids());
    if set.is_empty() {
        return Ok(true);
    }
    match op {
        Op::SetFlag { flag, value, .. } => session.set_flag(&set, *flag, *value).await?,
        Op::Move { to, .. } => session.move_uids(&set, to).await?,
        Op::Expunge { .. } => session.expunge_uids(&set).await?,
    }
    Ok(true)
}

/// Puts the server's flags back on messages after a refused flag change.
async fn restore_flags(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    account: AccountId,
    folder: &str,
    uids: &[u32],
) -> Result<()> {
    let Some(folder_id) = store.lock().unwrap().folder_id(account, folder)? else {
        return Ok(());
    };
    if session.select(folder).await.is_err() {
        return Ok(());
    }
    let flags = session.fetch_flags(&uid_set(uids), None).await?;
    store.lock().unwrap().update_flags(folder_id, &flags)
}

fn touch(list: &mut Vec<String>, folder: &str) {
    if !list.iter().any(|f| f == folder) {
        list.push(folder.to_owned());
    }
}

/// Brings one folder's cache in step with the server.
pub async fn sync_folder(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    account: AccountId,
    folder_id: FolderId,
    name: &str,
    on_event: &impl Fn(Event),
) -> Result<()> {
    let status = session.select(name).await?;
    let mut state = store.lock().unwrap().folder_state(folder_id)?;
    if state.uid_validity.is_some() && state.uid_validity != status.uid_validity {
        store.lock().unwrap().reset_folder(account, folder_id)?;
        state = store.lock().unwrap().folder_state(folder_id)?;
    }
    let pending = store.lock().unwrap().pending_uids(account, name)?;

    // Flag changes on messages we already have.
    if state.count > 0 && status.exists > 0 {
        let changed = match (state.highest_modseq, status.highest_modseq) {
            (Some(old), Some(new)) if old == new => Vec::new(),
            (Some(old), Some(_)) => session.fetch_flags("1:*", Some(old)).await?,
            _ => {
                let local = store.lock().unwrap().local_uids(folder_id)?;
                let from = local[local.len().saturating_sub(FLAG_WINDOW)];
                session.fetch_flags(&format!("{from}:*"), None).await?
            }
        };
        let changed: Vec<_> = changed
            .into_iter()
            .filter(|(uid, _)| !pending.contains(uid))
            .collect();
        if !changed.is_empty() {
            store.lock().unwrap().update_flags(folder_id, &changed)?;
        }
    }

    // Messages added or removed. When the count and UIDNEXT both match what we
    // stored, nothing was added or expunged, so the UID list can be skipped.
    let unchanged = state.uid_next.is_some()
        && state.uid_next == status.uid_next
        && state.count as usize + pending_removals(store, account, name)? == status.exists as usize;
    if !unchanged {
        let server: HashSet<u32> = if status.exists == 0 {
            HashSet::new()
        } else {
            session.search_uids("ALL").await?.into_iter().collect()
        };
        let local = store.lock().unwrap().local_uids(folder_id)?;
        let local_set: HashSet<u32> = local.iter().copied().collect();

        let vanished: Vec<u32> = local
            .iter()
            .copied()
            .filter(|uid| !server.contains(uid))
            .collect();
        store
            .lock()
            .unwrap()
            .remove_uids(account, folder_id, &vanished)?;

        let mut missing: Vec<u32> = server
            .into_iter()
            .filter(|uid| !local_set.contains(uid) && !pending.contains(uid))
            .collect();
        missing.sort_unstable_by(|a, b| b.cmp(a));
        fetch_headers(store, session, account, folder_id, name, &missing, on_event).await?;
    }

    store.lock().unwrap().set_folder_synced(
        folder_id,
        status.uid_validity,
        status.uid_next,
        status.highest_modseq,
    )
}

/// How many messages queued moves and deletes have already taken out of the
/// cache for `folder`, while the server still has them.
fn pending_removals(store: &Mutex<Store>, account: AccountId, folder: &str) -> Result<usize> {
    Ok(store
        .lock()
        .unwrap()
        .pending_ops(account)?
        .iter()
        .filter(|q| q.op.folder() == folder && !matches!(q.op, Op::SetFlag { .. }))
        .map(|q| q.op.uids().len())
        .sum())
}

/// Fetches headers newest first, in batches, reporting progress.
async fn fetch_headers(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    account: AccountId,
    folder_id: FolderId,
    name: &str,
    uids: &[u32],
    on_event: &impl Fn(Event),
) -> Result<()> {
    let total = uids.len() as u32;
    let mut done = 0;
    on_event(Event::SyncProgress {
        account,
        folder: name.to_owned(),
        done,
        total,
    });
    for chunk in uids.chunks(BATCH) {
        let headers = session.fetch_headers(&uid_set(chunk)).await?;
        store.lock().unwrap().insert_headers(folder_id, &headers)?;
        done += chunk.len() as u32;
        on_event(Event::SyncProgress {
            account,
            folder: name.to_owned(),
            done,
            total,
        });
    }
    Ok(())
}

/// Fetches a message body the cache does not have yet and saves it.
pub async fn fetch_message(
    store: &Mutex<Store>,
    account: &Account,
    secret: &Secret,
    message: i64,
) -> Result<Vec<u8>> {
    let source = store.lock().unwrap().message_source(message)?;
    if let Some(path) = &source.raw_path
        && let Ok(raw) = std::fs::read(path)
    {
        return Ok(raw);
    }
    let mut session = ImapSession::connect(&account.imap, &account.username, secret).await?;
    let raw = session.fetch_raw(&source.folder_name, source.uid).await?;
    store.lock().unwrap().save_raw(message, &raw)?;
    let _ = session.logout().await;
    Ok(raw)
}
