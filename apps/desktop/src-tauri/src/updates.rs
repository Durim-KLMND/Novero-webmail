//! Updates from the project's GitHub releases.
//!
//! Each release publishes a `latest.json` manifest next to the installer, and
//! the app reads the one on the newest release. Installers are checked against
//! the update key built into the app, so a manifest alone can't make it
//! install anything else.
//!
//! Updates are looked for shortly after start-up and then every few hours.
//! With automatic downloads on (the default) they are downloaded before the
//! person is told, so "Restart to update" only has to run the installer;
//! with them off the person is told one is out and downloads it with a click.
//! Every change is sent to the UI as an [`UpdateStatus`].

use std::sync::Mutex;
use std::time::{Duration, Instant};

use mail_core::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::compose::now;
use crate::settings::Settings;
use crate::state::AppState;

/// The public half of the update signing key (see docs/releasing.md). Builds
/// without one can't update themselves.
const PUBKEY: Option<&str> = option_env!("NOVERO_UPDATER_PUBKEY");

const RELEASES: &str = "https://github.com/Durim-KLMND/Novero-webmail/releases";

/// The first look waits until start-up work is done.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Sent to the UI with the new [`UpdateStatus`] whenever it changes.
pub const UPDATE_STATUS: &str = "update-status";

/// Message windows get this long to save what was typed before installing.
const SAVE_DRAFTS: Duration = Duration::from_secs(5);

fn pubkey() -> Option<&'static str> {
    PUBKEY.map(str::trim).filter(|k| !k.is_empty())
}

/// The updater plugin, in builds that have an update key.
pub fn plugin<R: Runtime>() -> Option<tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config>> {
    pubkey().map(|key| tauri_plugin_updater::Builder::new().pubkey(key).build())
}

/// Where the manifest is: on the newest release, which GitHub serves under
/// `latest`. `NOVERO_UPDATE_ENDPOINT` replaces it, for trying out releases
/// from a local server.
fn endpoint() -> String {
    std::env::var("NOVERO_UPDATE_ENDPOINT")
        .unwrap_or_else(|_| format!("{RELEASES}/latest/download/latest.json"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// The release notes, when the release has any.
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Nothing newer is known.
    #[default]
    Idle,
    Checking,
    /// A newer version is out and hasn't been downloaded.
    Available,
    Downloading,
    /// Downloaded and waiting for a restart.
    Ready,
    /// The installer is about to take over.
    Installing,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub phase: Phase,
    /// The version found, from Available on.
    pub update: Option<UpdateInfo>,
    /// Bytes downloaded so far, and the size when the server says it.
    pub downloaded: u64,
    pub total: Option<u64>,
    /// When a check last reached the server, in seconds since 1970.
    pub last_checked: Option<i64>,
}

#[derive(Default)]
pub struct Updates {
    status: Mutex<UpdateStatus>,
    /// Found but not downloaded, while automatic downloads are off.
    found: Mutex<Option<Update>>,
    /// The downloaded update waiting to be installed.
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// One check or download at a time; a second one waits and then finds it
    /// done.
    busy: tokio::sync::Mutex<()>,
}

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone().filter(|b| !b.trim().is_empty()),
    }
}

fn failed(e: tauri_plugin_updater::Error) -> Error {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) => Error::Invalid(
            "the download didn't match the release's signature, so it was not installed".into(),
        ),
        E::ReleaseNotFound => Error::Network("no release was found yet".into()),
        e => Error::Network(e.to_string()),
    }
}

/// Changes the status and tells the UI.
fn set(app: &AppHandle, change: impl FnOnce(&mut UpdateStatus)) {
    let updates = app.state::<Updates>();
    let status = {
        let mut status = updates.status.lock().unwrap();
        change(&mut status);
        status.clone()
    };
    let _ = app.emit(UPDATE_STATUS, &status);
}

fn saved_settings(app: &AppHandle) -> Settings {
    let state = app.state::<AppState>();
    Settings::load(&state.store.lock().unwrap())
}

