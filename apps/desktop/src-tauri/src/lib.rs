//! Tauri host: registers commands and forwards core events to the UI.

mod calendar;
mod commands;
mod compose;
mod crash;
mod files;
mod invites;
mod links;
mod reader;
mod rules;
mod settings;
mod snooze;
mod state;
mod updates;
mod window;

use std::time::Duration;

use tauri::Manager;

/// How often every folder of every account is synced. The inbox also gets
/// new mail right away through IDLE when the server supports it.
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        // Starting the app again (say, from a mailto: link) focuses the open
        // window instead, handing it the link.
        .plugin(tauri_plugin_single_instance::init(
            |app, args, _cwd| match links::from_args(args) {
                Some(link) => links::opened(app, link),
                None => links::show(app),
            },
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init());
    if let Some(updater) = updates::plugin() {
        builder = builder.plugin(updater);
    }
    builder
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            crash::install(crash::folder(&dir));
            let state = state::AppState::open(&dir)?;
            app.manage(state);
            window::open_main(app.handle())?;
            app.manage(links::PendingLink::default());
            app.manage(files::Drops::default());
            if let Some(link) = links::from_args(std::env::args()) {
                links::opened(app.handle(), link);
            }
            updates::start(app.handle());
            calendar::start_reminders(app.handle());
            compose::start_scheduler(app.handle());
            snooze::start(app.handle());

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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(drop) = event {
                files::window_event(window, drop);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::discover_settings,
            commands::add_account,
            commands::sign_in_again,
            commands::update_account_servers,
            commands::cancel_sign_in,
            commands::list_accounts,
            commands::remove_account,
            commands::list_folders,
            commands::list_messages,
            commands::conversation,
            commands::open_message,
            commands::sync_account,
            commands::pending_changes,
            commands::set_flag,
            commands::move_messages,
            commands::archive_messages,
            commands::delete_messages,
            compose::new_draft,
            compose::mailto_draft,
            compose::reply_draft,
            compose::forward_draft,
            compose::open_draft,
            compose::save_draft,
            compose::attach_files,
            compose::remove_attachment,
            compose::discard_draft,
            compose::send_draft,
            compose::suggest_contacts,
            files::attach_dropped,
            files::dropped_picture,
            files::pick_picture,
            compose::hide_contact,
            compose::get_draft,
            compose::open_composer_window,
            calendar::list_calendars,
            calendar::save_calendar,
            calendar::delete_calendar,
            calendar::calendar_occurrences,
            calendar::get_event,
            calendar::save_event,
            calendar::delete_event,
            calendar::skip_occurrence,
            snooze::snooze_messages,
            snooze::unsnooze_messages,
            snooze::snoozed_count,
            snooze::snoozed_until,
            invites::message_invite,
            invites::answer_invite,
            rules::list_rules,
            rules::save_rule,
            rules::delete_rule,
            rules::order_rules,
            rules::run_rules,
            compose::list_outbox,
            compose::outbox_status,
            compose::retry_outbox,
            compose::delete_outbox,
            compose::edit_outbox,
            compose::send_outbox_now,
            settings::get_settings,
            settings::save_settings,
            settings::list_signatures,
            settings::save_signature,
            settings::delete_signature,
            settings::set_default_signatures,
            settings::list_templates,
            settings::save_template,
            settings::delete_template,
            links::take_mailto,
            links::open_default_apps,
            settings::test_notification,
            settings::open_notification_settings,
            crash::crash_report,
            crash::discard_crash_report,
            crash::send_crash_report,
            updates::updates_available,
            updates::check_for_update,
            updates::ready_update,
            updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Novero Mail");
}
