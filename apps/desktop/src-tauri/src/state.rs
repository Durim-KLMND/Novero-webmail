//! Shared state behind the commands, and the background work for each account:
//! syncing, pushing local changes, drafts and outgoing mail to the servers,
//! and IDLE on the inbox.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mail_auth::SecretStore;
use mail_auth::oauth::{self, OAuthClient};
use mail_core::{Account, AccountId, AuthMethod, Error, Event, OAuthProvider, Result, Secret};
use mail_store::{OutboxState, Store};
use mail_sync::{IdleChange, ImapSession, NewMail, Scope};
use tauri_plugin_notification::NotificationExt;

use crate::settings::Settings;
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
/// Changes such as messages marked read wait this long after the last one,
/// so a burst of them goes to the server together, but never longer than
/// `SETTLE_MAX` in all. Mail to send doesn't wait.
const SETTLE: Duration = Duration::from_millis(1500);
const SETTLE_MAX: Duration = Duration::from_secs(8);
/// Flag changes the server reports this soon after we sent some are taken
/// to be ours, and don't start a sync.
const ECHO_WINDOW: Duration = Duration::from_secs(20);

/// An access token this close to expiring is replaced before use, so it
/// cannot run out in the middle of a sync.
const TOKEN_MARGIN: i64 = 5 * 60;

pub struct AppState {
    /// The app's data folder: the database, cached mail and crash reports.
    pub data_dir: PathBuf,
    pub store: Arc<Mutex<Store>>,
    pub secrets: Arc<dyn SecretStore>,
    /// Accounts with a sync running, and the follow-up sync requested meanwhile:
    /// `Some(true)` for a full one, `Some(false)` for the inbox only.
    syncing: Mutex<HashMap<AccountId, Option<bool>>>,
    workers: Mutex<HashMap<AccountId, Worker>>,
    /// Current OAuth2 access tokens and when they expire. Kept in memory
    /// only; the refresh token in the credential store gets new ones.
    tokens: Mutex<HashMap<AccountId, (String, i64)>>,
    /// When each account last sent changes to the server, so the server
    /// telling us about them again can be ignored.
    pushed: Mutex<HashMap<AccountId, tokio::time::Instant>>,
    /// The connection each account keeps open to download messages as they open.
    pub readers: crate::reader::Readers,
    /// Ends the browser sign-in that is waiting, when the person cancels.
    pub sign_in: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
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
            data_dir: dir.to_owned(),
            store: Arc::new(Mutex::new(Store::open(dir)?)),
            secrets: mail_auth::platform_store(),
            syncing: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
            pushed: Mutex::new(HashMap::new()),
            readers: Default::default(),
            sign_in: Mutex::new(None),
        })
    }

    pub fn account(&self, id: AccountId) -> Result<Account> {
        self.store
            .lock()
            .unwrap()
            .account(id)?
            .ok_or_else(|| Error::Storage("that account no longer exists".into()))
    }

    /// What the account signs in with now: its password, or a current
    /// access token, refreshed when the last one is about to expire.
    pub async fn secret(&self, account: &Account) -> Result<Secret> {
        let saved = self.secrets.get(&secret_key(account.id))?;
        let provider = match account.auth {
            AuthMethod::Password => {
                return saved
                    .map(Secret::Password)
                    .ok_or_else(|| Error::Auth("the saved password is missing".into()));
            }
            AuthMethod::OAuth2 { provider } => provider,
        };
        if let Some((token, expires_at)) = self.tokens.lock().unwrap().get(&account.id)
            && *expires_at > oauth::now() + TOKEN_MARGIN
        {
            return Ok(Secret::AccessToken(token.clone()));
        }
        let refresh_token = saved
            .ok_or_else(|| Error::Auth(format!("the {} sign-in is missing", provider.label())))?;
        let tokens = oauth::refresh(&oauth_client(provider)?, &refresh_token).await?;
        self.keep_tokens(account.id, &tokens)?;
        Ok(Secret::AccessToken(tokens.access_token))
    }

    /// Saves the refresh token, if a new one came, and caches the access token.
    pub fn keep_tokens(&self, id: AccountId, tokens: &oauth::Tokens) -> Result<()> {
        if let Some(refresh_token) = &tokens.refresh_token {
            self.secrets.set(&secret_key(id), refresh_token)?;
        }
        self.tokens
            .lock()
            .unwrap()
            .insert(id, (tokens.access_token.clone(), tokens.expires_at));
        Ok(())
    }

    /// Forgets an account's cached access token, e.g. when it is removed.
    pub fn forget_tokens(&self, id: AccountId) {
        self.tokens.lock().unwrap().remove(&id);
    }
}

