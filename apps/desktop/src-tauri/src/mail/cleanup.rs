//! Mail cleanup: the commands behind the cleanup view, which sorts every
//! account's mail into categories by sender, and filing new mail from
//! senders the user put in Spam into Junk.

use std::collections::BTreeMap;

use mail_core::categories::Category;
use mail_core::{AccountId, Error, FolderRole, Result};
use mail_store::{CleanupItem, CleanupReport, CleanupScope, Unchecked};
use mail_sync::{ImapSession, NewMail};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::to_role;
use crate::state::AppState;

/// Progress of reading list headers: `{ done, total }`.
pub const CLEANUP_PROGRESS: &str = "cleanup-progress";

/// One check of list headers at a time; a second waits for the first.
static CHECKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Copy, Serialize)]
struct Progress {
    done: usize,
    total: usize,
}

/// Every sender's mail in a scope, grouped by sender and category.
#[tauri::command]
pub async fn cleanup_report(
    app: AppHandle,
    scope: CleanupScope,
    account: Option<AccountId>,
) -> Result<CleanupReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.cleanup(scope, account)
    })
    .await
    .map_err(|e| Error::Storage(e.to_string()))?
}

/// The messages of a group, each with its own category.
#[tauri::command]
pub fn cleanup_items(state: State<'_, AppState>, ids: Vec<i64>) -> Result<Vec<CleanupItem>> {
    state.store.lock().unwrap().cleanup_items(&ids)
}

/// Puts everything from a sender in a category, or back to guessing.
#[tauri::command]
pub fn set_sender_category(
    state: State<'_, AppState>,
    email: String,
    category: Option<Category>,
) -> Result<()> {
    state
        .store
        .lock()
        .unwrap()
        .set_sender_category(&email, category)
}

/// Puts messages in a category, or back to their sender's or guessed one.
#[tauri::command]
pub fn set_message_category(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    category: Option<Category>,
) -> Result<()> {
    state
        .store
        .lock()
        .unwrap()
        .set_message_category(&ids, category)
}

/// Moves messages to their account's Junk folder.
#[tauri::command]
pub fn junk_messages(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> Result<()> {
    to_role(&app, &state, &ids, FolderRole::Junk)
}

/// Reads the list headers of mail cached before they were fetched: from the
/// message file when it is on disk, else from the server. Reports progress
/// as `cleanup-progress` events and returns how many messages are still
/// unchecked (those of accounts that couldn't be reached).
#[tauri::command]
pub async fn check_list_headers(
    app: AppHandle,
    scope: CleanupScope,
    account: Option<AccountId>,
) -> Result<usize> {
    let _one_at_a_time = CHECKING.lock().await;
    let state = app.state::<AppState>();
    let waiting = state.store.lock().unwrap().unchecked(scope, account)?;
    let total = waiting.len();
    if total == 0 {
        return Ok(0);
    }
    let mut done = 0;
    let report = |done: usize| {
        let _ = app.emit(CLEANUP_PROGRESS, Progress { done, total });
    };
    report(0);

    // Messages already on disk need no connection.
    let (cached, remote): (Vec<Unchecked>, Vec<Unchecked>) =
        waiting.into_iter().partition(|m| m.raw_path.is_some());
    for chunk in cached.chunks(200) {
        let chunk = chunk.to_vec();
        let found = tauri::async_runtime::spawn_blocking(move || {
            chunk
                .into_iter()
                .filter_map(|m| {
                    let raw = std::fs::read(m.raw_path?).ok()?;
                    let h = mail_mime::parse_header_fields(&raw);
                    Some((m.id, h.signals, h.unsubscribe))
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| Error::Storage(e.to_string()))?;
        state.store.lock().unwrap().set_signals(&found)?;
        done += found.len();
        report(done);
    }

    // The rest from each account's server, one folder after another.
    let mut by_account: BTreeMap<i64, BTreeMap<String, Vec<(i64, u32)>>> = BTreeMap::new();
    for m in remote {
        by_account
            .entry(m.account_id.0)
            .or_default()
            .entry(m.folder_name)
            .or_default()
            .push((m.id, m.uid));
    }
    for (id, folders) in by_account {
        let checked = async {
            let account = state.account(AccountId(id))?;
            let secret = state.secret(&account).await?;
            let mut session =
                ImapSession::connect(&account.imap, &account.username, &secret).await?;
            for (name, wanted) in &folders {
                let counted = std::sync::atomic::AtomicUsize::new(done);
                let result =
                    mail_sync::read_signals(&state.store, &mut session, name, wanted, |n| {
                        let now = counted.fetch_add(n, std::sync::atomic::Ordering::Relaxed) + n;
                        report(now);
                    })
                    .await;
                done = counted.into_inner();
                // A folder that went away since the last sync is skipped.
                if let Err(Error::Network(e)) = result {
                    return Err(Error::Network(e));
                }
            }
            let _ = session.logout().await;
            Ok::<(), Error>(())
        }
        .await;
        if checked.is_err() {
            // Unreachable for now: its mail stays unchecked and is sorted
            // from sender and subject alone.
            continue;
        }
    }
    let left = state.store.lock().unwrap().unchecked(scope, account)?.len();
    report(total);
    Ok(left)
}

/// Moves new inbox mail from senders the user put in Spam to Junk, and
/// returns the mail still to announce.
pub fn apply(app: &AppHandle, account: AccountId, new_mail: &[NewMail]) -> Vec<NewMail> {
    if new_mail.is_empty() {
        return Vec::new();
    }
    let state = app.state::<AppState>();
    let spam: Vec<i64> = {
        let store = state.store.lock().unwrap();
        let has_junk = store
            .folder_with_role(account, FolderRole::Junk)
            .ok()
            .flatten()
            .is_some();
        if !has_junk {
            return new_mail.to_vec();
        }
        new_mail
            .iter()
            .filter(|m| {
                let (email, _) = mail_store::sender_of(m.from.as_deref());
                !email.is_empty()
                    && store.sender_category(&email).ok().flatten() == Some(Category::Spam)
            })
            .map(|m| m.id)
            .collect()
    };
    if spam.is_empty() || to_role(app, &state, &spam, FolderRole::Junk).is_err() {
        return new_mail.to_vec();
    }
    new_mail
        .iter()
        .filter(|m| !spam.contains(&m.id))
        .cloned()
        .collect()
}
