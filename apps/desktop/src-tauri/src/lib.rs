//! Tauri host: registers commands and forwards core events to the UI.

mod commands;
mod state;

use std::time::Duration;

use tauri::Manager;

/// How often every account is synced in the background. IDLE push replaces this in phase 2.
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            let state = state::AppState::open(&dir)?;
            app.manage(state);

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(SYNC_INTERVAL);
                loop {
                    ticker.tick().await;
                    state::sync_all(&handle);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::provider_preset,
            commands::add_account,
            commands::list_accounts,
            commands::remove_account,
            commands::list_folders,
            commands::list_messages,
            commands::open_message,
            commands::sync_account,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Novero Mail");
}
