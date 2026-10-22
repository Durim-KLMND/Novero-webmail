//! Calendars synced with Google and Microsoft accounts: connecting an
//! account, and the background task that sends changes made here and takes
//! in the server's.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mail_auth::oauth::{self, OAuthClient};
use mail_calendar_sync::Client;
use mail_core::calendar::CalendarAccount;
use mail_core::{Error, OAuthProvider, Result};
use mail_store::SyncedCalendar;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Notify;

use crate::calendar::changed;
use crate::commands::browser_sign_in;
use crate::plan::{self, Feature};
use crate::state::AppState;

/// How often calendars are synced when nothing asks sooner.
const INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Changes wait this long after the last one, so a burst goes together.
const SETTLE: Duration = Duration::from_millis(1500);
/// An access token this close to expiring is replaced before use.
const TOKEN_MARGIN: i64 = 5 * 60;

#[derive(Default)]
pub struct CalendarSync {
    wake: Notify,
    syncing: AtomicBool,
    /// Access tokens by calendar account, with when they expire.
    tokens: Mutex<HashMap<i64, (String, i64)>>,
}

/// Credential store key for a calendar account's refresh token.
fn secret_key(id: i64) -> String {
    format!("calendar-{id}")
}

fn client_for(provider: OAuthProvider) -> Result<OAuthClient> {
    OAuthClient::for_calendar(provider).ok_or_else(|| {
        Error::Auth(format!(
            "this version of Novero Mail cannot sign in with {}",
            provider.label()
        ))
    })
}

/// Syncs soon, e.g. after a change in an online calendar.
pub fn sync_soon(app: &AppHandle) {
    app.state::<CalendarSync>().wake.notify_one();
}

/// Starts the background sync.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            sync_all(&app).await;
            let sync = app.state::<CalendarSync>();
            tokio::select! {
                _ = tokio::time::sleep(INTERVAL) => {}
                _ = sync.wake.notified() => tokio::time::sleep(SETTLE).await,
            }
        }
    });
}

async fn sync_all(app: &AppHandle) {
    let accounts = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        if !plan::is_pro(&store) {
            return;
        }
        store.list_calendar_accounts().unwrap_or_default()
    };
    if accounts.is_empty() {
        return;
    }
    let sync = app.state::<CalendarSync>();
    sync.syncing.store(true, Ordering::Relaxed);
    changed(app);
    for account in accounts {
        let outcome = sync_account(app, &account).await;
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let _ = match &outcome {
            Ok(note) => store.set_calendar_account_status(
                account.id,
                Some(oauth::now()),
                note.as_deref(),
                false,
            ),
            Err(err) => store.set_calendar_account_status(
                account.id,
                None,
                Some(&err.to_string()),
                matches!(err, Error::Auth(_)),
            ),
        };
    }
    sync.syncing.store(false, Ordering::Relaxed);
    changed(app);
}

/// A current access token for the account, refreshed when about to expire.
async fn access_token(app: &AppHandle, account: &CalendarAccount) -> Result<String> {
    let sync = app.state::<CalendarSync>();
    if let Some((token, expires_at)) = sync.tokens.lock().unwrap().get(&account.id)
        && *expires_at > oauth::now() + TOKEN_MARGIN
    {
        return Ok(token.clone());
    }
    let state = app.state::<AppState>();
    let refresh_token = state.secrets.get(&secret_key(account.id))?.ok_or_else(|| {
        Error::Auth(format!(
            "the {} sign-in is missing",
            account.provider.label()
        ))
    })?;
    let tokens = oauth::refresh(&client_for(account.provider)?, &refresh_token).await?;
    keep_tokens(app, account.id, &tokens)?;
    Ok(tokens.access_token)
}

fn keep_tokens(app: &AppHandle, id: i64, tokens: &oauth::Tokens) -> Result<()> {
    if let Some(refresh_token) = &tokens.refresh_token {
        app.state::<AppState>()
            .secrets
            .set(&secret_key(id), refresh_token)?;
    }
    app.state::<CalendarSync>()
        .tokens
        .lock()
        .unwrap()
        .insert(id, (tokens.access_token.clone(), tokens.expires_at));
    Ok(())
}

/// Syncs one account's calendars. Returns a note on changes the server
/// refused, which were undone here.
async fn sync_account(app: &AppHandle, account: &CalendarAccount) -> Result<Option<String>> {
    let token = access_token(app, account).await?;
    let client = Client::new(account.provider, &token)?;
    let remote = client.calendars().await?;
    let calendars = with_store(app, |s| s.sync_calendar_list(account.id, &remote))?;
    changed(app);
    let mut note = None;
    for calendar in calendars {
        let full = push(app, &client, &calendar, &mut note).await?;
        pull(app, &client, &calendar, full).await?;
    }
    Ok(note)
}

fn with_store<T>(app: &AppHandle, f: impl FnOnce(&mail_store::Store) -> Result<T>) -> Result<T> {
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    f(&store)
}