/// The app registered with `provider`, if this build has its client id.
pub fn oauth_client(provider: OAuthProvider) -> Result<OAuthClient> {
    OAuthClient::for_provider(provider).ok_or_else(|| {
        Error::Auth(format!(
            "this version of Novero Mail cannot sign in with {}",
            provider.label()
        ))
    })
}

/// Credential store key for an account's password or refresh token.
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
    state.readers.close(id);
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

/// Tells the UI how many messages wait to send or were refused.
pub fn report_outbox(app: &AppHandle, id: AccountId, error: Option<String>) {
    let (queued, failed, scheduled) = app
        .state::<AppState>()
        .store
        .lock()
        .unwrap()
        .outbox_counts(id)
        .unwrap_or_default();
    emit(
        app,
        Event::OutboxChanged {
            account: id,
            queued,
            failed,
            scheduled,
            error,
        },
    );
}

/// Whether the account has changes, drafts or mail still to send.
fn has_outgoing(app: &AppHandle, id: AccountId) -> bool {
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    store.pending_count(id).unwrap_or(0) > 0
        || store.drafts_to_upload(id).is_ok_and(|d| !d.is_empty())
        || store.outbox(id).is_ok_and(|o| {
            let now = crate::compose::now();
            o.iter()
                .any(|i| i.state != OutboxState::Failed && !i.waiting(now))
        })
}

/// Whether the account has mail to send now.
fn mail_due(app: &AppHandle, id: AccountId) -> bool {
    let now = crate::compose::now();
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .outbox(id)
        .is_ok_and(|o| {
            o.iter()
                .any(|i| i.state == OutboxState::Queued && !i.waiting(now))
        })
}