/// Looks for a newer release, and downloads it when `download`.
async fn find(app: &AppHandle, download: bool) -> Result<Option<UpdateInfo>> {
    let updates = app.state::<Updates>();
    let _one = updates.busy.lock().await;
    let before = updates.status.lock().unwrap().phase;
    // A downloaded update stays on show while looking for a newer one.
    if matches!(before, Phase::Idle | Phase::Available) {
        set(app, |s| s.phase = Phase::Checking);
    }
    let found = async {
        let url = Url::parse(&endpoint()).map_err(|e| Error::Invalid(e.to_string()))?;
        app.updater_builder()
            .endpoints(vec![url])
            .map_err(failed)?
            .build()
            .map_err(failed)?
            .check()
            .await
            .map_err(failed)
    }
    .await;
    let found = match found {
        Ok(found) => found,
        Err(e) => {
            set(app, |s| s.phase = before);
            return Err(e);
        }
    };
    let checked = Some(now());
    let Some(update) = found else {
        *updates.found.lock().unwrap() = None;
        let ready = updates.ready.lock().unwrap().is_some();
        set(app, |s| {
            s.last_checked = checked;
            if !ready {
                *s = UpdateStatus {
                    last_checked: checked,
                    ..UpdateStatus::default()
                };
            }
        });
        return Ok(None);
    };
    let ready = updates
        .ready
        .lock()
        .unwrap()
        .as_ref()
        .filter(|(ready, _)| ready.version == update.version)
        .map(|(ready, _)| info(ready));
    if let Some(ready) = ready {
        set(app, |s| {
            s.last_checked = checked;
            s.phase = Phase::Ready;
        });
        return Ok(Some(ready));
    }
    let found = info(&update);
    set(app, |s| {
        s.last_checked = checked;
        s.phase = Phase::Available;
        s.update = Some(found.clone());
        s.downloaded = 0;
        s.total = None;
    });
    if download {
        fetch(app, update).await?;
    } else {
        *updates.found.lock().unwrap() = Some(update);
    }
    Ok(Some(found))
}

/// Downloads `update`, telling the UI how far it got about every percent.
async fn fetch(app: &AppHandle, update: Update) -> Result<()> {
    let updates = app.state::<Updates>();
    set(app, |s| s.phase = Phase::Downloading);
    let mut downloaded = 0u64;
    let mut told = Instant::now();
    let mut told_at = 0u64;
    let result = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let step = total.map_or(256 * 1024, |t| (t / 100).max(1));
                if downloaded - told_at >= step || told.elapsed() >= Duration::from_secs(1) {
                    told = Instant::now();
                    told_at = downloaded;
                    set(app, |s| {
                        s.downloaded = downloaded;
                        s.total = total;
                    });
                }
            },
            || {},
        )
        .await;
    match result {
        Ok(bytes) => {
            *updates.ready.lock().unwrap() = Some((update, bytes));
            set(app, |s| {
                s.phase = Phase::Ready;
                s.downloaded = s.total.unwrap_or(downloaded);
            });
            Ok(())
        }
        Err(e) => {
            // Offered again, so the person can try once more.
            *updates.found.lock().unwrap() = Some(update);
            set(app, |s| {
                s.phase = Phase::Available;
                s.downloaded = 0;
            });
            Err(failed(e))
        }
    }
}

/// Checks now and then while the app runs. With automatic downloads off it
/// still looks, so the person hears about new versions.
pub fn start(app: &AppHandle) {
    app.manage(Updates::default());
    if pubkey().is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            // Offline or no release yet: the next round tries again.
            let _ = find(&app, saved_settings(&app).auto_update).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Whether this build can update itself.
#[tauri::command]
pub fn updates_available() -> bool {
    pubkey().is_some()
}

/// Where updates are at.
#[tauri::command]
pub fn update_status(updates: State<'_, Updates>) -> UpdateStatus {
    updates.status.lock().unwrap().clone()
}

/// Checks right away and downloads what it finds. `None` means this is the
/// newest version.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>> {
    if pubkey().is_none() {
        return Err(Error::Invalid(
            "This copy of Novero Mail can't update itself.".into(),
        ));
    }
    find(&app, true).await
}

/// Downloads the update that was found while automatic downloads are off.
#[tauri::command]
pub async fn download_update(app: AppHandle) -> Result<()> {
    let updates = app.state::<Updates>();
    let _one = updates.busy.lock().await;
    let Some(update) = updates.found.lock().unwrap().take() else {
        // Downloaded by a check that ran meanwhile, or never found.
        return match updates.ready.lock().unwrap().is_some() {
            true => Ok(()),
            false => Err(Error::Invalid("There is no update to download.".into())),
        };
    };
    fetch(&app, update).await
}

/// Saves the drafts in message windows by closing them, which saves what
/// was typed (see ComposerWindow.svelte), and waits until they are gone.
async fn close_message_windows(app: &AppHandle) {
    let open = || {
        app.webview_windows()
            .into_iter()
            .filter(|(label, _)| label.starts_with("compose-"))
            .map(|(_, window)| window)
            .collect::<Vec<_>>()
    };
    for window in open() {
        let _ = window.close();
    }
    let started = Instant::now();
    while !open().is_empty() && started.elapsed() < SAVE_DRAFTS {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Installs the downloaded update and restarts. On Windows the installer
/// closes the app, shows its progress and opens the new version.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    let updates = app.state::<Updates>();
    let Some((update, bytes)) = updates.ready.lock().unwrap().take() else {
        return Err(Error::Invalid("There is no update to install.".into()));
    };
    set(&app, |s| s.phase = Phase::Installing);
    close_message_windows(&app).await;
    if let Err(e) = update.install(&bytes) {
        *updates.ready.lock().unwrap() = Some((update, bytes));
        set(&app, |s| s.phase = Phase::Ready);
        return Err(failed(e));
    }
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_newest_releases_manifest() {
        assert!(endpoint().ends_with("/releases/latest/download/latest.json"));
    }
}
