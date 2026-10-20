//! The calendar: commands that read and edit calendars and events, and the
//! background task that shows reminders.

use std::time::Duration;

use mail_core::Result;
use mail_core::calendar::{Calendar, Event, Occurrence};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;

/// Tells every window the calendar changed, so open views reload.
pub const CALENDAR_CHANGED: &str = "calendar-changed";

/// How often reminders are looked for.
const REMINDER_CHECK: Duration = Duration::from_secs(30);

pub(crate) fn changed(app: &AppHandle) {
    let _ = app.emit(CALENDAR_CHANGED, ());
}

#[tauri::command]
pub fn list_calendars(state: State<'_, AppState>) -> Result<Vec<Calendar>> {
    state.store.lock().unwrap().list_calendars()
}

/// Saves a calendar and returns it as stored (with its id when new).
#[tauri::command]
pub fn save_calendar(
    app: AppHandle,
    state: State<'_, AppState>,
    calendar: Calendar,
) -> Result<Calendar> {
    let saved = state.store.lock().unwrap().save_calendar(&calendar)?;
    changed(&app);
    Ok(saved)
}

/// Deletes a calendar kept on this computer and its events. The last one
/// can't be deleted.
#[tauri::command]
pub fn delete_calendar(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().delete_calendar(id)?;
    changed(&app);
    Ok(())
}

/// Everything happening from `from` to `to` (UTC seconds) in shown calendars.
#[tauri::command]
pub fn calendar_occurrences(
    state: State<'_, AppState>,
    from: i64,
    to: i64,
) -> Result<Vec<Occurrence>> {
    state.store.lock().unwrap().occurrences(from, to)
}

#[tauri::command]
pub fn get_event(state: State<'_, AppState>, id: i64) -> Result<Option<Event>> {
    state.store.lock().unwrap().event(id)
}

/// Saves an event and returns it as stored. A timed event without a time zone
/// gets the computer's, so repeats keep their wall-clock time.
#[tauri::command]
pub fn save_event(app: AppHandle, state: State<'_, AppState>, mut event: Event) -> Result<Event> {
    if event.all_day {
        event.tz = None;
    } else if event.tz.is_none() {
        event.tz = mail_calendar::system_zone();
    }
    let saved = state.store.lock().unwrap().edit_event(&event)?;
    changed(&app);
    crate::calsync::sync_soon(&app);
    Ok(saved)
}

#[tauri::command]
pub fn delete_event(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().remove_event(id)?;
    changed(&app);
    crate::calsync::sync_soon(&app);
    Ok(())
}

/// Takes one occurrence, the one starting at `starts`, out of a repeating
/// event. An occurrence of an online series kept on its own is deleted, so
/// there is no event left to return.
#[tauri::command]
pub fn skip_occurrence(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    starts: i64,
) -> Result<Option<Event>> {
    let saved = state.store.lock().unwrap().skip_change(id, starts)?;
    changed(&app);
    crate::calsync::sync_soon(&app);
    Ok(saved)
}

/// Shows a notification for each reminder as it comes due.
pub fn start_reminders(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(REMINDER_CHECK);
        loop {
            ticker.tick().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or_default();
            let due = {
                let state = app.state::<AppState>();
                let mut store = state.store.lock().unwrap();
                store.due_reminders(now).unwrap_or_default()
            };
            for reminder in due {
                let (title, body) = reminder_text(&reminder.event, reminder.starts, now);
                let _ = app.notification().builder().title(title).body(body).show();
            }
        }
    });
}

/// The title and text of a reminder: the event, when it starts and where.
fn reminder_text(event: &Event, starts: i64, now: i64) -> (String, String) {
    let title = if event.title.trim().is_empty() {
        "(no title)".to_owned()
    } else {
        event.title.trim().to_owned()
    };
    let minutes = (starts - now + 59) / 60;
    let when = if event.all_day {
        "Today".to_owned()
    } else {
        match minutes {
            ..=0 => "Starting now".to_owned(),
            1 => "In 1 minute".to_owned(),
            m if m < 60 => format!("In {m} minutes"),
            m if m < 120 => "In 1 hour".to_owned(),
            m if m < 24 * 60 => format!("In {} hours", m / 60),
            m => match m / (24 * 60) {
                1 => "Tomorrow".to_owned(),
                d => format!("In {d} days"),
            },
        }
    };
    let body = match event.location.trim() {
        "" => when,
        place => format!("{when} · {place}"),
    };
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str, location: &str) -> Event {
        Event {
            id: 1,
            calendar: 1,
            uid: "u".into(),
            title: title.into(),
            location: location.into(),
            notes: String::new(),
            starts: 0,
            ends: 3600,
            all_day: false,
            tz: None,
            rrule: None,
            exdates: vec![],
            organizer: None,
            attendees: vec![],
            response: None,
            reminder: Some(15),
            sequence: 0,
            cancelled: false,
            series: None,
        }
    }

    #[test]
    fn reminders_say_how_soon_and_where() {
        let e = event("Stand-up", "Room 2");
        assert_eq!(
            reminder_text(&e, 900, 0),
            ("Stand-up".into(), "In 15 minutes · Room 2".into())
        );
        assert_eq!(reminder_text(&e, 0, 0).1, "Starting now · Room 2");
        assert_eq!(
            reminder_text(&event(" ", ""), 7200, 0),
            ("(no title)".into(), "In 2 hours".into())
        );
        assert_eq!(reminder_text(&event("x", ""), 86_400, 0).1, "Tomorrow");
    }
}
