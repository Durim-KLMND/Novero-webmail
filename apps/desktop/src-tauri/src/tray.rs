//! The icon in the system tray (the notification area). It is there while
//! the app runs, so closing the main window can hide it instead of quitting
//! (Settings > General) while mail keeps being checked.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, CloseRequestApi, Manager, Window};
use tauri_plugin_notification::NotificationExt;

use crate::settings::Settings;
use crate::state::AppState;

const ID: &str = "main";
const NAME: &str = "Novero Mail";
/// Remembers that the "still running" notice was shown, so it comes once.
const HINT_SHOWN: &str = "tray-hint-shown";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Novero Mail", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Check for new mail", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Novero Mail", true, None::<&str>)?;
    let line = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &check, &line, &quit])?;
    let mut tray = TrayIconBuilder::with_id(ID)
        .tooltip(NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => crate::links::show(app),
            "check" => crate::state::sync_all(app),
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::links::show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Puts the unread count in the tray icon's tooltip.
pub fn set_unread(app: &AppHandle, count: u32) {
    if let Some(tray) = app.tray_by_id(ID) {
        let tip = match crate::badge::label(count) {
            Some(n) => format!("{NAME}: {n} unread"),
            None => NAME.to_owned(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// The close button of the main window: hides it to the tray, or quits,
/// as chosen in Settings. Message windows close as usual.
pub fn close_requested(window: &Window, api: &CloseRequestApi) {
    if window.label() != "main" {
        return;
    }
    api.prevent_close();
    let app = window.app_handle();
    let (to_tray, hint_shown) = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let settings = Settings::load(&store);
        let shown = store.setting(HINT_SHOWN).ok().flatten().is_some();
        if settings.close_to_tray && !shown {
            let _ = store.set_setting(HINT_SHOWN, &serde_json::Value::Bool(true));
        }
        (settings.close_to_tray, shown)
    };
    if !to_tray {
        quit_app(app);
        return;
    }
    let _ = window.hide();
    if !hint_shown {
        let _ = app
            .notification()
            .builder()
            .title("Novero Mail is still running")
            .body("It keeps checking your mail from the system tray. To quit, right-click its icon there, or change what closing does in Settings.")
            .show();
    }
}

/// Quits Novero Mail, after message windows have saved their drafts.
pub fn quit_app(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::updates::close_message_windows(&app).await;
        app.exit(0);
    });
}
