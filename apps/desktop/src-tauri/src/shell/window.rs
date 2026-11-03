//! The app's windows and their title bar. With the custom title bar on (the
//! default), windows open without the Windows frame and the UI draws its own
//! bar with the minimise, maximise and close buttons (lib/TitleBar.svelte).

use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::settings::{Settings, Theme};
use crate::state::AppState;

/// Tells every window the custom title bar was turned on (`true`) or off.
pub const TITLEBAR: &str = "titlebar";

/// Whether windows should draw their own title bar.
pub fn custom_titlebar(app: &AppHandle) -> bool {
    Settings::load(&app.state::<AppState>().store.lock().unwrap()).custom_titlebar
}

/// The main window's size on launch. The folder and message lists keep their
/// own widths (MailView.svelte), so the extra width goes to the reading pane.
const MAIN_SIZE: (f64, f64) = (1480.0, 900.0);
const MAIN_MIN_SIZE: (f64, f64) = (800.0, 500.0);

/// The launch size, shrunk to fit smaller screens with a little room around it.
fn main_size(app: &AppHandle) -> (f64, f64) {
    let Ok(Some(monitor)) = app.primary_monitor() else {
        return MAIN_SIZE;
    };
    let area = monitor.work_area().size.to_logical::<f64>(monitor.scale_factor());
    let fit = |want: f64, room: f64, min: f64| want.min(room * 0.92).max(min).round();
    (
        fit(MAIN_SIZE.0, area.width, MAIN_MIN_SIZE.0),
        fit(MAIN_SIZE.1, area.height, MAIN_MIN_SIZE.1),
    )
}

/// Opens the main window. It is made here rather than in tauri.conf.json so
/// it starts with the right frame and doesn't flash the other one.
pub fn open_main(app: &AppHandle) -> tauri::Result<()> {
    let (width, height) = main_size(app);
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Novero Mail")
        .inner_size(width, height)
        .min_inner_size(MAIN_MIN_SIZE.0, MAIN_MIN_SIZE.1)
        .center()
        .decorations(!custom_titlebar(app))
        .build()
        .map(|window| paint_background(app, &window))?;
    Ok(())
}

/// Gives the window and its webview the theme's background (--bg in
/// app.css), so a fast resize shows that instead of white before the page
/// catches up. The page keeps it in step when the theme changes (lib/theme.ts).
pub fn paint_background(app: &AppHandle, window: &WebviewWindow) {
    let theme = Settings::load(&app.state::<AppState>().store.lock().unwrap()).theme;
    let dark = match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => window.theme().is_ok_and(|t| t == tauri::Theme::Dark),
    };
    let color = if dark {
        Color(0x11, 0x11, 0x11, 0xff)
    } else {
        Color(0xff, 0xff, 0xff, 0xff)
    };
    let _ = window.set_background_color(Some(color));
}

/// Switches every open window to the title bar just picked in Settings.
pub fn set_custom_titlebar(app: &AppHandle, on: bool) {
    for window in app.webview_windows().values() {
        let _ = window.set_decorations(!on);
    }
    let _ = app.emit(TITLEBAR, on);
}
