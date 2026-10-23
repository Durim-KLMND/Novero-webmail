//! Snoozing: hiding messages until a set time, and bringing them back
//! unread with a notification when it comes.

use std::time::Duration;

use mail_core::{AccountId, Event, FlagKind, Op, Result};
use mail_store::Woken;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::commands::change;
use crate::mail::compose::now;
use crate::settings::Settings;
use crate::state::{AppState, emit};

/// How often snoozed messages are checked.
const WAKE_CHECK: Duration = Duration::from_secs(30);

fn changed(app: &AppHandle, state: &AppState, ids: &[i64]) {
    let mut accounts: Vec<AccountId> = Vec::new();
    if let Ok(found) = state.store.lock().unwrap().locate(ids) {
        for m in found {
            if !accounts.contains(&m.account_id) {
                accounts.push(m.account_id);
            }
        }
    }
    for account in accounts {
        emit(app, Event::MailboxChanged { account });
    }
}

/// Hides messages until `until` (UTC seconds).
#[tauri::command]
pub fn snooze_messages(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    until: i64,
) -> Result<()> {
    if until <= now() {
        return Err(mail_core::Error::Invalid(
            "Pick a time in the future.".into(),
        ));
    }
    let store = state.store.lock().unwrap();
    crate::plan::require(&store, crate::plan::Feature::Snooze)?;
    store.snooze(&ids, until)?;
    drop(store);
    changed(&app, &state, &ids);
    Ok(())
}

/// Brings snoozed messages back now.
#[tauri::command]
pub fn unsnooze_messages(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> Result<()> {
    state.store.lock().unwrap().unsnooze(&ids)?;
    changed(&app, &state, &ids);
    Ok(())
}

#[tauri::command]
pub fn snoozed_count(state: State<'_, AppState>) -> Result<u32> {
    state.store.lock().unwrap().snoozed_count()
}

/// When a message comes back, if it is snoozed.
#[tauri::command]
pub fn snoozed_until(state: State<'_, AppState>, id: i64) -> Result<Option<i64>> {
    state.store.lock().unwrap().snoozed_until(id)
}

/// Brings snoozed messages back as their time comes: unread, and announced.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(WAKE_CHECK);
        loop {
            ticker.tick().await;
            let state = app.state::<AppState>();
            let woken = state
                .store
                .lock()
                .unwrap()
                .wake_snoozed(now())
                .unwrap_or_default();
            if woken.is_empty() {
                continue;
            }
            let ids: Vec<i64> = woken.iter().map(|w| w.id).collect();
            // `change` announces the accounts whose mail changed.
            let _ = change(&app, &state, &ids, |m| Op::SetFlag {
                folder: m.folder_name.clone(),
                uid_validity: m.uid_validity,
                uids: Vec::new(),
                flag: FlagKind::Seen,
                value: false,
            });
            if Settings::load(&state.store.lock().unwrap()).notifications {
                let (title, body) = woken_text(&woken);
                let _ = app.notification().builder().title(title).body(body).show();
            }
        }
    });
}

/// The notification for mail back from snooze.
fn woken_text(woken: &[Woken]) -> (String, String) {
    let subject = |w: &Woken| {
        w.subject
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("(no subject)")
            .to_owned()
    };
    match woken {
        [one] => ("Back from snooze".into(), subject(one)),
        many => {
            let mut body: Vec<String> = many.iter().take(3).map(subject).collect();
            if many.len() > 3 {
                body.push(format!("and {} more", many.len() - 3));
            }
            (
                format!("{} messages back from snooze", many.len()),
                body.join("\n"),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn woken(subject: &str) -> Woken {
        Woken {
            id: 1,
            account: AccountId(1),
            from: None,
            subject: Some(subject.into()),
        }
    }

    #[test]
    fn notifications_name_what_came_back() {
        assert_eq!(
            woken_text(&[woken("Invoice")]),
            ("Back from snooze".into(), "Invoice".into())
        );
        let many: Vec<Woken> = ["a", "b", "c", "d", " "].into_iter().map(woken).collect();
        assert_eq!(
            woken_text(&many),
            (
                "5 messages back from snooze".into(),
                "a\nb\nc\nand 2 more".into()
            )
        );
    }
}
