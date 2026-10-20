//! Calendars synced with online accounts: the accounts, the changes made
//! here that the server hasn't had yet, and taking in the server's changes.
//!
//! An event changed here gets a `dirty` number, bumped on every change, and
//! keeps it until the server has the change. A pushed change clears it only
//! if nothing changed meanwhile, and the server's copy never overwrites an
//! event with a change still waiting.

use std::collections::HashSet;

use mail_core::calendar::{CalendarAccount, Event, RemoteCalendar, RemoteChange, RemoteEvent};
use mail_core::{Error, OAuthProvider, Result};
use rusqlite::{OptionalExtension, Row, params};

use crate::calendar::{EVENT_SELECT, event_row, to_json};
use crate::storage;
use crate::store::Store;

/// An event's calendar, remote id and series.
type RemoteIds = (i64, Option<String>, Option<String>);

const ACCOUNT_SELECT: &str =
    "SELECT id, provider, email, synced_at, error, needs_signin FROM calendar_accounts";

fn account_row(row: &Row<'_>) -> rusqlite::Result<CalendarAccount> {
    let provider: String = row.get(1)?;
    Ok(CalendarAccount {
        id: row.get(0)?,
        provider: OAuthProvider::parse(&provider).unwrap_or(OAuthProvider::Google),
        email: row.get(2)?,
        synced_at: row.get(3)?,
        error: row.get(4)?,
        needs_sign_in: row.get(5)?,
    })
}

/// An online calendar as the sync needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedCalendar {
    pub id: i64,
    pub remote_id: String,
    pub read_only: bool,
    pub sync_token: Option<String>,
}

/// An event changed here, to send to the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSave {
    pub event: Event,
    /// `None` for an event the server doesn't have yet.
    pub remote_id: Option<String>,
    /// The change count when read; see [`Store::pushed`].
    pub dirty: i64,
    /// Starts of the series' occurrences kept as their own events. These are
    /// not in `event.exdates` as the server sees them.
    pub moved: Vec<i64>,
}

/// What a calendar has waiting for the server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pending {
    pub saves: Vec<PendingSave>,
    pub deletions: Vec<String>,
}