/// Sends queued changes, drafts and mail whenever woken, retrying with backoff
/// while offline. This is the only task that sends an account's changes and
/// mail, so nothing goes twice.
async fn push_loop(app: AppHandle, id: AccountId, wake: Arc<Notify>) {
    loop {
        wake.notified().await;
        let started = tokio::time::Instant::now();
        while !mail_due(&app, id) && started.elapsed() < SETTLE_MAX {
            let quiet = SETTLE.min(SETTLE_MAX - started.elapsed());
            tokio::select! {
                _ = tokio::time::sleep(quiet) => break,
                _ = wake.notified() => {}
            }
        }
        let mut delay = RETRY_FIRST;
        loop {
            let state = app.state::<AppState>();
            let login = match state.account(id) {
                Ok(account) => state.secret(&account).await.map(|s| (account, s)),
                Err(e) => Err(e),
            };
            let (account, secret) = match login {
                Ok(login) => login,
                Err(Error::Network(e)) => {
                    // The sign-in provider is out of reach: retry like any connection.
                    report_pending(&app, id, Some(format!("offline, will retry ({e})")));
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = wake.notified() => {}
                    }
                    delay = (delay * 2).min(RETRY_MAX);
                    continue;
                }
                Err(e) => {
                    // A removed account or a missing or refused sign-in: nothing to retry.
                    report_pending(&app, id, Some(e.to_string()));
                    report_outbox(&app, id, Some(e.to_string()));
                    break;
                }
            };
            let changes =
                mail_sync::push_changes(&state.store, &account, &secret, |e| emit(&app, e)).await;
            let outbox =
                mail_send::flush_outbox(&state.store, &account, &secret, |e| emit(&app, e)).await;

            if changes.as_ref().is_ok_and(|c| c.applied > 0) {
                state
                    .pushed
                    .lock()
                    .unwrap()
                    .insert(id, tokio::time::Instant::now());
            }
            let offline = matches!(changes, Err(Error::Network(_)))
                || matches!(outbox, Err(Error::Network(_)));
            let mut changed = false;
            let changes_error = match changes {
                Ok(outcome) => {
                    // Changes the server took are on screen already; only
                    // undone ones and folders synced afterwards change lists.
                    changed |= !outcome.rejected.is_empty() || !outcome.touched.is_empty();
                    (!outcome.rejected.is_empty()).then(|| {
                        format!(
                            "the server refused a change, so it was undone: {}",
                            outcome.rejected.join("; ")
                        )
                    })
                }
                Err(Error::Network(e)) => Some(format!("offline, will retry ({e})")),
                Err(e) => Some(e.to_string()),
            };
            let outbox_error = match outbox {
                Ok(outcome) => {
                    changed |= outcome.saved_to.is_some();
                    (!outcome.refused.is_empty()).then(|| {
                        format!("the server refused to send: {}", outcome.refused.join("; "))
                    })
                }
                Err(Error::Network(e)) => Some(format!("offline, will retry ({e})")),
                Err(e) => Some(e.to_string()),
            };
            if changed {
                emit(&app, Event::MailboxChanged { account: id });
            }
            report_pending(&app, id, changes_error);
            report_outbox(&app, id, outbox_error);
            if !offline {
                break;
            }
            // Retry later, or sooner if the user acts again.
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = wake.notified() => {}
            }
            delay = (delay * 2).min(RETRY_MAX);
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
            let secret = state.secret(&account).await?;
            let mut session =
                ImapSession::connect(&account.imap, &account.username, &secret).await?;
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
                match changed {
                    IdleChange::Mail => request_sync(&app, id, false),
                    // Most likely our own messages marked read coming back;
                    // otherwise another device's, picked up quietly.
                    IdleChange::Flags if !pushed_lately(&app, id) => request_sync(&app, id, false),
                    _ => {}
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

/// Whether the account sent changes to the server in the last moments.
fn pushed_lately(app: &AppHandle, id: AccountId) -> bool {
    app.state::<AppState>()
        .pushed
        .lock()
        .unwrap()
        .get(&id)
        .is_some_and(|at| at.elapsed() < ECHO_WINDOW)
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
        let secret = state.secret(&account).await?;
        let scope = if full {
            Scope::All
        } else {
            Scope::Folder("INBOX")
        };
        mail_sync::sync_account(&state.store, &account, &secret, scope, |e| emit(app, e)).await
    }
    .await;
    if let Ok(report) = &result {
        // Rules first, so mail they file away or mark read isn't announced.
        let left = crate::rules::apply(app, id, &report.new_mail);
        announce(app, id, &left);
    }
    if result.is_ok() && has_outgoing(app, id) {
        // Reachable again with changes or mail still waiting: send them.
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

/// Shows a Windows notification for new mail, unless Novero Mail is the
/// window in front or notifications are turned off.
fn announce(app: &AppHandle, id: AccountId, new_mail: &[NewMail]) {
    if new_mail.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    if !Settings::load(&state.store.lock().unwrap()).notifications {
        return;
    }
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if focused {
        return;
    }
    let account = state.account(id).map(|a| a.email).unwrap_or_default();
    let (title, body) = notification_text(&account, new_mail);
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Shows a sample new-mail notification, after `delay` seconds so there is
/// time to switch to another window. Shown even when Novero Mail is in front
/// or notifications are off, since that is what is being tried.
pub async fn test_notification(app: &AppHandle, several: bool, delay: u64) -> Result<()> {
    if delay > 0 {
        tokio::time::sleep(Duration::from_secs(delay.min(60))).await;
    }
    let account = app
        .state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_accounts()?
        .into_iter()
        .next()
        .map(|a| a.email)
        .unwrap_or_else(|| "you@example.com".into());
    let (title, body) = notification_text(&account, &sample_mail(several));
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| Error::Protocol(format!("Windows didn't show the notification: {e}")))
}

/// Made-up mail for a test notification: one message, or a handful.
fn sample_mail(several: bool) -> Vec<NewMail> {
    let samples = [
        (
            "Novero Mail <hello@noveromail.com>",
            "This is how new mail looks",
        ),
        ("Ann Lee <ann@example.com>", "Lunch on Friday?"),
        ("Sam Patel <sam@example.com>", "Notes from today's meeting"),
        ("Billing <billing@example.com>", "Your receipt"),
        ("Jo Kim <jo@example.com>", "Photos from the weekend"),
    ];
    samples
        .iter()
        .take(if several { samples.len() } else { 1 })
        .map(|(from, subject)| NewMail {
            id: 0,
            from: Some((*from).into()),
            subject: Some((*subject).into()),
        })
        .collect()
}

/// The title and text of a new-mail notification: the message itself when
/// one arrived, else a count and the newest few subjects.
fn notification_text(account: &str, new_mail: &[NewMail]) -> (String, String) {
    let sender = |m: &NewMail| {
        let from = m.from.as_deref().unwrap_or("Someone");
        // "Ann Lee <ann@example.test>" shows as "Ann Lee".
        match from.split_once(" <") {
            Some((name, _)) if !name.trim().is_empty() => name.trim().trim_matches('"').to_owned(),
            _ => from.trim_matches(|c| c == '<' || c == '>').to_owned(),
        }
    };
    let subject = |m: &NewMail| {
        m.subject
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("(no subject)")
            .to_owned()
    };
    match new_mail {
        [one] => (sender(one), subject(one)),
        many => {
            let lines: Vec<String> = many
                .iter()
                .take(3)
                .map(|m| format!("{}: {}", sender(m), subject(m)))
                .collect();
            let more = many.len().saturating_sub(3);
            let mut body = lines.join("\n");
            if more > 0 {
                body.push_str(&format!("\nand {more} more"));
            }
            (format!("{} new messages for {account}", many.len()), body)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(from: &str, subject: &str) -> NewMail {
        NewMail {
            id: 1,
            from: Some(from.into()),
            subject: Some(subject.into()),
        }
    }

    #[test]
    fn one_new_message_is_shown_by_sender_and_subject() {
        let (title, body) = notification_text("me@x", &[mail("Ann Lee <ann@x>", "Lunch?")]);
        assert_eq!((title.as_str(), body.as_str()), ("Ann Lee", "Lunch?"));
        let (title, _) = notification_text("me@x", &[mail("ann@x", "")]);
        assert_eq!(title, "ann@x");
    }

    #[test]
    fn test_notifications_look_like_real_ones() {
        let (title, body) = notification_text("me@x", &sample_mail(false));
        assert_eq!(
            (title.as_str(), body.as_str()),
            ("Novero Mail", "This is how new mail looks")
        );
        let (title, body) = notification_text("me@x", &sample_mail(true));
        assert_eq!(title, "5 new messages for me@x");
        assert!(body.ends_with("and 2 more"));
    }

    #[test]
    fn several_are_counted_with_the_first_few_listed() {
        let list: Vec<NewMail> = (1..=5)
            .map(|i| mail(&format!("P{i} <p{i}@x>"), &format!("S{i}")))
            .collect();
        let (title, body) = notification_text("me@x", &list);
        assert_eq!(title, "5 new messages for me@x");
        assert_eq!(body, "P1: S1\nP2: S2\nP3: S3\nand 2 more");
    }
}
