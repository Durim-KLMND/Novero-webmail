//! Tauri host: registers commands and forwards core events to the UI.

/// Version of the Rust core, shown in the UI as a smoke test of the command bridge.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_version])
        .run(tauri::generate_context!())
        .expect("error while running the desktop app");
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_the_package() {
        assert_eq!(super::app_version(), "0.1.0");
    }
}
