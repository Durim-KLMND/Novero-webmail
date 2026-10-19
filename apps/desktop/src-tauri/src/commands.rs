//! Commands the UI calls through `invoke`.

use mail_auth::providers::{ProviderPreset, preset_for};
use mail_core::{Account, AccountId, Endpoint, Error, FolderId, NewAccount, Result};
use mail_mime::ParsedMessage;
use mail_store::{Folder, MessagePage};
use mail_sync::ImapSession;
use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::state::{AppState, secret_key, spawn_sync};

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
    spawn_sync(&app, account.id);
    Ok(account)
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>> {
    state.store.lock().unwrap().list_accounts()
}

/// Removes an account, its saved password and everything cached for it.
#[tauri::command]
pub fn remove_account(state: State<'_, AppState>, id: AccountId) -> Result<()> {
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

/// Fetches (or reads from cache) a message, then decodes and sanitizes it.
#[tauri::command]
pub async fn open_message(
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
    state.store.lock().unwrap().mark_seen(id)?;
    Ok(parsed)
}

/// Starts a sync of one account now.
#[tauri::command]
pub fn sync_account(app: AppHandle, account: AccountId) {
    spawn_sync(&app, account);
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_the_package() {
        assert_eq!(super::app_version(), "0.1.0");
    }
}
