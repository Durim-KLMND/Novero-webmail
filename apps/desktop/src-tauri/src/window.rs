//! The app's windows and their title bar. With the custom title bar on (the
//! default), windows open without the Windows frame and the UI draws its own
//! bar with the minimise, maximise and close buttons (lib/TitleBar.svelte).

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::settings::Settings;
use crate::state::AppState;

/// Tells every window the custom title bar was turned on (`true`) or off.
pub const TITLEBAR: &str = "titlebar";

/// Whether windows should draw their own title bar.
pub fn custom_titlebar(app: &AppHandle) -> bool {
    Settings::load(&app.state::<AppState>().store.lock().unwrap()).custom_titlebar
}

/// Opens the main window. It is made here rather than in tauri.conf.json so
/// it starts with the right frame and doesn't flash the other one.
pub fn open_main(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Novero Mail")
        .inner_size(1280.0, 800.0)
        .min_inner_size(800.0, 500.0)
        .decorations(!custom_titlebar(app))
        .build()?;
    Ok(())
}

/// Switches every open window to the title bar just picked in Settings.
pub fn set_custom_titlebar(app: &AppHandle, on: bool) {
    for window in app.webview_windows().values() {
        let _ = window.set_decorations(!on);
    }
    let _ = app.emit(TITLEBAR, on);
}