/// Sends a calendar's waiting changes. Returns whether the calendar needs
/// a full pull afterwards.
async fn push(
    app: &AppHandle,
    client: &Client,
    calendar: &SyncedCalendar,
    note: &mut Option<String>,
) -> Result<bool> {
    let pending = with_store(app, |s| s.pending(calendar.id))?;
    let mut full = false;
    for id in &pending.deletions {
        match client.delete(&calendar.remote_id, id).await {
            Ok(()) => {}
            Err(Error::Rejected(why) | Error::Invalid(why)) => {
                // It stays on the server; a full pull brings it back.
                note.get_or_insert(why);
                full = true;
            }
            Err(err) => return Err(err),
        }
        with_store(app, |s| s.deletion_done(calendar.id, id))?;
    }
    for save in &pending.saves {
        let result = match &save.remote_id {
            None => client
                .create(&calendar.remote_id, &save.event, &save.moved)
                .await
                .map(Some),
            Some(id) => {
                client
                    .update(&calendar.remote_id, id, &save.event, &save.moved)
                    .await
            }
        };
        match result {
            Ok(Some(saved)) => {
                with_store(app, |s| {
                    s.pushed(save.event.id, save.dirty, &saved.id, saved.etag.as_deref())
                })?;
                if save.event.rrule.is_some() && !client.keeps_series() {
                    full = true;
                }
            }
            // Deleted on the server meanwhile: the deletion wins.
            Ok(None) => {
                with_store(app, |s| s.drop_change(save.event.id, calendar.id))?;
                full = true;
            }
            Err(Error::Rejected(why) | Error::Invalid(why)) => {
                note.get_or_insert(why);
                with_store(app, |s| s.drop_change(save.event.id, calendar.id))?;
                full = true;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(full)
}

async fn pull(
    app: &AppHandle,
    client: &Client,
    calendar: &SyncedCalendar,
    full: bool,
) -> Result<()> {
    let since = if full {
        None
    } else {
        calendar.sync_token.as_deref()
    };
    let found = match client.pull(&calendar.remote_id, since).await? {
        Some(found) => found,
        None if since.is_some() => match client.pull(&calendar.remote_id, None).await? {
            Some(found) => found,
            None => return Ok(()),
        },
        None => return Ok(()),
    };
    let any = with_store(app, |s| {
        let any = s.apply_remote(calendar.id, &found.changes, found.complete)?;
        s.set_sync_token(calendar.id, found.token.as_deref())?;
        Ok(any)
    })?;
    if any {
        changed(app);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarAccounts {
    accounts: Vec<CalendarAccount>,
    /// A sync is running now.
    syncing: bool,
    /// Providers this build can sign in with.
    providers: Vec<OAuthProvider>,
}

#[tauri::command]
pub fn list_calendar_accounts(
    state: State<'_, AppState>,
    sync: State<'_, CalendarSync>,
) -> Result<CalendarAccounts> {
    Ok(CalendarAccounts {
        accounts: state.store.lock().unwrap().list_calendar_accounts()?,
        syncing: sync.syncing.load(Ordering::Relaxed),
        providers: [OAuthProvider::Google, OAuthProvider::Microsoft]
            .into_iter()
            .filter(|p| OAuthClient::for_calendar(*p).is_some())
            .collect(),
    })
}

/// Signs in to a Google or Microsoft account for its calendars and syncs
/// them. With `id`, signs that account in again.
#[tauri::command]
pub async fn connect_calendar(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: OAuthProvider,
    id: Option<i64>,
) -> Result<CalendarAccount> {
    plan::require(&state.store.lock().unwrap(), Feature::CalendarSync)?;
    let again = match id {
        Some(id) => Some(
            state
                .store
                .lock()
                .unwrap()
                .calendar_account(id)?
                .ok_or_else(|| Error::Invalid("That account is no longer connected.".into()))?,
        ),
        None => None,
    };
    let hint = again.as_ref().map(|a| a.email.as_str()).unwrap_or_default();
    let tokens = browser_sign_in(&app, &state, client_for(provider)?, hint).await?;
    let email = tokens.email.clone().ok_or_else(|| {
        Error::Auth(format!(
            "{} did not say which account signed in",
            provider.label()
        ))
    })?;
    if let Some(account) = &again
        && !account.email.eq_ignore_ascii_case(&email)
    {
        return Err(Error::Auth(format!(
            "you signed in as {email}; choose {} on the sign-in page",
            account.email
        )));
    }
    let account = state
        .store
        .lock()
        .unwrap()
        .add_calendar_account(provider, &email)?;
    keep_tokens(&app, account.id, &tokens)?;
    state
        .store
        .lock()
        .unwrap()
        .set_calendar_account_status(account.id, None, None, false)?;
    sync_soon(&app);
    changed(&app);
    Ok(account)
}

/// Removes an account's calendars from the app. Nothing changes online.
#[tauri::command]
pub fn disconnect_calendar(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().remove_calendar_account(id)?;
    state.secrets.delete(&secret_key(id))?;
    app.state::<CalendarSync>()
        .tokens
        .lock()
        .unwrap()
        .remove(&id);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn sync_calendars(app: AppHandle) {
    sync_soon(&app);
}
