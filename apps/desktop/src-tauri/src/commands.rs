//! Commands the UI calls through `invoke`.

use mail_auth::providers::{ProviderPreset, preset_for};
use std::collections::BTreeMap;

use mail_core::{
    Account, AccountId, Endpoint, Error, Event, FlagKind, FolderId, FolderRole, NewAccount, Op,
    Result,
};
use mail_mime::ParsedMessage;
use mail_store::{Folder, MessageLocation, MessagePage};
use mail_sync::ImapSession;
use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::state::{
    AppState, emit, push_now, report_pending, request_sync, secret_key, start_account, stop_account,
};

/// Version of the Rust core.
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Suggested server settings for an email address.
#[tauri::command]
pub fn provider_preset(email: String) -> Option<ProviderPreset> {
    preset_for(&email)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddAccountInput {
    display_name: String,
    email: String,
    username: String,
    password: String,
    imap: Endpoint,
    smtp: Endpoint,
}

/// Checks the login against the IMAP server, saves the account and starts its first sync.
#[tauri::command]
pub async fn add_account(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AddAccountInput,
) -> Result<Account> {
    let email = input.email.trim().to_owned();
    if !email.contains('@') {
        return Err(Error::Auth("enter a full email address".into()));
    }
    let username = match input.username.trim() {
        "" => email.clone(),
        name => name.to_owned(),
    };
    let duplicate = state
        .store
        .lock()
        .unwrap()
        .list_accounts()?
        .into_iter()
        .any(|a| a.email.eq_ignore_ascii_case(&email) && a.imap.host == input.imap.host);
    if duplicate {
        return Err(Error::Auth(format!("{email} is already added")));
    }

    let session = ImapSession::connect(&input.imap, &username, &input.password).await?;
    let _ = session.logout().await;

    let account = state.store.lock().unwrap().add_account(&NewAccount {
        display_name: match input.display_name.trim() {
            "" => email.clone(),
            name => name.to_owned(),
        },
        email,
        username,
        imap: input.imap,
        smtp: input.smtp,
    })?;
    if let Err(err) = state.secrets.set(&secret_key(account.id), &input.password) {
        let _ = state.store.lock().unwrap().remove_account(account.id);
        return Err(err);
    }
    start_account(&app, account.id);
    request_sync(&app, account.id, true);
    Ok(account)
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>> {
    state.store.lock().unwrap().list_accounts()
}

/// Removes an account, its saved password and everything cached for it.
/// Changes still waiting for the server are dropped with it.
#[tauri::command]
pub fn remove_account(app: AppHandle, state: State<'_, AppState>, id: AccountId) -> Result<()> {
    stop_account(&app, id);
    state.secrets.delete(&secret_key(id))?;
    state.store.lock().unwrap().remove_account(id)
}

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>, account: AccountId) -> Result<Vec<Folder>> {
    state.store.lock().unwrap().list_folders(account)
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    folder: FolderId,
    offset: u32,
    limit: u32,
) -> Result<MessagePage> {
    state
        .store
        .lock()
        .unwrap()
        .list_messages(folder, offset, limit.min(500))
}

/// Fetches (or reads from cache) a message, then decodes and sanitizes it, and
/// marks it read.
#[tauri::command]
pub async fn open_message(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    allow_remote: bool,
) -> Result<ParsedMessage> {
    let source = state.store.lock().unwrap().message_source(id)?;
    let account = state.account(source.account_id)?;
    let password = if source.raw_path.is_some() {
        String::new()
    } else {
        state.password(account.id)?
    };
    let raw = mail_sync::fetch_message(&state.store, &account, &password, id).await?;
    let parsed =
        tauri::async_runtime::spawn_blocking(move || mail_mime::parse_message(&raw, allow_remote))
            .await
            .map_err(|e| Error::Protocol(e.to_string()))??;
    let unread: Vec<i64> = state
        .store
        .lock()
        .unwrap()
        .locate(&[id])?
        .into_iter()
        .filter(|m| !m.flags.seen)
        .map(|m| m.id)
        .collect();
    if !unread.is_empty() {
        change(&app, &state, &unread, |m| Op::SetFlag {
            folder: m.folder_name.clone(),
            uid_validity: m.uid_validity,
            uids: Vec::new(),
            flag: FlagKind::Seen,
            value: true,
        })?;
    }
    Ok(parsed)
}

/// Starts a full sync of one account now.
#[tauri::command]
pub fn sync_account(app: AppHandle, account: AccountId) {
    request_sync(&app, account, true);
}

/// Changes still waiting for the server.
#[tauri::command]
pub fn pending_changes(state: State<'_, AppState>, account: AccountId) -> Result<u32> {
    state.store.lock().unwrap().pending_count(account)
}

/// Marks messages read or unread, or flags and unflags them.
#[tauri::command]
pub fn set_flag(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    flag: FlagKind,
    value: bool,
) -> Result<()> {
    change(&app, &state, &ids, |m| Op::SetFlag {
        folder: m.folder_name.clone(),
        uid_validity: m.uid_validity,
        uids: Vec::new(),
        flag,
        value,
    })
}

/// Moves messages to another folder of the same account.
#[tauri::command]
pub fn move_messages(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    folder: FolderId,
) -> Result<()> {
    let target = state
        .store
        .lock()
        .unwrap()
        .folder(folder)?
        .ok_or_else(|| Error::Storage("that folder no longer exists".into()))?;
    let located = state.store.lock().unwrap().locate(&ids)?;
    if located.iter().any(|m| m.account_id != target.account_id) {
        return Err(Error::Storage(
            "messages can only move within their own account".into(),
        ));
    }
    let ids: Vec<i64> = located
        .into_iter()
        .filter(|m| m.folder_id != target.id)
        .map(|m| m.id)
        .collect();
    change(&app, &state, &ids, |m| Op::Move {
        folder: m.folder_name.clone(),
        uid_validity: m.uid_validity,
        uids: Vec::new(),
        to: target.name.clone(),
    })
}

/// Moves messages to their account's Archive folder.
#[tauri::command]
pub fn archive_messages(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> Result<()> {
    to_role(&app, &state, &ids, FolderRole::Archive)
}

/// Moves messages to Trash, or deletes them for good when they are already in
/// Trash or the account has no Trash folder.
#[tauri::command]
pub fn delete_messages(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> Result<()> {
    to_role(&app, &state, &ids, FolderRole::Trash)
}

/// Moves messages to the folder with `role` in their account. For Trash,
/// messages already there (or with no Trash to go to) are deleted for good.
fn to_role(app: &AppHandle, state: &AppState, ids: &[i64], role: FolderRole) -> Result<()> {
    let located = state.store.lock().unwrap().locate(ids)?;
    let mut targets = BTreeMap::new();
    for m in &located {
        if let std::collections::btree_map::Entry::Vacant(e) = targets.entry(m.account_id.0) {
            e.insert(
                state
                    .store
                    .lock()
                    .unwrap()
                    .folder_with_role(m.account_id, role)?,
            );
        }
    }
    if role != FolderRole::Trash && targets.values().any(Option::is_none) {
        return Err(Error::Storage(format!(
            "this account has no {} folder",
            role.as_str().unwrap_or("such")
        )));
    }
    let ids: Vec<i64> = located
        .iter()
        .filter(|m| m.folder_role != role || role == FolderRole::Trash)
        .map(|m| m.id)
        .collect();
    change(app, state, &ids, |m| match &targets[&m.account_id.0] {
        Some(to) if m.folder_role != FolderRole::Trash && *to != m.folder_name => Op::Move {
            folder: m.folder_name.clone(),
            uid_validity: m.uid_validity,
            uids: Vec::new(),
            to: to.clone(),
        },
        _ => Op::Expunge {
            folder: m.folder_name.clone(),
            uid_validity: m.uid_validity,
            uids: Vec::new(),
        },
    })
}

/// Applies a change to the cache, queues it for the server and wakes the
/// account's push task. Messages are grouped into one operation per folder;
/// `make` builds it from the group's first message and the UIDs are filled in.
fn change(
    app: &AppHandle,
    state: &AppState,
    ids: &[i64],
    make: impl Fn(&MessageLocation) -> Op,
) -> Result<()> {
    let located = state.store.lock().unwrap().locate(ids)?;
    let mut groups: BTreeMap<(i64, i64), Vec<&MessageLocation>> = BTreeMap::new();
    for m in &located {
        groups
            .entry((m.account_id.0, m.folder_id.0))
            .or_default()
            .push(m);
    }
    let mut accounts = Vec::new();
    for group in groups.values() {
        let first = group[0];
        let uids: Vec<u32> = group.iter().map(|m| m.uid).collect();
        let op = match make(first) {
            Op::SetFlag {
                folder,
                uid_validity,
                flag,
                value,
                ..
            } => Op::SetFlag {
                folder,
                uid_validity,
                uids,
                flag,
                value,
            },
            Op::Move {
                folder,
                uid_validity,
                to,
                ..
            } => Op::Move {
                folder,
                uid_validity,
                uids,
                to,
            },
            Op::Expunge {
                folder,
                uid_validity,
                ..
            } => Op::Expunge {
                folder,
                uid_validity,
                uids,
            },
        };
        state
            .store
            .lock()
            .unwrap()
            .apply_op(first.account_id, &op)?;
        if !accounts.contains(&first.account_id) {
            accounts.push(first.account_id);
        }
    }
    for account in accounts {
        emit(app, Event::MailboxChanged { account });
        report_pending(app, account, None);
        push_now(app, account);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_the_package() {
        assert_eq!(super::app_version(), "0.1.0");
    }
}
