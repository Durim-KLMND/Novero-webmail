//! Shared state behind the commands, and the background work for each account:
//! syncing, pushing local changes to the server, and IDLE on the inbox.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mail_auth::SecretStore;
use mail_core::{Account, AccountId, Error, Event, Result};
use mail_store::Store;
use mail_sync::{ImapSession, Scope};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

/// Name of the event that carries [`Event`]s to the UI.
pub const MAIL_EVENT: &str = "mail-event";

/// How long one IDLE lasts before it is renewed. RFC 2177 asks for under 29 minutes.
const IDLE_WAIT: Duration = Duration::from_secs(25 * 60);

/// Retry delays after a failed connection, doubling up to the maximum.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub secrets: Arc<dyn SecretStore>,
    /// Accounts with a sync running, and the follow-up sync requested meanwhile:
    /// `Some(true)` for a full one, `Some(false)` for the inbox only.
    syncing: Mutex<HashMap<AccountId, Option<bool>>>,
    workers: Mutex<HashMap<AccountId, Worker>>,
}

/// Background tasks of one account.
struct Worker {
    /// Wakes the task that sends queued changes.
    push: Arc<Notify>,
    tasks: Vec<JoinHandle<()>>,
}

impl AppState {
    pub fn open(dir: &Path) -> Result<Self> {
        Ok(Self {
            store: Arc::new(Mutex::new(Store::open(dir)?)),
            secrets: mail_auth::platform_store(),
            syncing: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
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

pub fn emit(app: &AppHandle, event: Event) {
    let _ = app.emit(MAIL_EVENT, event);
}

/// Starts the background tasks of an account: pushing changes and IDLE.
pub fn start_account(app: &AppHandle, id: AccountId) {
    let state = app.state::<AppState>();
    let mut workers = state.workers.lock().unwrap();
    if workers.contains_key(&id) {
        return;
    }
    let push = Arc::new(Notify::new());
    let tasks = vec![
        tauri::async_runtime::spawn(push_loop(app.clone(), id, push.clone())),
        tauri::async_runtime::spawn(idle_loop(app.clone(), id)),
    ];
    // Changes left in the queue from the last run go out now.
    push.notify_one();
    workers.insert(id, Worker { push, tasks });
}

/// Stops an account's background tasks, e.g. when it is removed.
pub fn stop_account(app: &AppHandle, id: AccountId) {
    let state = app.state::<AppState>();
    if let Some(worker) = state.workers.lock().unwrap().remove(&id) {
        for task in worker.tasks {
            task.abort();
        }
    }
}

/// Asks the account's push task to send queued changes now.
pub fn push_now(app: &AppHandle, id: AccountId) {
    let state = app.state::<AppState>();
    if let Some(worker) = state.workers.lock().unwrap().get(&id) {
        worker.push.notify_one();
    }
}

/// Tells the UI how many changes are still waiting for the server.
pub fn report_pending(app: &AppHandle, id: AccountId, error: Option<String>) {
    let pending = app
        .state::<AppState>()
        .store
        .lock()
        .unwrap()
        .pending_count(id)
        .unwrap_or(0);
    emit(
        app,
        Event::PendingChanges {
            account: id,
            pending,
            error,
        },
    );
}

/// Sends queued changes whenever woken, retrying with backoff while offline.
/// This is the only task that sends an account's changes, so none goes twice.
async fn push_loop(app: AppHandle, id: AccountId, wake: Arc<Notify>) {
    loop {
        wake.notified().await;
        let mut delay = RETRY_FIRST;
        loop {
            let state = app.state::<AppState>();
            let result = async {
                let account = state.account(id)?;
                let password = state.password(id)?;
                mail_sync::push_changes(&state.store, &account, &password, |e| emit(&app, e)).await
            }
            .await;
            match result {
                Ok(outcome) => {
                    if outcome.applied > 0 || !outcome.rejected.is_empty() {
                        emit(&app, Event::MailboxChanged { account: id });
                    }
                    let error = (!outcome.rejected.is_empty()).then(|| {
                        format!(
                            "the server refused a change, so it was undone: {}",
                            outcome.rejected.join("; ")
                        )
                    });
                    report_pending(&app, id, error);
                    break;
                }
                Err(Error::Network(e)) => {
                    report_pending(&app, id, Some(format!("offline, will retry ({e})")));
                    // Retry later, or sooner if the user acts again.
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = wake.notified() => {}
                    }
                    delay = (delay * 2).min(RETRY_MAX);
                }
                Err(e) => {
                    report_pending(&app, id, Some(e.to_string()));
                    break;
                }
            }
        }
    }
}

/// Keeps a connection in IDLE on the inbox, so new mail appears without polling.
/// Servers without IDLE rely on the periodic sync instead.
async fn idle_loop(app: AppHandle, id: AccountId) {
    let mut delay = RETRY_FIRST;
    let mut reconnecting = false;
    loop {
        let state = app.state::<AppState>();
        let result: Result<()> = async {
            let account = state.account(id)?;
            let password = state.password(id)?;
            let mut session =
                ImapSession::connect(&account.imap, &account.username, &password).await?;
            if !session.has("IDLE") {
                return Err(Error::Protocol("the server does not support IDLE".into()));
            }
            if reconnecting {
                // Back after a drop: catch up on what was missed and send what waited.
                request_sync(&app, id, true);
                push_now(&app, id);
            }
            delay = RETRY_FIRST;
            loop {
                session.select("INBOX").await?;
                let (s, changed) = session.idle(IDLE_WAIT).await?;
                session = s;
                if changed {
                    request_sync(&app, id, false);
                }
            }
        }
        .await;
        match result {
            Err(Error::Network(_)) => {}
            // No IDLE, a removed account or a bad password: nothing to retry.
            _ => return,
        }
        reconnecting = true;
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RETRY_MAX);
    }
}

/// Starts a sync of one account: every folder when `full`, else the inbox.
/// If one is already running, another runs right after it.
pub fn request_sync(app: &AppHandle, id: AccountId, full: bool) {
    let state = app.state::<AppState>();
    {
        let mut syncing = state.syncing.lock().unwrap();
        if let Some(next) = syncing.get_mut(&id) {
            *next = Some(next.unwrap_or(false) || full);
            return;
        }
        syncing.insert(id, None);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut full = full;
        loop {
            run_sync(&app, id, full).await;
            let state = app.state::<AppState>();
            let mut syncing = state.syncing.lock().unwrap();
            match syncing.get(&id).copied().flatten() {
                Some(next) => {
                    full = next;
                    syncing.insert(id, None);
                }
                None => {
                    syncing.remove(&id);
                    break;
                }
            }
        }
    });
}

async fn run_sync(app: &AppHandle, id: AccountId, full: bool) {
    let state = app.state::<AppState>();
    emit(app, Event::SyncStarted { account: id });
    let result = async {
        let account = state.account(id)?;
        let password = state.password(id)?;
        let scope = if full {
            Scope::All
        } else {
            Scope::Folder("INBOX")
        };
        mail_sync::sync_account(&state.store, &account, &password, scope, |e| emit(app, e)).await
    }
    .await;
    if result.is_ok() && state.store.lock().unwrap().pending_count(id).unwrap_or(0) > 0 {
        // Reachable again with changes still waiting: send them.
        push_now(app, id);
    }
    emit(
        app,
        Event::SyncFinished {
            account: id,
            error: result.err().map(|e| e.to_string()),
        },
    );
}

/// Starts a full sync of every account.
pub fn sync_all(app: &AppHandle) {
    for id in account_ids(app) {
        request_sync(app, id, true);
    }
}

/// Starts background work and a first sync for every saved account.
pub fn start_all(app: &AppHandle) {
    for id in account_ids(app) {
        start_account(app, id);
    }
    sync_all(app);
}

fn account_ids(app: &AppHandle) -> Vec<AccountId> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_accounts()
        .map(|accounts| accounts.into_iter().map(|a| a.id).collect())
        .unwrap_or_default()
}
