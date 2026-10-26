//! Tauri host: registers commands and forwards core events to the UI.

mod calendar;
mod commands;
mod mail;
mod settings;
mod shell;
mod state;
mod todo;

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
            |app, args, _cwd| match mail::links::from_args(args) {
                Some(link) => mail::links::opened(app, link),
                None => mail::links::show(app),
            },
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init());
    if let Some(updater) = shell::updates::plugin() {
        builder = builder.plugin(updater);
    }
    builder
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            shell::crash::install(shell::crash::folder(&dir));
            let state = state::AppState::open(&dir)?;
            app.manage(state);
            app.manage(calendar::sync::CalendarSync::default());
            shell::window::open_main(app.handle())?;
            shell::tray::create(app.handle())?;
            shell::badge::start(app.handle());
            app.manage(mail::links::PendingLink::default());
            app.manage(mail::files::Drops::default());
            if let Some(link) = mail::links::from_args(std::env::args()) {
                mail::links::opened(app.handle(), link);
            }
            shell::updates::start(app.handle());
            calendar::events::start_reminders(app.handle());
            calendar::sync::start(app.handle());
            mail::compose::start_scheduler(app.handle());
            mail::snooze::start(app.handle());

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
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::DragDrop(drop) => mail::files::window_event(window, drop),
            tauri::WindowEvent::CloseRequested { api, .. } => {
                shell::tray::close_requested(window, api)
            }
            _ => {}
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
            mail::cleanup::cleanup_report,
            mail::cleanup::cleanup_items,
            mail::cleanup::set_sender_category,
            mail::cleanup::set_message_category,
            mail::cleanup::junk_messages,
            mail::cleanup::check_list_headers,
            mail::folders::create_folder,
            mail::folders::rename_folder,
            mail::folders::delete_folder,
            mail::folders::mark_folder_read,
            mail::folders::empty_folder,
            mail::compose::new_draft,
            mail::compose::mailto_draft,
            mail::compose::reply_draft,
            mail::compose::forward_draft,
            mail::compose::open_draft,
            mail::compose::save_draft,
            mail::compose::attach_files,
            mail::compose::remove_attachment,
            mail::compose::discard_draft,
            mail::compose::send_draft,
            mail::compose::suggest_contacts,
            mail::files::attach_dropped,
            mail::files::dropped_picture,
            mail::files::pick_picture,
            mail::compose::hide_contact,
            mail::compose::get_draft,
            mail::compose::open_composer_window,
            calendar::events::list_calendars,
            calendar::events::save_calendar,
            calendar::events::delete_calendar,
            calendar::events::calendar_occurrences,
            calendar::events::get_event,
            calendar::events::save_event,
            calendar::events::delete_event,
            calendar::events::skip_occurrence,
            calendar::sync::list_calendar_accounts,
            calendar::sync::connect_calendar,
            calendar::sync::disconnect_calendar,
            calendar::sync::sync_calendars,
            mail::snooze::snooze_messages,
            mail::snooze::unsnooze_messages,
            mail::snooze::snoozed_count,
            mail::snooze::snoozed_until,
            mail::attachments::attachment_contents,
            mail::attachments::save_attachment,
            mail::invites::message_invite,
            mail::invites::answer_invite,
            mail::rules::list_rules,
            mail::rules::save_rule,
            mail::rules::delete_rule,
            mail::rules::order_rules,
            mail::rules::run_rules,
            mail::compose::list_outbox,
            mail::compose::outbox_status,
            mail::compose::retry_outbox,
            mail::compose::delete_outbox,
            mail::compose::edit_outbox,
            mail::compose::send_outbox_now,
            settings::get_settings,
            settings::save_settings,
            settings::list_signatures,
            settings::save_signature,
            settings::delete_signature,
            settings::set_default_signatures,
            settings::list_templates,
            settings::save_template,
            settings::delete_template,
            todo::list_notes,
            todo::save_note,
            todo::delete_note,
            todo::task_board,
            todo::save_task_column,
            todo::delete_task_column,
            todo::move_task_column,
            todo::save_task,
            todo::move_task,
            todo::delete_task,
            todo::clear_done_tasks,
            mail::links::take_mailto,
            mail::links::open_default_apps,
            settings::test_notification,
            settings::open_notification_settings,
            shell::crash::crash_report,
            shell::crash::discard_crash_report,
            shell::crash::send_crash_report,
            shell::updates::updates_available,
            shell::updates::check_for_update,
            shell::updates::update_status,
            shell::updates::download_update,
            shell::updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Novero Mail");
}
