//! Downloading messages as they are opened. Each account keeps one
//! connection open for this, so opening one message after another doesn't
//! sign in each time, and the next few older messages come down with it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mail_core::{Account, AccountId, Error, Result};
use mail_sync::ImapSession;
use tauri::{AppHandle, Manager};
use tokio::time::Instant;

use crate::state::AppState;

/// A connection unused for this long is closed rather than reused: servers
/// drop quiet connections, and a stale one would only fail first.
const KEEP_OPEN: Duration = Duration::from_secs(4 * 60);
/// When a message has to be downloaded, this many older ones in its folder
/// come down too, if each is at most `AHEAD_MAX_SIZE`.
const AHEAD: u32 = 5;
const AHEAD_MAX_SIZE: u32 = 512 * 1024;

type Slot = Arc<tokio::sync::Mutex<Option<(ImapSession, Instant)>>>;

/// The open connection of each account, if any.
#[derive(Default)]
pub struct Readers(Mutex<HashMap<AccountId, Slot>>);

impl Readers {
    fn slot(&self, id: AccountId) -> Slot {
        self.0.lock().unwrap().entry(id).or_default().clone()
    }

    /// Closes an account's connection, e.g. when the account is removed.
    pub fn close(&self, id: AccountId) {
        self.0.lock().unwrap().remove(&id);
    }
}

/// A message's raw bytes: from the cache, or downloaded over the account's
/// open connection. A download also fetches the next few older messages.
pub async fn raw(app: &AppHandle, account: &Account, id: i64) -> Result<Vec<u8>> {
    let state = app.state::<AppState>();
    if let Some(raw) = mail_sync::cached_message(&state.store, id)? {
        return Ok(raw);
    }
    let slot = state.readers.slot(account.id);
    let raw = {
        let mut open = slot.lock().await;
        // It may have come down ahead while we waited for the connection.
        if let Some(raw) = mail_sync::cached_message(&state.store, id)? {
            return Ok(raw);
        }
        let mut session = connection(&state, account, &mut open).await?;
        let raw = match mail_sync::download_message(&state.store, &mut session.0, id).await {
            // A reused connection may have been dropped by the server: once more on a new one.
            Err(Error::Network(_)) if session.1 => {
                session = (connect(&state, account).await?, false);
                mail_sync::download_message(&state.store, &mut session.0, id).await
            }
            other => other,
        };
        if !matches!(raw, Err(Error::Network(_))) {
            *open = Some((session.0, Instant::now()));
        }
        raw?
    };
    let app = app.clone();
    let account = account.clone();
    tauri::async_runtime::spawn(async move { download_ahead(&app, &account, id).await });
    Ok(raw)
}

/// Downloads the messages likely to be opened after `id`. Best effort: a
/// failure here only means they download when opened.
async fn download_ahead(app: &AppHandle, account: &Account, id: i64) {
    let state = app.state::<AppState>();
    let Ok(source) = state.store.lock().unwrap().message_source(id) else {
        return;
    };
    let wanted = match state
        .store
        .lock()
        .unwrap()
        .bodies_after(id, AHEAD_MAX_SIZE, AHEAD)
    {
        Ok(wanted) if !wanted.is_empty() => wanted,
        _ => return,
    };
    let slot = state.readers.slot(account.id);
    let mut open = slot.lock().await;
    let Ok(mut session) = connection(&state, account, &mut open).await else {
        return;
    };
    let done =
        mail_sync::download_messages(&state.store, &mut session.0, &source.folder_name, &wanted)
            .await;
    if !matches!(done, Err(Error::Network(_))) {
        *open = Some((session.0, Instant::now()));
    }
}

/// The account's open connection if it is fresh, else a new one; with
/// whether it was reused.
async fn connection(
    state: &AppState,
    account: &Account,
    open: &mut Option<(ImapSession, Instant)>,
) -> Result<(ImapSession, bool)> {
    match open.take() {
        Some((session, used)) if used.elapsed() < KEEP_OPEN => Ok((session, true)),
        _ => Ok((connect(state, account).await?, false)),
    }
}

async fn connect(state: &AppState, account: &Account) -> Result<ImapSession> {
    let secret = state.secret(account).await?;
    ImapSession::connect(&account.imap, &account.username, &secret).await
}
