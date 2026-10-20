//! Calendars and events kept on this computer.

use mail_core::calendar::{Calendar, Event, Occurrence, Response};
use mail_core::{AccountId, Error, Result};
use rusqlite::{OptionalExtension, Row, params};

use crate::storage;
use crate::store::Store;

const CALENDAR_SELECT: &str = "SELECT id, name, color, account_id, visible FROM calendars";

const EVENT_SELECT: &str = "SELECT id, calendar_id, uid, title, location, notes, starts, ends, \
     all_day, tz, rrule, exdates, organizer, attendees, response, reminder, sequence, cancelled \
     FROM events";

fn calendar_row(row: &Row<'_>) -> rusqlite::Result<Calendar> {
    Ok(Calendar {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        account: row.get::<_, Option<i64>>(3)?.map(AccountId),
        visible: row.get(4)?,
    })
}

fn json<T: serde::de::DeserializeOwned + Default>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

fn response_text(response: Option<Response>) -> Option<&'static str> {
    response.map(Response::partstat)
}

fn event_row(row: &Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get(0)?,
        calendar: row.get(1)?,
        uid: row.get(2)?,
        title: row.get(3)?,
        location: row.get(4)?,
        notes: row.get(5)?,
        starts: row.get(6)?,
        ends: row.get(7)?,
        all_day: row.get(8)?,
        tz: row.get(9)?,
        rrule: row.get(10)?,
        exdates: json(row, 11)?,
        organizer: row.get(12)?,
        attendees: json(row, 13)?,
        response: row
            .get::<_, Option<String>>(14)?
            .map(|r| Response::from_partstat(&r)),
        reminder: row.get(15)?,
        sequence: row.get(16)?,
        cancelled: row.get(17)?,
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Storage(e.to_string()))
}

/// A reminder that is due: the event and the start of the occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub event: Event,
    pub starts: i64,
}

