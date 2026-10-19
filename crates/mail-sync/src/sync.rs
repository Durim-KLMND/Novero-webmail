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
use std::sync::atomic::{AtomicU32, Ordering};

use mail_core::{
    Account, AccountId, Error, Event, FolderId, FolderRole, Op, Result, Secret, SyncStage,
};
use mail_store::Store;

use crate::ImapSession;
use crate::drafts::upload_drafts;
use crate::imap::uid_set;

/// Headers fetched per round trip.
const BATCH: usize = 500;

/// Without CONDSTORE, flags are refreshed for this many of the newest messages.
const FLAG_WINDOW: usize = 2000;

/// Bodies of messages newer than this are downloaded during sync, so they
/// read offline and their text is searchable.
const PREFETCH_DAYS: i64 = 30;

/// Larger messages (big attachments) are left until opened.
const PREFETCH_MAX_SIZE: u32 = 2 * 1024 * 1024;

/// Bodies downloaded per folder in one sync, so a first sync stays short;
/// the next syncs continue where it stopped.
const PREFETCH_PER_SYNC: u32 = 300;

/// Bodies fetched per round trip.
const PREFETCH_BATCH: usize = 25;

/// A message that arrived in an inbox since the last sync, unread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMail {
    pub id: i64,
    pub from: Option<String>,
    pub subject: Option<String>,
}

/// What a sync found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// New unread mail in inboxes that had synced before. A first sync
    /// reports none, so adding an account does not announce old mail.
    pub new_mail: Vec<NewMail>,
}

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

/// Syncs the folders in `scope`, inbox first, then downloads the bodies of
/// recent messages in them.
pub async fn sync_account(
    store: &Mutex<Store>,
    account: &Account,
    secret: &Secret,
    scope: Scope<'_>,
    on_event: impl Fn(Event),
) -> Result<SyncReport> {
    let mut session = ImapSession::connect(&account.imap, &account.username, secret).await?;
    let remote = session.list_folders().await?;
    let mut folders = store.lock().unwrap().sync_folders(account.id, &remote)?;
    folders.retain(|(_, f)| f.selectable);
    if let Scope::Folder(name) = scope {
        folders.retain(|(_, f)| f.name == name);
    }
    folders.sort_by_key(|(_, f)| (f.role != FolderRole::Inbox, f.role.sort_rank()));

    // Messages on the server the cache lacks, so the whole download can be
    // shown as one progress bar. An estimate: STATUS counts every message.
    let mut missing = 0;
    for (folder_id, folder) in &folders {
        let cached = store.lock().unwrap().folder_state(*folder_id)?.count;
        if let Ok(count) = session.message_count(&folder.name).await {
            missing += count.saturating_sub(cached);
        }
    }
    let progress = Progress::new(account.id, SyncStage::Messages, missing, &on_event);
    // Per-folder events also move the account's bar.
    let folder_done = AtomicU32::new(0);
    let forward = |event: Event| {
        if let Event::SyncProgress { done, .. } = event {
            progress.set(folder_done.load(Ordering::Relaxed) + done);
        }
        on_event(event);
    };

    let mut report = SyncReport::default();
    let mut synced = Vec::new();
    for (folder_id, folder) in folders {
        let before = progress.done();
        let result = sync_folder(
            store,
            &mut session,
            account.id,
            folder_id,
            &folder.name,
            &forward,
        )
        .await;
        folder_done.store(progress.done().max(before), Ordering::Relaxed);
        match result {
            // Deleted on the server since LIST; the next sync drops it.
            Err(Error::Rejected(_)) => continue,
            Ok(new_mail) => report.new_mail.extend(new_mail),
            Err(e) => return Err(e),
        }
        synced.push((folder_id, folder));
    }
    progress.finish();

    // Messages show up first; their bodies follow.
    let since = now() - PREFETCH_DAYS * 24 * 60 * 60;
    let mut wanted = Vec::new();
    for (folder_id, folder) in synced {
        if matches!(folder.role, FolderRole::Trash | FolderRole::Junk) {
            continue;
        }
        let bodies = store.lock().unwrap().bodies_to_prefetch(
            folder_id,
            since,
            PREFETCH_MAX_SIZE,
            PREFETCH_PER_SYNC,
        )?;
        if !bodies.is_empty() {
            wanted.push((folder.name, bodies));
        }
    }
    let total = wanted.iter().map(|(_, b)| b.len() as u32).sum();
    let progress = Progress::new(account.id, SyncStage::Bodies, total, &on_event);
    for (name, bodies) in &wanted {
        match prefetch_bodies(store, &mut session, name, bodies, &progress).await {
            Err(Error::Rejected(_)) => continue,
            Ok(()) => {}
            Err(e) => return Err(e),
        }
    }
    progress.finish();
    if total > 0 {
        // Attachments are known for sure once bodies are in.
        on_event(Event::MailboxChanged {
            account: account.id,
        });
    }
    session.logout().await?;
    Ok(report)
}

