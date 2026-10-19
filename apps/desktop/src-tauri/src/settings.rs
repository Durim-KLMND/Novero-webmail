//! App preferences, kept in the database so they survive reinstalls of the
//! UI and are read by the background tasks too.

use mail_core::{AccountId, Result};
use mail_store::Store;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follows the Windows setting.
    #[default]
    System,
    Light,
    Dark,
}

/// Which releases updates come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    #[default]
    Stable,
    /// Test versions before they are released, and stable releases too.
    Beta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    /// Group messages into conversations in the message list.
    pub conversations: bool,
    /// Show a Windows notification when new mail arrives.
    pub notifications: bool,
    /// Look for updates in the background and download them.
    pub auto_update: bool,
    pub update_channel: Channel,
    /// After a crash, offer to send a report. Nothing is sent without asking.
    pub crash_reports: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            conversations: true,
            notifications: true,
            auto_update: true,
            update_channel: Channel::Stable,
            crash_reports: true,
        }
    }
}

const KEY: &str = "app";

impl Settings {
    /// The saved settings; anything missing or unreadable takes its default.
    pub fn load(store: &Store) -> Settings {
        store
            .setting(KEY)
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn save(&self, store: &Store) -> Result<()> {
        let value = serde_json::to_value(self).expect("settings serialize");
        store.set_setting(KEY, &value)
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    Settings::load(&state.store.lock().unwrap())
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    settings.save(&state.store.lock().unwrap())?;
    Ok(settings)
}

/// Sets the signature added to the account's new messages.
#[tauri::command]
pub fn set_signature(
    state: State<'_, AppState>,
    account: AccountId,
    signature: String,
) -> Result<()> {
    // Trailing blank lines would only push the quote further down.
    let signature = signature.trim_end().replace("\r\n", "\n");
    state
        .store
        .lock()
        .unwrap()
        .set_signature(account, &signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_take_their_defaults() {
        let dir = std::env::temp_dir();
        let store = Store::open_in_memory(&dir).unwrap();
        assert_eq!(Settings::load(&store), Settings::default());
        // Older or partial values keep the rest at their defaults.
        store
            .set_setting(KEY, &serde_json::json!({ "theme": "dark" }))
            .unwrap();
        let loaded = Settings::load(&store);
        assert_eq!(loaded.theme, Theme::Dark);
        assert!(loaded.conversations && loaded.notifications && loaded.auto_update);
        assert_eq!(loaded.update_channel, Channel::Stable);
        let off = Settings {
            notifications: false,
            ..loaded
        };
        off.save(&store).unwrap();
        assert_eq!(Settings::load(&store), off);
    }
}
