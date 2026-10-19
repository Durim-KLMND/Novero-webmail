//! Tauri host: registers commands and forwards core events to the UI.

mod commands;
mod compose;
mod state;

use std::time::Duration;

use tauri::Manager;

/// How often every folder of every account is synced. The inbox also gets
/// new mail right away through IDLE when the server supports it.
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
            state::start_all(&handle);
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(SYNC_INTERVAL);
                ticker.tick().await;
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
            commands::pending_changes,
            commands::set_flag,
            commands::move_messages,
            commands::archive_messages,
            commands::delete_messages,
            compose::new_draft,
            compose::reply_draft,
            compose::forward_draft,
            compose::open_draft,
            compose::save_draft,
            compose::attach_files,
            compose::remove_attachment,
            compose::discard_draft,
            compose::send_draft,
            compose::list_outbox,
            compose::outbox_status,
            compose::retry_outbox,
            compose::delete_outbox,
            compose::edit_outbox,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Novero Mail");
}
