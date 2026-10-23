//! Commands the UI calls through `invoke`.

use std::collections::BTreeMap;

use mail_auth::discover::{Discovered, Discoverer};
use mail_auth::oauth::{self, OAuthClient, Tokens};
use mail_core::{
    Account, AccountId, AuthMethod, Endpoint, Error, Event, FlagKind, FolderId, FolderRole,
    NewAccount, Op, Result, Secret,
};
use mail_mime::ParsedMessage;
use mail_store::{Folder, Mailbox, MessageLocation, MessagePage, MessageSummary};
use mail_sync::ImapSession;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::settings::Settings;
use crate::state::{
    AppState, emit, oauth_client, push_now, report_pending, request_sync, secret_key,
    start_account, stop_account,
};

/// Version of the Rust core.
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Server settings found for an email address.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupInfo {
    #[serde(flatten)]
    settings: Discovered,
    /// Whether this build can open the provider's sign-in page.
    oauth_available: bool,
}

/// Looks up server settings for an email address (see `mail_auth::discover`).
#[tauri::command]
pub async fn discover_settings(email: String) -> Option<SetupInfo> {
    let settings = Discoverer::new().discover(&email).await?;
    let oauth_available = settings
        .oauth
        .is_some_and(|p| OAuthClient::for_provider(p).is_some());
    Some(SetupInfo {
        settings,
        oauth_available,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddAccountInput {
    display_name: String,
    email: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    imap: Endpoint,
    smtp: Endpoint,
    #[serde(default)]
    auth: AuthMethod,
}

/// Signs in (with the password, or through the provider's page in the
/// browser), checks the login against the IMAP server, saves the account and
/// starts its first sync.
#[tauri::command]
pub async fn add_account(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AddAccountInput,
) -> Result<Account> {
    let mut email = input.email.trim().to_owned();
    if !email.contains('@') {
        return Err(Error::Invalid("enter a full email address".into()));
    }
    crate::plan::can_add_account(&state.store.lock().unwrap())?;
    let mut username = match input.username.trim() {
        "" => email.clone(),
        name => name.to_owned(),
    };
    let mut tokens = None;
    let secret = match input.auth {
        AuthMethod::Password if input.password.is_empty() => {
            return Err(Error::Invalid("enter your password".into()));
        }
        AuthMethod::Password => Secret::Password(input.password),
        AuthMethod::OAuth2 { provider } => {
            let signed_in = browser_sign_in(&app, &state, oauth_client(provider)?, &email).await?;
            // The account is whichever one was chosen on the sign-in page.
            if let Some(address) = &signed_in.email {
                email = address.clone();
            }
            username = email.clone();
            let secret = Secret::AccessToken(signed_in.access_token.clone());
            tokens = Some(signed_in);
            secret
        }
    };
    let duplicate = state
        .store
        .lock()
        .unwrap()
        .list_accounts()?
        .into_iter()
        .any(|a| a.email.eq_ignore_ascii_case(&email) && a.imap.host == input.imap.host);
    if duplicate {
        return Err(Error::Invalid(format!("{email} is already added")));
    }

    let session = ImapSession::connect(&input.imap, &username, &secret).await?;
    let _ = session.logout().await;
    // Sending problems (a wrong SMTP server, SMTP turned off for the mailbox)
    // are found now rather than at the first message.
    mail_send::check_login(&input.smtp, &username, &secret)
        .await
        .map_err(sending_failed)?;

    let account = state.store.lock().unwrap().add_account(&NewAccount {
        display_name: match input.display_name.trim() {
            "" => email.clone(),
            name => name.to_owned(),
        },
        email,
        username,
        imap: input.imap,
        smtp: input.smtp,
        auth: input.auth,
    })?;
    let saved = match (&secret, &tokens) {
        (_, Some(tokens)) => state.keep_tokens(account.id, tokens),
        (Secret::Password(password), None) => state.secrets.set(&secret_key(account.id), password),
        (Secret::AccessToken(_), None) => unreachable!("tokens come with an access token"),
    };
    if let Err(err) = saved {
        let _ = state.store.lock().unwrap().remove_account(account.id);
        return Err(err);
    }
    start_account(&app, account.id);
    request_sync(&app, account.id, true);
    Ok(account)
}

fn sending_failed(err: Error) -> Error {
    match err {
        Error::Auth(why) => Error::Auth(format!(
            "receiving works, but the sending server refused the sign-in: {why}"
        )),
        Error::Network(why) => Error::Network(format!(
            "receiving works, but the sending server could not be reached: {why}"
        )),
        other => other,
    }
}

/// Signs an account in again: with a new password, or through the provider's
/// page when its sign-in expired or was revoked. Then syncs it.
#[tauri::command]
pub async fn sign_in_again(
    app: AppHandle,
    state: State<'_, AppState>,
    id: AccountId,
    password: Option<String>,
) -> Result<Account> {
    let account = state.account(id)?;
    let (secret, tokens) = match account.auth {
        AuthMethod::Password => match password.filter(|p| !p.is_empty()) {
            Some(password) => (Secret::Password(password), None),
            None => return Err(Error::Invalid("enter your password".into())),
        },
        AuthMethod::OAuth2 { provider } => {
            let tokens =
                browser_sign_in(&app, &state, oauth_client(provider)?, &account.email).await?;
            if let Some(other) = tokens
                .email
                .as_ref()
                .filter(|e| !e.eq_ignore_ascii_case(&account.email))
            {
                return Err(Error::Auth(format!(
                    "you signed in as {other}; choose {} on the sign-in page",
                    account.email
                )));
            }
            (
                Secret::AccessToken(tokens.access_token.clone()),
                Some(tokens),
            )
        }
    };
    let session = ImapSession::connect(&account.imap, &account.username, &secret).await?;
    let _ = session.logout().await;
    match (&secret, &tokens) {
        (_, Some(tokens)) => state.keep_tokens(id, tokens)?,
        (Secret::Password(password), None) => state.secrets.set(&secret_key(id), password)?,
        (Secret::AccessToken(_), None) => unreachable!("tokens come with an access token"),
    }
    // Work that stopped on the old sign-in starts over.
    stop_account(&app, id);
    start_account(&app, id);
    request_sync(&app, id, true);
    Ok(account)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettingsInput {
    imap: Endpoint,
    smtp: Endpoint,
    /// Password accounts only; empty for the email address.
    #[serde(default)]
    username: String,
    /// A new password, when the server change needs one.
    #[serde(default)]
    password: String,
}

/// Changes an account's incoming and outgoing servers. Both are tried with
/// the account's sign-in first, so a typo can't leave it unable to sync.
#[tauri::command]
pub async fn update_account_servers(
    app: AppHandle,
    state: State<'_, AppState>,
    id: AccountId,
    input: ServerSettingsInput,
) -> Result<Account> {
    let account = state.account(id)?;
    let trimmed = |e: Endpoint| Endpoint {
        host: e.host.trim().to_owned(),
        ..e
    };
    let (imap, smtp) = (trimmed(input.imap), trimmed(input.smtp));
    if imap.host.is_empty() || smtp.host.is_empty() {
        return Err(Error::Invalid("enter both server addresses".into()));
    }
    if imap.port == 0 || smtp.port == 0 {
        return Err(Error::Invalid("enter a port between 1 and 65535".into()));
    }
    let (username, password) = match account.auth {
        // The provider's sign-in always uses the address it signed in as.
        AuthMethod::OAuth2 { .. } => (account.username.clone(), None),
        AuthMethod::Password => (
            match input.username.trim() {
                "" => account.email.clone(),
                name => name.to_owned(),
            },
            Some(input.password).filter(|p| !p.is_empty()),
        ),
    };
    let secret = match &password {
        Some(password) => Secret::Password(password.clone()),
        None => state.secret(&account).await?,
    };

    let session = ImapSession::connect(&imap, &username, &secret).await?;
    let _ = session.logout().await;
    mail_send::check_login(&smtp, &username, &secret)
        .await
        .map_err(sending_failed)?;

    let updated = state
        .store
        .lock()
        .unwrap()
        .update_servers(id, &username, &imap, &smtp)?;
    if let Some(password) = &password {
        state.secrets.set(&secret_key(id), password)?;
    }
    // Connections to the old servers close; everything reconnects to the new ones.
    stop_account(&app, id);
    start_account(&app, id);
    request_sync(&app, id, true);
    Ok(updated)
}

/// Stops waiting for the browser sign-in.
#[tauri::command]
pub fn cancel_sign_in(state: State<'_, AppState>) {
    if let Some(cancel) = state.sign_in.lock().unwrap().take() {
        let _ = cancel.send(());
    }
}

/// Opens the provider's sign-in page in the browser and waits for it, unless
/// the person cancels.
pub(crate) async fn browser_sign_in(
    app: &AppHandle,
    state: &AppState,
    client: OAuthClient,
    login_hint: &str,
) -> Result<Tokens> {
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    if let Some(earlier) = state.sign_in.lock().unwrap().replace(cancel) {
        // Only one sign-in page at a time; the newest one wins.
        let _ = earlier.send(());
    }
    let opener = app.clone();
    let flow = oauth::sign_in(&client, login_hint, move |url| {
        opener
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| Error::Auth(format!("could not open the browser: {e}")))
    });
    let result = tokio::select! {
        result = flow => result,
        _ = cancelled => Err(Error::Auth("sign-in was cancelled".into())),
    };
    if let Some(window) = app.get_webview_window("main") {
        // The browser had the focus; bring the app back.
        let _ = window.set_focus();
    }
    result
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>> {
    state.store.lock().unwrap().list_accounts()
}

/// Removes an account, its saved password or sign-in and everything cached for it.
/// Changes still waiting for the server are dropped with it.
#[tauri::command]
pub fn remove_account(app: AppHandle, state: State<'_, AppState>, id: AccountId) -> Result<()> {
    stop_account(&app, id);
    state.forget_tokens(id);
    state.secrets.delete(&secret_key(id))?;
    state.store.lock().unwrap().remove_account(id)
}

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>, account: AccountId) -> Result<Vec<Folder>> {
    state.store.lock().unwrap().list_folders(account)
}

/// A page of a folder, of every inbox, or of search results. Folders list
/// conversations when that setting is on.
#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    mailbox: Mailbox,
    offset: u32,
    limit: u32,
) -> Result<MessagePage> {
    let store = state.store.lock().unwrap();
    let threaded = Settings::load(&store).conversations;
    store.list(&mailbox, threaded, offset, limit.min(500))
}

/// The messages of a conversation, oldest first (see `Store::conversation`).
/// With conversations turned off, just the message itself.
#[tauri::command]
pub fn conversation(state: State<'_, AppState>, message: i64) -> Result<Vec<MessageSummary>> {
    let store = state.store.lock().unwrap();
    if Settings::load(&store).conversations {
        store.conversation(message)
    } else {
        Ok(store
            .conversation(message)?
            .into_iter()
            .filter(|m| m.id == message)
            .collect())
    }
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
    let (_, raw) = raw_message(&app, id).await?;
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

/// A message's account and its raw bytes, from the cache or the server.
pub(crate) async fn raw_message(app: &AppHandle, id: i64) -> Result<(Account, Vec<u8>)> {
    let state = app.state::<AppState>();
    let source = state.store.lock().unwrap().message_source(id)?;
    let account = state.account(source.account_id)?;
    let raw = crate::mail::reader::raw(app, &account, id).await?;
    Ok((account, raw))
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
    move_to(&app, &state, &ids, folder)
}

/// Moves messages to `folder`, which must be in their own account.
pub(crate) fn move_to(
    app: &AppHandle,
    state: &AppState,
    ids: &[i64],
    folder: FolderId,
) -> Result<()> {
    let target = state
        .store
        .lock()
        .unwrap()
        .folder(folder)?
        .ok_or_else(|| Error::Storage("that folder no longer exists".into()))?;
    let located = state.store.lock().unwrap().locate(ids)?;
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
    change(app, state, &ids, |m| Op::Move {
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
pub(crate) fn to_role(
    app: &AppHandle,
    state: &AppState,
    ids: &[i64],
    role: FolderRole,
) -> Result<()> {
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
pub(crate) fn change(
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
    fn version_matches_the_installer() {
        // The updater compares installers by the version in tauri.conf.json.
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(super::app_version(), config["version"]);
    }
}
