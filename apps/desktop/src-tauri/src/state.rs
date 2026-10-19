//! Shared state behind the commands, and background sync.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use mail_auth::SecretStore;
use mail_core::{Account, AccountId, Error, Event, Result};
use mail_store::Store;
use tauri::{AppHandle, Emitter, Manager};

/// Name of the event that carries [`Event`]s to the UI.
pub const MAIL_EVENT: &str = "mail-event";

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub secrets: Arc<dyn SecretStore>,
    syncing: Mutex<HashSet<AccountId>>,
}

impl AppState {
    pub fn open(dir: &Path) -> Result<Self> {
        Ok(Self {
            store: Arc::new(Mutex::new(Store::open(dir)?)),
            secrets: mail_auth::platform_store(),
            syncing: Mutex::new(HashSet::new()),
        })
    }

    pub fn account(&self, id: AccountId) -> Result<Account> {
        self.store
            .lock()
            .unwrap()
            .account(id)?
            .ok_or_else(|| Error::Storage("that account no longer exists".into()))
    }

    pub fn password(&self, id: AccountId) -> Result<String> {
        self.secrets.get(&secret_key(id))?.ok_or_else(|| {
            Error::Auth("the saved password is missing; remove the account and add it again".into())
        })
    }
}

/// Credential store key for an account's password.
pub fn secret_key(id: AccountId) -> String {
    format!("account-{}", id.0)
}

/// Starts a background sync of one account unless one is already running.
pub fn spawn_sync(app: &AppHandle, id: AccountId) {
    let state = app.state::<AppState>();
    if !state.syncing.lock().unwrap().insert(id) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _ = app.emit(MAIL_EVENT, Event::SyncStarted { account: id });
        let result = async {
            let account = state.account(id)?;
            let password = state.password(id)?;
            mail_sync::sync_account(&state.store, &account, &password, |event| {
                let _ = app.emit(MAIL_EVENT, event);
            })
            .await
        }
        .await;
        state.syncing.lock().unwrap().remove(&id);
        let _ = app.emit(
            MAIL_EVENT,
            Event::SyncFinished {
                account: id,
                error: result.err().map(|e| e.to_string()),
            },
        );
    });
}

/// Starts a sync of every account.
pub fn sync_all(app: &AppHandle) {
    let ids = match app
        .state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_accounts()
    {
        Ok(accounts) => accounts.into_iter().map(|a| a.id).collect(),
        Err(_) => Vec::new(),
    };
    for id in ids {
        spawn_sync(app, id);
    }
}
