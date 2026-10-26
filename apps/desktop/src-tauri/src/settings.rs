//! App preferences, kept in the database so they survive reinstalls of the
//! UI and are read by the background tasks too.

use mail_core::{AccountId, Result, Signature, Template};
use mail_store::Store;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

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
    /// After a crash, offer to send a report. Nothing is sent without asking.
    pub crash_reports: bool,
    /// Windows draw Novero Mail's own title bar instead of the Windows one.
    pub custom_titlebar: bool,
    /// The close button hides the main window to the system tray, where the
    /// app keeps checking mail. Off, it quits Novero Mail.
    pub close_to_tray: bool,
    /// The taskbar icon shows how many inbox messages are unread.
    pub unread_badge: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            conversations: true,
            notifications: true,
            auto_update: true,
            crash_reports: true,
            custom_titlebar: true,
            close_to_tray: true,
            unread_badge: true,
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
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings> {
    let before = {
        let store = state.store.lock().unwrap();
        let before = Settings::load(&store);
        settings.save(&store)?;
        before
    };
    if before.custom_titlebar != settings.custom_titlebar {
        crate::shell::window::set_custom_titlebar(&app, settings.custom_titlebar);
    }
    if before.unread_badge != settings.unread_badge {
        crate::shell::badge::changed(&app);
    }
    Ok(settings)
}

/// Shows a sample new-mail notification (see `state::test_notification`).
#[tauri::command]
pub async fn test_notification(app: AppHandle, several: bool, delay: u64) -> Result<()> {
    crate::state::test_notification(&app, several, delay).await
}

/// Opens the Windows settings page for notifications, where they can be
/// allowed, silenced or changed.
#[tauri::command]
pub fn open_notification_settings(app: AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    if !cfg!(windows) {
        return Err(mail_core::Error::Invalid(
            "Notification settings can only be opened on Windows.".into(),
        ));
    }
    app.opener()
        .open_url("ms-settings:notifications", None::<&str>)
        .map_err(|e| mail_core::Error::Invalid(format!("could not open Windows settings: {e}")))
}

/// Every account's signatures.
#[tauri::command]
pub fn list_signatures(state: State<'_, AppState>) -> Result<Vec<Signature>> {
    state.store.lock().unwrap().signatures()
}

/// Adds or changes a signature: the plain text, and the formatted version
/// when it has formatting (empty otherwise).
#[tauri::command]
pub fn save_signature(state: State<'_, AppState>, signature: Signature) -> Result<Signature> {
    let store = state.store.lock().unwrap();
    let signature = Signature {
        name: named(&signature.name, "Signature"),
        // Trailing blank lines would only push the quote further down.
        text: signature.text.trim_end().replace("\r\n", "\n"),
        html: cleaned(&signature.html),
        ..signature
    };
    store.save_signature(&signature)
}

#[tauri::command]
pub fn delete_signature(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().delete_signature(id)
}

/// Picks the signatures an account's new messages and replies start with.
#[tauri::command]
pub fn set_default_signatures(
    state: State<'_, AppState>,
    account: AccountId,
    new: Option<i64>,
    reply: Option<i64>,
) -> Result<()> {
    state
        .store
        .lock()
        .unwrap()
        .set_default_signatures(account, new, reply)
}

#[tauri::command]
pub fn list_templates(state: State<'_, AppState>) -> Result<Vec<Template>> {
    state.store.lock().unwrap().templates()
}

/// Adds or changes a template.
#[tauri::command]
pub fn save_template(state: State<'_, AppState>, template: Template) -> Result<Template> {
    let store = state.store.lock().unwrap();
    let template = Template {
        name: named(&template.name, "Template"),
        subject: template.subject.trim().to_owned(),
        text: template.text.trim_end().replace("\r\n", "\n"),
        html: cleaned(&template.html),
        ..template
    };
    store.save_template(&template)
}

#[tauri::command]
pub fn delete_template(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().delete_template(id)
}

/// The name typed, or `fallback` when none was.
fn named(name: &str, fallback: &str) -> String {
    match name.trim() {
        "" => fallback.to_owned(),
        name => name.to_owned(),
    }
}

/// Formatted text made safe to keep and send; "" stays "".
fn cleaned(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    mail_mime::clean_compose_html(html).trim().to_owned()
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
        assert!(loaded.custom_titlebar && loaded.close_to_tray && loaded.unread_badge);
        let off = Settings {
            notifications: false,
            ..loaded
        };
        off.save(&store).unwrap();
        assert_eq!(Settings::load(&store), off);
    }
}