impl Store {
    pub fn list_calendars(&self) -> Result<Vec<Calendar>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("{CALENDAR_SELECT} ORDER BY position, id"))
            .map_err(storage)?;
        stmt.query_map([], calendar_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// Saves a calendar; a new one (id 0) goes last.
    pub fn save_calendar(&self, calendar: &Calendar) -> Result<Calendar> {
        let name = calendar.name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("Give the calendar a name.".into()));
        }
        let account = calendar.account.map(|a| a.0);
        let id = if calendar.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO calendars (name, color, account_id, visible, position) \
                     VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(position), 0) + 1 FROM calendars))",
                    params![name, calendar.color, account, calendar.visible],
                )
                .map_err(storage)?;
            self.conn.last_insert_rowid()
        } else {
            self.conn
                .execute(
                    "UPDATE calendars SET name = ?2, color = ?3, account_id = ?4, visible = ?5 WHERE id = ?1",
                    params![calendar.id, name, calendar.color, account, calendar.visible],
                )
                .map_err(storage)?;
            calendar.id
        };
        self.conn
            .query_row(
                &format!("{CALENDAR_SELECT} WHERE id = ?1"),
                [id],
                calendar_row,
            )
            .map_err(storage)
    }

    /// Deletes a calendar and its events. The last one stays.
    pub fn delete_calendar(&self, id: i64) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM calendars", [], |r| r.get(0))
            .map_err(storage)?;
        if count <= 1 {
            return Err(Error::Invalid("The last calendar can't be deleted.".into()));
        }
        self.conn
            .execute("DELETE FROM calendars WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    pub fn event(&self, id: i64) -> Result<Option<Event>> {
        self.conn
            .query_row(&format!("{EVENT_SELECT} WHERE id = ?1"), [id], event_row)
            .optional()
            .map_err(storage)
    }

    pub fn event_by_uid(&self, uid: &str) -> Result<Option<Event>> {
        self.conn
            .query_row(&format!("{EVENT_SELECT} WHERE uid = ?1"), [uid], event_row)
            .optional()
            .map_err(storage)
    }

    /// Saves an event: a new one (id 0) gets a UID if it has none.
    pub fn save_event(&self, event: &Event) -> Result<Event> {
        if event.ends < event.starts {
            return Err(Error::Invalid("The event ends before it starts.".into()));
        }
        let calendar: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM calendars WHERE id = ?1",
                [event.calendar],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if calendar.is_none() {
            return Err(Error::Invalid("That calendar no longer exists.".into()));
        }
        let mut event = event.clone();
        if event.uid.trim().is_empty() {
            event.uid = mail_calendar::new_uid();
        }
        let last = mail_calendar::last_end(&event);
        let exdates = to_json(&event.exdates)?;
        let attendees = to_json(&event.attendees)?;
        let values = params![
            event.calendar,
            event.uid,
            event.title.trim(),
            event.location.trim(),
            event.notes,
            event.starts,
            event.ends,
            event.all_day,
            event.tz,
            event.rrule.as_deref().filter(|r| !r.trim().is_empty()),
            exdates,
            event.organizer,
            attendees,
            response_text(event.response),
            event.reminder,
            event.sequence,
            event.cancelled,
            last,
        ];
        let id = if event.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO events (calendar_id, uid, title, location, notes, starts, ends, all_day, tz, \
                     rrule, exdates, organizer, attendees, response, reminder, sequence, cancelled, last_ends) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                    values,
                )
                .map_err(|e| match e {
                    rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
                        Error::Invalid("That event is already in the calendar.".into())
                    }
                    other => storage(other),
                })?;
            self.conn.last_insert_rowid()
        } else {
            let changed = self
                .conn
                .execute(
                    "UPDATE events SET calendar_id = ?1, uid = ?2, title = ?3, location = ?4, notes = ?5, \
                     starts = ?6, ends = ?7, all_day = ?8, tz = ?9, rrule = ?10, exdates = ?11, organizer = ?12, \
                     attendees = ?13, response = ?14, reminder = ?15, sequence = ?16, cancelled = ?17, \
                     last_ends = ?18, reminded = NULL WHERE id = ?19",
                    rusqlite::params_from_iter(values.iter().copied().chain([&event.id as &dyn rusqlite::ToSql])),
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err(Error::Invalid("That event no longer exists.".into()));
            }
            event.id
        };
        self.event(id)?
            .ok_or_else(|| Error::Storage("the saved event is missing".into()))
    }

    pub fn delete_event(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM events WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Takes one occurrence (by its start) out of a repeating event.
    pub fn skip_occurrence(&self, id: i64, starts: i64) -> Result<Event> {
        let mut event = self
            .event(id)?
            .ok_or_else(|| Error::Invalid("That event no longer exists.".into()))?;
        if !event.exdates.contains(&starts) {
            event.exdates.push(starts);
        }
        self.save_event(&event)
    }

    /// Everything happening between `from` and `to` in visible calendars,
    /// by start.
    pub fn occurrences(&self, from: i64, to: i64) -> Result<Vec<Occurrence>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "{EVENT_SELECT} WHERE starts < ?2 AND (last_ends IS NULL OR last_ends > ?1) \
                 AND calendar_id IN (SELECT id FROM calendars WHERE visible = 1)"
            ))
            .map_err(storage)?;
        let events = stmt
            .query_map(params![from, to], event_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        let mut found: Vec<Occurrence> = events
            .iter()
            .flat_map(|e| {
                mail_calendar::occurrences(e, from, to)
                    .into_iter()
                    .map(move |(starts, ends)| Occurrence {
                        event: e.id,
                        calendar: e.calendar,
                        title: e.title.clone(),
                        location: e.location.clone(),
                        starts,
                        ends,
                        all_day: e.all_day,
                        repeats: e.rrule.is_some(),
                        cancelled: e.cancelled,
                        response: e.response,
                    })
            })
            .collect();
        found.sort_by_key(|o| (!o.all_day, o.starts, o.event));
        Ok(found)
    }

    /// Reminders due at `now`: occurrences starting within their reminder
    /// time that haven't been reminded about yet. Each is returned once.
    pub fn due_reminders(&mut self, now: i64) -> Result<Vec<Reminder>> {
        // The longest reminder looked for: a week.
        const AHEAD: i64 = 7 * 86_400;
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "{EVENT_SELECT} WHERE reminder IS NOT NULL AND cancelled = 0 AND starts < ?2 \
                 AND (last_ends IS NULL OR last_ends > ?1) \
                 AND calendar_id IN (SELECT id FROM calendars WHERE visible = 1)"
            ))
            .map_err(storage)?;
        let events = stmt
            .query_map(params![now, now + AHEAD], event_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        drop(stmt);
        let mut due = Vec::new();
        for event in events {
            let Some(minutes) = event.reminder else {
                continue;
            };
            if event.response == Some(Response::Declined) {
                continue;
            }
            let reminded: Option<i64> = self
                .conn
                .query_row(
                    "SELECT reminded FROM events WHERE id = ?1",
                    [event.id],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            let next = mail_calendar::occurrences(&event, now, now + AHEAD)
                .into_iter()
                .map(|(s, _)| s)
                .find(|s| *s >= now);
            if let Some(starts) = next
                && starts - minutes * 60 <= now
                && reminded != Some(starts)
            {
                self.conn
                    .execute(
                        "UPDATE events SET reminded = ?2 WHERE id = ?1",
                        params![event.id, starts],
                    )
                    .map_err(storage)?;
                due.push(Reminder { event, starts });
            }
        }
        Ok(due)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(calendar: i64, title: &str, starts: i64, rrule: Option<&str>) -> Event {
        Event {
            id: 0,
            calendar,
            uid: String::new(),
            title: title.into(),
            location: String::new(),
            notes: String::new(),
            starts,
            ends: starts + 3600,
            all_day: false,
            tz: Some("UTC".into()),
            rrule: rrule.map(str::to_owned),
            exdates: Vec::new(),
            organizer: None,
            attendees: Vec::new(),
            response: None,
            reminder: None,
            sequence: 0,
            cancelled: false,
        }
    }

    const DAY: i64 = 86_400;
    const T0: i64 = 1_790_000_000 - 1_790_000_000 % DAY;

    #[test]
    fn events_are_saved_found_by_range_and_repeat() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let calendars = store.list_calendars().unwrap();
        assert_eq!(calendars.len(), 1, "a first calendar is made");
        let cal = calendars[0].id;

        let once = store
            .save_event(&event(cal, " Dentist ", T0 + 10 * 3600, None))
            .unwrap();
        assert!(once.uid.ends_with("@noveromail"));
        assert_eq!(once.title, "Dentist");
        let daily = store
            .save_event(&event(
                cal,
                "Standup",
                T0 + 9 * 3600,
                Some("FREQ=DAILY;COUNT=3"),
            ))
            .unwrap();
        let titles = |from: i64, to: i64| -> Vec<String> {
            store
                .occurrences(from, to)
                .unwrap()
                .into_iter()
                .map(|o| o.title)
                .collect()
        };
        assert_eq!(titles(T0, T0 + DAY), ["Standup", "Dentist"]);
        assert_eq!(titles(T0 + DAY, T0 + 4 * DAY), ["Standup", "Standup"]);
        assert!(titles(T0 + 3 * DAY, T0 + 9 * DAY).is_empty());

        store
            .skip_occurrence(daily.id, T0 + DAY + 9 * 3600)
            .unwrap();
        assert_eq!(titles(T0 + DAY, T0 + 4 * DAY), ["Standup"]);

        // Hidden calendars are left out.
        let mut hidden = calendars[0].clone();
        hidden.visible = false;
        store.save_calendar(&hidden).unwrap();
        assert!(titles(T0, T0 + DAY).is_empty());

        let mut bad = once.clone();
        bad.ends = bad.starts - 1;
        assert!(store.save_event(&bad).is_err());
        let mut copy = once.clone();
        copy.id = 0;
        assert!(store.save_event(&copy).is_err(), "one event per UID");
        assert!(
            store.delete_calendar(cal).is_err(),
            "the last calendar stays"
        );
        store.delete_event(once.id).unwrap();
        assert!(store.event(once.id).unwrap().is_none());
        assert_eq!(
            store.event_by_uid(&daily.uid).unwrap().unwrap().id,
            daily.id
        );
    }

    #[test]
    fn reminders_come_once_per_occurrence() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let cal = store.list_calendars().unwrap()[0].id;
        let mut e = event(cal, "Call", T0 + 3600, Some("FREQ=DAILY"));
        e.reminder = Some(10);
        store.save_event(&e).unwrap();
        assert!(store.due_reminders(T0 + 3600 - 11 * 60).unwrap().is_empty());
        let due = store.due_reminders(T0 + 3600 - 9 * 60).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].starts, T0 + 3600);
        assert!(store.due_reminders(T0 + 3600 - 5 * 60).unwrap().is_empty());
        // The next day's occurrence reminds again.
        assert_eq!(store.due_reminders(T0 + DAY + 3600 - 60).unwrap().len(), 1);
    }
}
