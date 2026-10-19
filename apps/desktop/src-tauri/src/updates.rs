//! Updates from the project's GitHub releases.
//!
//! Each release publishes a `latest.json` manifest next to the installer. The
//! stable channel reads the one of the newest stable release; the beta channel
//! reads the copy on the `beta-channel` release, which every release, test or
//! stable, replaces. Installers are checked against the update key built into
//! the app, so a manifest alone can't make it install anything else.
//!
//! Updates are looked for in the background and downloaded before the person
//! is told, so "Restart to update" only has to run the installer.

use std::sync::Mutex;
use std::time::Duration;

use mail_core::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::settings::{Channel, Settings};
use crate::state::AppState;

/// The public half of the update signing key (see docs/releasing.md). Builds
/// without one can't update themselves.
const PUBKEY: Option<&str> = option_env!("NOVERO_UPDATER_PUBKEY");

const RELEASES: &str = "https://github.com/Durim-KLMND/Novero-webmail/releases";

/// The first look waits until start-up work is done.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Sent to the UI when an update has been downloaded.
pub const UPDATE_READY: &str = "update-ready";

fn pubkey() -> Option<&'static str> {
    PUBKEY.map(str::trim).filter(|k| !k.is_empty())
}

/// The updater plugin, in builds that have an update key.
pub fn plugin<R: Runtime>() -> Option<tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config>> {
    pubkey().map(|key| tauri_plugin_updater::Builder::new().pubkey(key).build())
}

/// Where a channel's manifest is. `NOVERO_UPDATE_ENDPOINT` replaces it, for
/// trying out releases from a local server.
fn endpoint(channel: Channel) -> String {
    if let Ok(url) = std::env::var("NOVERO_UPDATE_ENDPOINT") {
        return url;
    }
    match channel {
        Channel::Stable => format!("{RELEASES}/latest/download/latest.json"),
        Channel::Beta => format!("{RELEASES}/download/beta-channel/latest.json"),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// The release notes, when the release has any.
    pub notes: Option<String>,
}

/// The downloaded update waiting to be installed.
#[derive(Default)]
pub struct Updates {
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// One check at a time; a second one waits and then finds it done.
    checking: tokio::sync::Mutex<()>,
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
        E::ReleaseNotFound => Error::Network("no release was found on this channel yet".into()),
        e => Error::Network(e.to_string()),
    }
}

fn saved_channel(app: &AppHandle) -> Channel {
    let state = app.state::<AppState>();
    Settings::load(&state.store.lock().unwrap()).update_channel
}

/// Looks for a newer release on `channel` and downloads it.
async fn find(app: &AppHandle, channel: Channel) -> Result<Option<UpdateInfo>> {
    let updates = app.state::<Updates>();
    let _one = updates.checking.lock().await;
    let url = Url::parse(&endpoint(channel)).map_err(|e| Error::Invalid(e.to_string()))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(failed)?
        .build()
        .map_err(failed)?;
    let Some(update) = updater.check().await.map_err(failed)? else {
        return Ok(None);
    };
    if let Some((ready, _)) = &*updates.ready.lock().unwrap()
        && ready.version == update.version
    {
        return Ok(Some(info(ready)));
    }
    let bytes = update.download(|_, _| {}, || {}).await.map_err(failed)?;
    let found = info(&update);
    *updates.ready.lock().unwrap() = Some((update, bytes));
    let _ = app.emit(UPDATE_READY, &found);
    Ok(Some(found))
}

/// Checks now and then while the app runs, when the person wants that.
pub fn start(app: &AppHandle) {
    app.manage(Updates::default());
    if pubkey().is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let wanted = {
                let state = app.state::<AppState>();
                Settings::load(&state.store.lock().unwrap()).auto_update
            };
            if wanted {
                // Offline or no release yet: the next round tries again.
                let _ = find(&app, saved_channel(&app)).await;
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Whether this build can update itself.
#[tauri::command]
pub fn updates_available() -> bool {
    pubkey().is_some()
}

/// Checks right away, on `channel` or else the saved one. `None` means this
/// is the newest version.
#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    channel: Option<Channel>,
) -> Result<Option<UpdateInfo>> {
    if pubkey().is_none() {
        return Err(Error::Invalid(
            "This copy of Novero Mail can't update itself.".into(),
        ));
    }
    let channel = channel.unwrap_or_else(|| saved_channel(&app));
    find(&app, channel).await
}

/// The update downloaded in the background, if there is one.
#[tauri::command]
pub fn ready_update(updates: State<'_, Updates>) -> Option<UpdateInfo> {
    updates.ready.lock().unwrap().as_ref().map(|(u, _)| info(u))
}

/// Installs the downloaded update and restarts. On Windows the installer
/// closes the app, shows its progress and opens the new version.
#[tauri::command]
pub fn install_update(app: AppHandle, updates: State<'_, Updates>) -> Result<()> {
    let Some((update, bytes)) = updates.ready.lock().unwrap().take() else {
        return Err(Error::Invalid("There is no update to install.".into()));
    };
    update.install(bytes).map_err(failed)?;
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_read_their_own_manifest() {
        assert!(endpoint(Channel::Stable).ends_with("/releases/latest/download/latest.json"));
        assert!(endpoint(Channel::Beta).ends_with("/releases/download/beta-channel/latest.json"));
    }
}