impl Store {
    pub fn list_calendar_accounts(&self) -> Result<Vec<CalendarAccount>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("{ACCOUNT_SELECT} ORDER BY id"))
            .map_err(storage)?;
        stmt.query_map([], account_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    pub fn calendar_account(&self, id: i64) -> Result<Option<CalendarAccount>> {
        self.conn
            .query_row(
                &format!("{ACCOUNT_SELECT} WHERE id = ?1"),
                [id],
                account_row,
            )
            .optional()
            .map_err(storage)
    }

    /// Adds an online account, or returns the one already added for this
    /// provider and address.
    pub fn add_calendar_account(
        &self,
        provider: OAuthProvider,
        email: &str,
    ) -> Result<CalendarAccount> {
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM calendar_accounts WHERE provider = ?1 AND email = ?2 COLLATE NOCASE",
                params![provider.as_str(), email],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let id = match existing {
            Some(id) => id,
            None => {
                self.conn
                    .execute(
                        "INSERT INTO calendar_accounts (provider, email) VALUES (?1, ?2)",
                        params![provider.as_str(), email],
                    )
                    .map_err(storage)?;
                self.conn.last_insert_rowid()
            }
        };
        self.calendar_account(id)?
            .ok_or_else(|| Error::Storage("the calendar account is missing".into()))
    }

    /// Removes an online account with its calendars and their events.
    pub fn remove_calendar_account(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM calendar_accounts WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Notes how the last sync went: `synced_at` is set when it worked.
    pub fn set_calendar_account_status(
        &self,
        id: i64,
        synced_at: Option<i64>,
        error: Option<&str>,
        needs_sign_in: bool,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE calendar_accounts SET synced_at = COALESCE(?2, synced_at), error = ?3, \
                 needs_signin = ?4 WHERE id = ?1",
                params![id, synced_at, error, needs_sign_in],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Brings an account's calendars in line with the list the server gave:
    /// new ones are added (after the others), gone ones removed with their
    /// events. Names and access follow the server; colour and whether a
    /// calendar is shown are the user's.
    pub fn sync_calendar_list(
        &self,
        account: i64,
        remote: &[RemoteCalendar],
    ) -> Result<Vec<SyncedCalendar>> {
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        for cal in remote {
            let changed = tx
                .execute(
                    "UPDATE calendars SET name = ?3, read_only = ?4 WHERE source = ?1 AND remote_id = ?2",
                    params![account, cal.id, cal.name, cal.read_only],
                )
                .map_err(storage)?;
            if changed == 0 {
                tx.execute(
                    "INSERT INTO calendars (name, color, visible, position, source, remote_id, read_only) \
                     VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM calendars), ?4, ?5, ?6)",
                    params![cal.name, cal.color, cal.shown, account, cal.id, cal.read_only],
                )
                .map_err(storage)?;
            }
        }
        let keep: HashSet<&str> = remote.iter().map(|c| c.id.as_str()).collect();
        let mut stmt = tx
            .prepare("SELECT id, remote_id, read_only, sync_token FROM calendars WHERE source = ?1")
            .map_err(storage)?;
        let all = stmt
            .query_map([account], |r| {
                Ok(SyncedCalendar {
                    id: r.get(0)?,
                    remote_id: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    read_only: r.get(2)?,
                    sync_token: r.get(3)?,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        drop(stmt);
        let mut kept = Vec::new();
        for cal in all {
            if keep.contains(cal.remote_id.as_str()) {
                kept.push(cal);
            } else {
                tx.execute("DELETE FROM calendars WHERE id = ?1", [cal.id])
                    .map_err(storage)?;
            }
        }
        tx.commit().map_err(storage)?;
        // In the server's order.
        kept.sort_by_key(|c| remote.iter().position(|r| r.id == c.remote_id));
        Ok(kept)
    }

    /// Where the next pull of a calendar starts; `None` makes it a full one.
    pub fn set_sync_token(&self, calendar: i64, token: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE calendars SET sync_token = ?2 WHERE id = ?1",
                params![calendar, token],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The online calendar of an event, if it is in one, and whether it may
    /// be changed.
    fn online(&self, calendar: i64) -> Result<Option<bool>> {
        let row: Option<(Option<i64>, bool)> = self
            .conn
            .query_row(
                "SELECT source, read_only FROM calendars WHERE id = ?1",
                [calendar],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        Ok(row.and_then(|(source, read_only)| source.map(|_| read_only)))
    }

    fn refuse_read_only(&self, calendar: i64) -> Result<bool> {
        match self.online(calendar)? {
            Some(true) => Err(Error::Invalid(
                "That calendar can only be looked at, not changed.".into(),
            )),
            Some(false) => Ok(true),
            None => Ok(false),
        }
    }

    /// An event's calendar, remote id and series.
    fn remote_ids(&self, id: i64) -> Result<Option<RemoteIds>> {
        self.conn
            .query_row(
                "SELECT calendar_id, remote_id, series FROM events WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(storage)
    }

    fn note_deletion(&self, calendar: i64, remote_id: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO calendar_deletions (calendar_id, remote_id) VALUES (?1, ?2)",
                params![calendar, remote_id],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn mark_dirty(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE events SET dirty = dirty + 1 WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Saves an event the user changed. In an online calendar the change
    /// waits for the server; moved to another calendar, the old one loses it.
    pub fn edit_event(&self, event: &Event) -> Result<Event> {
        let online = self.refuse_read_only(event.calendar)?;
        let before = match event.id {
            0 => None,
            id => self.remote_ids(id)?,
        };
        if let Some((old_calendar, remote_id, _)) = &before
            && *old_calendar != event.calendar
        {
            self.refuse_read_only(*old_calendar)?;
            if let Some(remote_id) = remote_id {
                self.note_deletion(*old_calendar, remote_id)?;
            }
        }
        let saved = self.save_event(event)?;
        if let Some((old_calendar, _, _)) = before
            && old_calendar != event.calendar
        {
            // In its new calendar it is a new event.
            self.conn
                .execute(
                    "UPDATE events SET remote_id = NULL, etag = NULL, series = NULL, \
                     original_start = NULL WHERE id = ?1",
                    [saved.id],
                )
                .map_err(storage)?;
        }
        if online {
            self.mark_dirty(saved.id)?;
        }
        self.event(saved.id)?
            .ok_or_else(|| Error::Storage("the saved event is missing".into()))
    }

    /// Deletes an event the user deleted. For an occurrence of an online
    /// series, the whole series goes.
    pub fn remove_event(&self, id: i64) -> Result<()> {
        let Some((calendar, remote_id, series)) = self.remote_ids(id)? else {
            return Ok(());
        };
        if self.refuse_read_only(calendar)? {
            let target = series.or(remote_id);
            if let Some(target) = target {
                self.note_deletion(calendar, &target)?;
                self.conn
                    .execute(
                        "DELETE FROM events WHERE calendar_id = ?1 AND (remote_id = ?2 OR series = ?2)",
                        params![calendar, target],
                    )
                    .map_err(storage)?;
            }
        }
        self.delete_event(id)
    }

    /// Takes one occurrence out of a repeating event the user changed. An
    /// occurrence kept as its own event is deleted on its own.
    pub fn skip_change(&self, id: i64, starts: i64) -> Result<Option<Event>> {
        let Some((calendar, remote_id, series)) = self.remote_ids(id)? else {
            return Err(Error::Invalid("That event no longer exists.".into()));
        };
        let online = self.refuse_read_only(calendar)?;
        if series.is_some() {
            if online && let Some(remote_id) = remote_id {
                self.note_deletion(calendar, &remote_id)?;
            }
            self.delete_event(id)?;
            return Ok(None);
        }
        let saved = self.skip_occurrence(id, starts)?;
        if online {
            self.mark_dirty(id)?;
        }
        Ok(Some(saved))
    }

    /// Changes and deletions of a calendar waiting for the server.
    pub fn pending(&self, calendar: i64) -> Result<Pending> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "{} WHERE calendar_id = ?1 AND dirty > 0",
                EVENT_SELECT.replace(" FROM events", ", remote_id, dirty FROM events")
            ))
            .map_err(storage)?;
        let rows = stmt
            .query_map([calendar], |r| {
                Ok((event_row(r)?, r.get::<_, Option<String>>(19)?, r.get(20)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<(Event, Option<String>, i64)>>>()
            .map_err(storage)?;
        drop(stmt);
        let mut saves = Vec::new();
        for (event, remote_id, dirty) in rows {
            let moved = match &remote_id {
                Some(series) if event.rrule.is_some() => self.moved_starts(calendar, series)?,
                _ => Vec::new(),
            };
            saves.push(PendingSave {
                event,
                remote_id,
                dirty,
                moved,
            });
        }
        let mut stmt = self
            .conn
            .prepare_cached("SELECT remote_id FROM calendar_deletions WHERE calendar_id = ?1")
            .map_err(storage)?;
        let deletions = stmt
            .query_map([calendar], |r| r.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        Ok(Pending { saves, deletions })
    }

    fn moved_starts(&self, calendar: i64, series: &str) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT original_start FROM events WHERE calendar_id = ?1 AND series = ?2 \
                 AND original_start IS NOT NULL",
            )
            .map_err(storage)?;
        stmt.query_map(params![calendar, series], |r| r.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// The server has the change counted `dirty` (and, for a new event, gave
    /// it `remote_id`). A later change still waits.
    pub fn pushed(&self, id: i64, dirty: i64, remote_id: &str, etag: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE events SET remote_id = ?3, etag = ?4, \
                 dirty = CASE WHEN dirty = ?2 THEN 0 ELSE dirty END WHERE id = ?1",
                params![id, dirty, remote_id, etag],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The server refused a change: it is dropped, and the next pull brings
    /// back the server's copy.
    pub fn drop_change(&self, id: i64, calendar: i64) -> Result<()> {
        let remote: Option<String> = self
            .conn
            .query_row("SELECT remote_id FROM events WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?
            .flatten();
        if remote.is_some() {
            self.conn
                .execute("UPDATE events SET dirty = 0 WHERE id = ?1", [id])
                .map_err(storage)?;
        } else {
            self.delete_event(id)?;
        }
        self.set_sync_token(calendar, None)
    }

    /// The server deleted the event too (or never had it).
    pub fn deletion_done(&self, calendar: i64, remote_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM calendar_deletions WHERE calendar_id = ?1 AND remote_id = ?2",
                params![calendar, remote_id],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Takes in the server's changes. With `complete`, `changes` is all the
    /// calendar has, and events it doesn't list go, unless changed here.
    /// Returns whether anything changed.
    pub fn apply_remote(
        &self,
        calendar: i64,
        changes: &[RemoteChange],
        complete: bool,
    ) -> Result<bool> {
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        let mut changed = false;
        let mut seen: HashSet<&str> = HashSet::new();
        // Series before their occurrences, so those can be taken out of them.
        let mut ordered: Vec<&RemoteChange> = changes.iter().collect();
        ordered.sort_by_key(|c| match c {
            RemoteChange::Changed(e) => e.series.is_some(),
            RemoteChange::Removed { series, .. } => series.is_some(),
        });
        for change in ordered {
            match change {
                RemoteChange::Changed(remote) => {
                    seen.insert(&remote.id);
                    changed |= self.take_remote(calendar, remote)?;
                }
                RemoteChange::Removed {
                    id,
                    series,
                    original_start,
                } => {
                    let gone = self
                        .conn
                        .execute(
                            "DELETE FROM events WHERE calendar_id = ?1 AND dirty = 0 \
                             AND (remote_id = ?2 OR series = ?2)",
                            params![calendar, id],
                        )
                        .map_err(storage)?;
                    changed |= gone > 0;
                    if let (Some(series), Some(start)) = (series, original_start) {
                        changed |= self.skip_in_series(calendar, series, *start)?;
                    }
                }
            }
        }
        if complete {
            let mut stmt = self
                .conn
                .prepare("SELECT id, remote_id FROM events WHERE calendar_id = ?1 AND dirty = 0")
                .map_err(storage)?;
            let stale: Vec<i64> = stmt
                .query_map([calendar], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
                })
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?
                .into_iter()
                .filter(|(_, remote)| !remote.as_deref().is_some_and(|r| seen.contains(r)))
                .map(|(id, _)| id)
                .collect();
            drop(stmt);
            for id in stale {
                self.delete_event(id)?;
                changed = true;
            }
        }
        tx.commit().map_err(storage)?;
        Ok(changed)
    }

    /// Adds `start` to the exceptions of the series `series`, if it is here.
    fn skip_in_series(&self, calendar: i64, series: &str, start: i64) -> Result<bool> {
        let master: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT id, exdates FROM events WHERE calendar_id = ?1 AND remote_id = ?2 AND series IS NULL",
                params![calendar, series],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        let Some((id, text)) = master else {
            return Ok(false);
        };
        let mut exdates: Vec<i64> = serde_json::from_str(&text).unwrap_or_default();
        if exdates.contains(&start) {
            return Ok(false);
        }
        exdates.push(start);
        exdates.sort_unstable();
        self.conn
            .execute(
                "UPDATE events SET exdates = ?2 WHERE id = ?1",
                params![id, to_json(&exdates)?],
            )
            .map_err(storage)?;
        Ok(true)
    }

    /// Saves one event from the server. Returns whether anything changed.
    fn take_remote(&self, calendar: i64, remote: &RemoteEvent) -> Result<bool> {
        let existing: Option<(i64, i64, Option<String>)> = self
            .conn
            .query_row(
                "SELECT id, dirty, etag FROM events WHERE calendar_id = ?1 AND remote_id = ?2",
                params![calendar, remote.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(storage)?;
        let mut event = remote.event.clone();
        event.calendar = calendar;
        event.series = remote.series.clone();
        let id = match existing {
            // A change made here wins until the server has it.
            Some((_, dirty, _)) if dirty > 0 => return Ok(false),
            Some((_, _, etag)) if etag.is_some() && etag == remote.etag => return Ok(false),
            Some((id, _, _)) => {
                let old = self.event(id)?;
                event.id = id;
                if let Some(old) = old {
                    // Exceptions from occurrences kept as their own events.
                    for start in old.exdates {
                        if !event.exdates.contains(&start) {
                            event.exdates.push(start);
                        }
                    }
                    event.uid = old.uid;
                }
                self.save_event(&event)?.id
            }
            None => {
                // An invitation already taken from mail becomes this event.
                let local: Option<i64> = self
                    .conn
                    .query_row(
                        "SELECT id FROM events WHERE uid = ?1 AND remote_id IS NULL AND dirty = 0",
                        [&event.uid],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(storage)?;
                let taken: bool = self
                    .conn
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM events WHERE uid = ?1)",
                        [&event.uid],
                        |r| r.get(0),
                    )
                    .map_err(storage)?;
                match local {
                    Some(id) => {
                        event.id = id;
                    }
                    None if taken || event.uid.is_empty() => {
                        event.uid = format!("{}/{}@sync", calendar, remote.id);
                    }
                    None => {}
                }
                self.save_event(&event)?.id
            }
        };
        self.conn
            .execute(
                "UPDATE events SET remote_id = ?2, etag = ?3, series = ?4, original_start = ?5, \
                 dirty = 0 WHERE id = ?1",
                params![
                    id,
                    remote.id,
                    remote.etag,
                    remote.series,
                    remote.original_start
                ],
            )
            .map_err(storage)?;
        if let (Some(series), Some(start)) = (&remote.series, remote.original_start) {
            self.skip_in_series(calendar, series, start)?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::calendar::Calendar;

    const T0: i64 = 1_790_000_000 - 1_790_000_000 % 86_400;

    fn event(title: &str, starts: i64, rrule: Option<&str>) -> Event {
        Event {
            id: 0,
            calendar: 0,
            uid: format!("{title}@remote"),
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
            series: None,
        }
    }

    fn remote(id: &str, etag: &str, event: Event) -> RemoteChange {
        RemoteChange::Changed(Box::new(RemoteEvent {
            id: id.into(),
            etag: Some(etag.into()),
            series: None,
            original_start: None,
            event,
        }))
    }

    fn setup() -> (tempfile::TempDir, Store, i64) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let account = store
            .add_calendar_account(OAuthProvider::Google, "me@gmail.test")
            .unwrap();
        let cals = store
            .sync_calendar_list(
                account.id,
                &[RemoteCalendar {
                    id: "primary".into(),
                    name: "Me".into(),
                    color: "#123456".into(),
                    read_only: false,
                    shown: true,
                }],
            )
            .unwrap();
        (dir, store, cals[0].id)
    }

    fn titles(store: &Store) -> Vec<String> {
        store
            .occurrences(T0, T0 + 10 * 86_400)
            .unwrap()
            .into_iter()
            .map(|o| o.title)
            .collect()
    }

    #[test]
    fn calendars_follow_the_account() {
        let (_dir, store, cal) = setup();
        let account = store.list_calendar_accounts().unwrap()[0].clone();
        assert_eq!(
            store
                .add_calendar_account(OAuthProvider::Google, "ME@gmail.test")
                .unwrap()
                .id,
            account.id,
            "the same account is added once"
        );
        let mut mine = store
            .list_calendars()
            .unwrap()
            .into_iter()
            .find(|c| c.id == cal)
            .unwrap();
        assert_eq!(mine.source, Some(account.id));
        assert!(store.delete_calendar(cal).is_err());

        // The user's colour stays; a calendar gone from the server goes.
        mine.color = "#ff0000".into();
        store.save_calendar(&mine).unwrap();
        let list = [
            RemoteCalendar {
                id: "primary".into(),
                name: "Renamed".into(),
                color: "#123456".into(),
                read_only: false,
                shown: true,
            },
            RemoteCalendar {
                id: "holidays".into(),
                name: "Holidays".into(),
                color: "#00ff00".into(),
                read_only: true,
                shown: false,
            },
        ];
        let synced = store.sync_calendar_list(account.id, &list).unwrap();
        assert_eq!(synced.len(), 2);
        assert!(synced[1].read_only);
        let all: Vec<Calendar> = store.list_calendars().unwrap();
        let renamed = all.iter().find(|c| c.id == cal).unwrap();
        assert_eq!(
            (renamed.name.as_str(), renamed.color.as_str()),
            ("Renamed", "#ff0000")
        );
        assert!(!all.iter().find(|c| c.name == "Holidays").unwrap().visible);

        // Nothing can be added to a read-only calendar.
        let mut e = event("x", T0, None);
        e.calendar = synced[1].id;
        assert!(store.edit_event(&e).is_err());

        store.sync_calendar_list(account.id, &list[1..]).unwrap();
        assert!(store.list_calendars().unwrap().iter().all(|c| c.id != cal));

        store.remove_calendar_account(account.id).unwrap();
        assert_eq!(
            store.list_calendars().unwrap().len(),
            1,
            "the local one stays"
        );
    }

    #[test]
    fn server_changes_come_in_and_local_ones_wait_for_it() {
        let (_dir, store, cal) = setup();
        let changes = [
            remote("a", "1", event("Lunch", T0 + 12 * 3600, None)),
            remote(
                "b",
                "1",
                event("Standup", T0 + 9 * 3600, Some("FREQ=DAILY;COUNT=3")),
            ),
        ];
        assert!(store.apply_remote(cal, &changes, true).unwrap());
        assert!(
            !store.apply_remote(cal, &changes, false).unwrap(),
            "same etags"
        );
        assert_eq!(titles(&store), ["Standup", "Lunch", "Standup", "Standup"]);
        assert!(store.pending(cal).unwrap().saves.is_empty());

        // An occurrence moved on the server comes as its own event.
        let mut moved = event("Standup (moved)", T0 + 86_400 + 15 * 3600, None);
        moved.uid = String::new();
        let instance = RemoteChange::Changed(Box::new(RemoteEvent {
            id: "b_1".into(),
            etag: Some("1".into()),
            series: Some("b".into()),
            original_start: Some(T0 + 86_400 + 9 * 3600),
            event: moved,
        }));
        store.apply_remote(cal, &[instance], false).unwrap();
        assert_eq!(
            titles(&store),
            ["Standup", "Lunch", "Standup (moved)", "Standup"]
        );

        // Changed here: waits, and the server's copy doesn't overwrite it.
        let lunch = store
            .occurrences(T0, T0 + 86_400)
            .unwrap()
            .into_iter()
            .find(|o| o.title == "Lunch")
            .unwrap();
        let mut edited = store.event(lunch.event).unwrap().unwrap();
        edited.title = "Long lunch".into();
        store.edit_event(&edited).unwrap();
        let pending = store.pending(cal).unwrap();
        assert_eq!(pending.saves.len(), 1);
        assert_eq!(pending.saves[0].remote_id.as_deref(), Some("a"));
        store
            .apply_remote(
                cal,
                &[remote("a", "2", event("Lunch", T0 + 12 * 3600, None))],
                false,
            )
            .unwrap();
        assert_eq!(
            store.event(lunch.event).unwrap().unwrap().title,
            "Long lunch"
        );

        // Pushed, then changed again meanwhile: the later change still waits.
        let dirty = pending.saves[0].dirty;
        store.edit_event(&edited).unwrap();
        store.pushed(lunch.event, dirty, "a", Some("3")).unwrap();
        assert_eq!(store.pending(cal).unwrap().saves.len(), 1);
        let dirty = store.pending(cal).unwrap().saves[0].dirty;
        store.pushed(lunch.event, dirty, "a", Some("4")).unwrap();
        assert!(store.pending(cal).unwrap().saves.is_empty());

        // The series' moved occurrence isn't sent as an exception.
        let standup = store
            .occurrences(T0, T0 + 86_400)
            .unwrap()
            .into_iter()
            .find(|o| o.title == "Standup")
            .unwrap();
        store
            .skip_change(standup.event, T0 + 2 * 86_400 + 9 * 3600)
            .unwrap();
        let pending = store.pending(cal).unwrap();
        assert_eq!(pending.saves[0].moved, [T0 + 86_400 + 9 * 3600]);
        assert_eq!(pending.saves[0].event.exdates.len(), 2);

        // Deleting the moved occurrence deletes the whole series.
        let instance = store
            .occurrences(T0, T0 + 10 * 86_400)
            .unwrap()
            .into_iter()
            .find(|o| o.title == "Standup (moved)")
            .unwrap();
        assert!(instance.repeats);
        store.remove_event(instance.event).unwrap();
        assert_eq!(store.pending(cal).unwrap().deletions, ["b"]);
        assert_eq!(titles(&store), ["Long lunch"]);
        store.deletion_done(cal, "b").unwrap();
        assert!(store.pending(cal).unwrap().deletions.is_empty());

        // A full listing without the lunch removes it.
        store.apply_remote(cal, &[], true).unwrap();
        assert!(titles(&store).is_empty());
    }

    #[test]
    fn an_invitation_from_mail_becomes_the_synced_event() {
        let (_dir, store, cal) = setup();
        let mut invite = event("Planning", T0 + 14 * 3600, None);
        invite.uid = "inv-1".into();
        let taken = store
            .take_invitation(&invite, None, mail_core::AccountId(1))
            .unwrap();
        assert_ne!(taken.calendar, cal, "invitations go to a local calendar");
        store
            .apply_remote(cal, &[remote("p", "1", invite.clone())], false)
            .unwrap();
        let all = store.occurrences(T0, T0 + 86_400).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].event, all[0].calendar), (taken.id, cal));
    }
}
