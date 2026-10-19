//! `mailto:` links. The installer registers Novero Mail as an email app with
//! Windows; once the person picks it as the default, Windows starts it with
//! the clicked link as its only argument. A second start hands the link to
//! the window that is already open (see the single-instance plugin in lib.rs).

use std::sync::Mutex;

use mail_core::{Error, Result};
use tauri::{AppHandle, Emitter, Manager, State};

/// Tells the UI a link is waiting; it picks it up with `take_mailto`.
pub const MAILTO: &str = "mailto";

/// The link the app was started or reached with, until the UI opens it.
#[derive(Default)]
pub struct PendingLink(Mutex<Option<String>>);

/// The `mailto:` link among command-line arguments (the program comes first).
pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter().skip(1).find(|a| {
        a.get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("mailto:"))
    })
}

/// Keeps `link` for the UI and brings the window forward.
pub fn opened(app: &AppHandle, link: String) {
    *app.state::<PendingLink>().0.lock().unwrap() = Some(link);
    let _ = app.emit(MAILTO, ());
    show(app);
}

/// Brings the main window to the front, as when the app is started again.
pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
pub fn take_mailto(pending: State<'_, PendingLink>) -> Option<String> {
    pending.0.lock().unwrap().take()
}

/// Opens the Windows settings page where the default email app is chosen.
/// Windows doesn't let apps make themselves the default.
#[tauri::command]
pub fn open_default_apps(app: AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    if !cfg!(windows) {
        return Err(Error::Invalid(
            "Choosing the default email app is only available on Windows.".into(),
        ));
    }
    let product = app.package_info().name.replace(' ', "%20");
    app.opener()
        .open_url(
            format!("ms-settings:defaultapps?registeredAppUser={product}"),
            None::<&str>,
        )
        .map_err(|e| Error::Invalid(format!("could not open Windows settings: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_link_argument() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            from_args(args(&["novero.exe", "MAILTO:ana@example.com"])).as_deref(),
            Some("MAILTO:ana@example.com")
        );
        assert_eq!(from_args(args(&["mailto:x@example.com"])), None);
        assert_eq!(from_args(args(&["novero.exe", "--flag"])), None);
    }
}