/// Downloads the bodies of `wanted` messages in `name` and indexes their
/// text, advancing `progress` as batches arrive.
async fn prefetch_bodies(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    name: &str,
    wanted: &[(i64, u32)],
    progress: &Progress<'_, impl Fn(Event)>,
) -> Result<()> {
    session.select(name).await?;
    for chunk in wanted.chunks(PREFETCH_BATCH) {
        let uids: Vec<u32> = chunk.iter().map(|(_, uid)| *uid).collect();
        let bodies = session.fetch_raw_many(&uid_set(&uids)).await?;
        for (id, uid) in chunk {
            // A message expunged meanwhile is simply missing.
            if let Some((_, raw)) = bodies.iter().find(|(u, _)| u == uid) {
                save_and_index(store, *id, raw)?;
            }
        }
        progress.add(chunk.len() as u32);
    }
    Ok(())
}

/// Reports one stage of an account sync as `AccountProgress` events, if the
/// stage has any work.
struct Progress<'a, F: Fn(Event)> {
    account: AccountId,
    stage: SyncStage,
    total: u32,
    done: AtomicU32,
    on_event: &'a F,
}

impl<'a, F: Fn(Event)> Progress<'a, F> {
    fn new(account: AccountId, stage: SyncStage, total: u32, on_event: &'a F) -> Self {
        let progress = Progress {
            account,
            stage,
            total,
            done: AtomicU32::new(0),
            on_event,
        };
        progress.emit();
        progress
    }

    fn done(&self) -> u32 {
        self.done.load(Ordering::Relaxed)
    }

    /// Sets the items done so far. The total is an estimate, so it grows if
    /// more turn up.
    fn set(&self, done: u32) {
        if done != self.done.load(Ordering::Relaxed) {
            self.done.store(done, Ordering::Relaxed);
            self.emit();
        }
    }

    fn add(&self, n: u32) {
        self.set(self.done.load(Ordering::Relaxed) + n);
    }

    /// Marks the stage complete, even if the estimate was high.
    fn finish(&self) {
        if self.total > 0 && self.done.load(Ordering::Relaxed) < self.total {
            self.done.store(self.total, Ordering::Relaxed);
            self.emit();
        }
    }

    fn emit(&self) {
        if self.total == 0 {
            return;
        }
        (self.on_event)(Event::AccountProgress {
            account: self.account,
            stage: self.stage,
            done: self.done.load(Ordering::Relaxed).min(self.total),
            total: self.total,
        });
    }
}

/// Caches a downloaded message and adds its text to the search index.
fn save_and_index(store: &Mutex<Store>, id: i64, raw: &[u8]) -> Result<()> {
    let text = mail_mime::search_text(raw);
    let store = store.lock().unwrap();
    store.save_raw(id, raw)?;
    if let Some(text) = text {
        store.index_body(id, &text)?;
    }
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
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
                other => {
                    other?;
                }
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

/// Brings one folder's cache in step with the server. Returns the new unread
/// messages when the folder is an inbox that had synced before.
pub async fn sync_folder(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    account: AccountId,
    folder_id: FolderId,
    name: &str,
    on_event: &impl Fn(Event),
) -> Result<Vec<NewMail>> {
    let status = session.select(name).await?;
    let mut state = store.lock().unwrap().folder_state(folder_id)?;
    let is_inbox = store.lock().unwrap().folder_role(folder_id)? == Some(FolderRole::Inbox);
    // Messages at or above the old UIDNEXT arrived since the last sync.
    let announce_from = state.uid_next.filter(|_| is_inbox);
    if state.uid_validity.is_some() && state.uid_validity != status.uid_validity {
        store.lock().unwrap().reset_folder(account, folder_id)?;
        state = store.lock().unwrap().folder_state(folder_id)?;
    }
    let pending = store.lock().unwrap().pending_uids(account, name)?;
    let mut new_mail = Vec::new();

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
        new_mail =
            fetch_headers(store, session, account, folder_id, name, &missing, on_event).await?;
        match announce_from {
            Some(from) => new_mail.retain(|(uid, _)| *uid >= from),
            None => new_mail.clear(),
        }
    }

    store.lock().unwrap().set_folder_synced(
        folder_id,
        status.uid_validity,
        status.uid_next,
        status.highest_modseq,
    )?;
    Ok(new_mail.into_iter().map(|(_, mail)| mail).collect())
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

/// Fetches headers newest first, in batches, reporting progress. Returns the
/// unread messages that were new to the cache, with their UIDs.
async fn fetch_headers(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    account: AccountId,
    folder_id: FolderId,
    name: &str,
    uids: &[u32],
    on_event: &impl Fn(Event),
) -> Result<Vec<(u32, NewMail)>> {
    let mut unread = Vec::new();
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
        let ids = store.lock().unwrap().insert_headers(folder_id, &headers)?;
        for (h, id) in headers.into_iter().zip(ids) {
            if let Some(id) = id
                && !h.flags.seen
            {
                unread.push((
                    h.uid,
                    NewMail {
                        id,
                        from: h.from,
                        subject: h.subject,
                    },
                ));
            }
        }
        done += chunk.len() as u32;
        on_event(Event::SyncProgress {
            account,
            folder: name.to_owned(),
            done,
            total,
        });
    }
    Ok(unread)
}

/// Fetches a message body the cache does not have yet, saves it and adds
/// its text to the search index.
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
    save_and_index(store, message, &raw)?;
    let _ = session.logout().await;
    Ok(raw)
}
