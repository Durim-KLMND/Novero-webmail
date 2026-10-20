//! Google Calendar API v3.
//!
//! A pull lists series (with their rule and exceptions) rather than each
//! occurrence, and Google's sync token makes the next pull bring only what
//! changed, deletions included.

use mail_core::calendar::{Attendee, Event, RemoteCalendar, RemoteChange, RemoteEvent, Response};
use mail_core::{Error, Result};
use reqwest::Method;
use serde_json::{Value, json};
use url::Url;

use crate::http::{Api, text};
use crate::{Pull, Saved, date_seconds, mailbox, utc_date, zone_of};

const BASE: &str = "https://www.googleapis.com/calendar/v3";

fn url(path: &[&str], query: &[(&str, &str)]) -> Result<String> {
    let mut url = Url::parse(BASE).map_err(|e| Error::Protocol(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| Error::Protocol("bad Google address".into()))?
        .extend(path);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url.into())
}

pub(crate) async fn calendars(api: &Api<'_>) -> Result<Vec<RemoteCalendar>> {
    let mut found = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut query = vec![("maxResults", "250")];
        if let Some(p) = &page {
            query.push(("pageToken", p));
        }
        let body = api
            .get(&url(&["users", "me", "calendarList"], &query)?, &[])
            .await?
            .unwrap_or_default();
        found.extend(
            body["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(calendar),
        );
        match text(&body, "nextPageToken") {
            Some(next) => page = Some(next.to_owned()),
            None => return Ok(found),
        }
    }
}

fn calendar(item: &Value) -> Option<RemoteCalendar> {
    let role = text(item, "accessRole")?;
    // Free/busy access shows no event details.
    if role == "freeBusyReader" || item["deleted"].as_bool() == Some(true) {
        return None;
    }
    Some(RemoteCalendar {
        id: text(item, "id")?.to_owned(),
        name: text(item, "summaryOverride")
            .or(text(item, "summary"))
            .unwrap_or("Calendar")
            .to_owned(),
        color: text(item, "backgroundColor")
            .unwrap_or("#4285f4")
            .to_owned(),
        read_only: !matches!(role, "owner" | "writer"),
        shown: item["selected"].as_bool().unwrap_or(false)
            || item["primary"].as_bool().unwrap_or(false),
    })
}

pub(crate) async fn pull(
    api: &Api<'_>,
    calendar: &str,
    since: Option<&str>,
) -> Result<Option<Pull>> {
    let mut changes = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut query = vec![("maxResults", "2500"), ("singleEvents", "false")];
        match since {
            Some(token) => query.push(("syncToken", token)),
            None => query.push(("showDeleted", "false")),
        }
        if let Some(p) = &page {
            query.push(("pageToken", p));
        }
        // 410: the sync token expired.
        let Some(body) = api
            .get(&url(&["calendars", calendar, "events"], &query)?, &[])
            .await?
        else {
            return Ok(None);
        };
        let default_reminder = body["defaultReminders"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["minutes"].as_i64())
            .min();
        changes.extend(
            body["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| change(item, default_reminder)),
        );
        if let Some(next) = text(&body, "nextPageToken") {
            page = Some(next.to_owned());
            continue;
        }
        return Ok(Some(Pull {
            changes,
            complete: since.is_none(),
            token: text(&body, "nextSyncToken").map(str::to_owned),
        }));
    }
}

/// A `start`/`end`: (UTC seconds, all-day, zone).
fn when(value: &Value) -> Option<(i64, bool, Option<String>)> {
    if let Some(date) = text(value, "date") {
        return Some((date_seconds(date)?, true, None));
    }
    let at: jiff::Timestamp = text(value, "dateTime")?.parse().ok()?;
    Some((
        at.as_second(),
        false,
        text(value, "timeZone").map(str::to_owned),
    ))
}

fn response(status: Option<&str>) -> Response {
    match status {
        Some("accepted") => Response::Accepted,
        Some("tentative") => Response::Tentative,
        Some("declined") => Response::Declined,
        _ => Response::NeedsAction,
    }
}

/// One item of an events list.
pub(crate) fn change(item: &Value, default_reminder: Option<i64>) -> Option<RemoteChange> {
    let id = text(item, "id")?.to_owned();
    let series = text(item, "recurringEventId").map(str::to_owned);
    let original_start = item
        .get("originalStartTime")
        .and_then(when)
        .map(|(at, _, _)| at);
    if text(item, "status") == Some("cancelled") {
        return Some(RemoteChange::Removed {
            id,
            series,
            original_start,
        });
    }
    let (starts, all_day, tz) = when(&item["start"])?;
    let (ends, _, _) = when(&item["end"]).unwrap_or((starts, all_day, None));
    let mut rrule = None;
    let mut exdates = Vec::new();
    for line in item["recurrence"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if let Some(rule) = line.strip_prefix("RRULE:") {
            rrule.get_or_insert_with(|| rule.to_owned());
        } else if line.starts_with("EXDATE") {
            exdates.extend(mail_calendar::ics::exdates(line));
        }
    }
    let attendees: Vec<Attendee> = item["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["resource"].as_bool() != Some(true))
        .filter_map(|a| {
            Some(Attendee {
                email: text(a, "email")?.to_owned(),
                name: text(a, "displayName").map(str::to_owned),
                response: response(text(a, "responseStatus")),
            })
        })
        .collect();
    let me = item["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["self"].as_bool() == Some(true));
    let organizer = &item["organizer"];
    // Someone else's meeting: the app shows who it is from.
    let organizer = (!attendees.is_empty() && organizer["self"].as_bool() != Some(true))
        .then(|| {
            Some(mailbox(
                text(organizer, "email")?,
                text(organizer, "displayName"),
            ))
        })
        .flatten();
    let reminders = &item["reminders"];
    let reminder = if reminders["useDefault"].as_bool().unwrap_or(true) {
        default_reminder
    } else {
        reminders["overrides"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["minutes"].as_i64())
            .min()
    };
    let event = Event {
        id: 0,
        calendar: 0,
        // Moved occurrences share the series' UID; the store makes them one.
        uid: if series.is_some() {
            String::new()
        } else {
            text(item, "iCalUID").unwrap_or_default().to_owned()
        },
        title: text(item, "summary").unwrap_or_default().to_owned(),
        location: text(item, "location").unwrap_or_default().to_owned(),
        notes: plain(text(item, "description").unwrap_or_default()),
        starts,
        ends: ends.max(starts),
        all_day,
        tz: if all_day { None } else { tz },
        rrule,
        exdates,
        organizer: organizer.clone(),
        attendees,
        response: organizer.and(me.map(|m| response(text(m, "responseStatus")))),
        reminder,
        sequence: item["sequence"].as_i64().unwrap_or(0),
        cancelled: false,
        series: None,
    };
    Some(RemoteChange::Changed(Box::new(RemoteEvent {
        id,
        etag: text(item, "etag").map(str::to_owned),
        series,
        original_start,
        event,
    })))
}

/// Google keeps descriptions as simple HTML; the app's notes are text.
pub(crate) fn plain(html: &str) -> String {
    if !html.contains('<') && !html.contains('&') {
        return html.to_owned();
    }
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = &rest[open..];
            break;
        };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        if tag.starts_with("br") || tag == "/p" || tag == "/div" || tag == "/li" {
            out.push('\n');
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .trim_end()
        .to_owned()
}

fn time(seconds: i64, event: &Event) -> Value {
    if event.all_day {
        return json!({ "date": utc_date(seconds) });
    }
    let (_, zone) = zone_of(event);
    let at = jiff::Timestamp::from_second(seconds).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    json!({ "dateTime": at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(), "timeZone": zone })
}

/// The body for saving `event`.
pub(crate) fn body(event: &Event, moved: &[i64], new: bool) -> Value {
    let mut body = json!({
        "summary": event.title,
        "location": event.location,
        "description": event.notes,
        "start": time(event.starts, event),
        "end": time(event.ends, event),
        "reminders": {
            "useDefault": false,
            "overrides": event.reminder.map(|m| vec![json!({ "method": "popup", "minutes": m })]).unwrap_or_default(),
        },
    });
    if event.series.is_none() {
        let recurrence: Vec<String> = match &event.rrule {
            Some(rule) => std::iter::once(format!("RRULE:{rule}"))
                .chain(
                    event
                        .exdates
                        .iter()
                        .filter(|start| !moved.contains(start))
                        .map(|start| mail_calendar::ics::exdate_line(event, *start)),
                )
                .collect(),
            None => Vec::new(),
        };
        if !recurrence.is_empty() || !new {
            body["recurrence"] = json!(recurrence);
        }
    }
    body
}

fn saved(body: &Value) -> Result<Saved> {
    Ok(Saved {
        id: text(body, "id")
            .ok_or_else(|| {
                Error::Protocol("Google Calendar did not say which event it saved".into())
            })?
            .to_owned(),
        etag: text(body, "etag").map(str::to_owned),
    })
}

pub(crate) async fn create(
    api: &Api<'_>,
    calendar: &str,
    event: &Event,
    moved: &[i64],
) -> Result<Saved> {
    let body = body(event, moved, true);
    let answer = api
        .send(
            Method::POST,
            &url(&["calendars", calendar, "events"], &[])?,
            Some(&body),
            &[],
        )
        .await?
        .ok_or_else(|| Error::Rejected("Google Calendar: that calendar is gone".into()))?;
    saved(&answer)
}

pub(crate) async fn update(
    api: &Api<'_>,
    calendar: &str,
    id: &str,
    event: &Event,
    moved: &[i64],
) -> Result<Option<Saved>> {
    let body = body(event, moved, false);
    match api
        .send(
            Method::PATCH,
            &url(&["calendars", calendar, "events", id], &[])?,
            Some(&body),
            &[],
        )
        .await?
    {
        Some(answer) => saved(&answer).map(Some),
        None => Ok(None),
    }
}

pub(crate) async fn delete(api: &Api<'_>, calendar: &str, id: &str) -> Result<()> {
    api.send(
        Method::DELETE,
        &url(&["calendars", calendar, "events", id], &[])?,
        None,
        &[],
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendars_are_read_from_the_list() {
        let list = json!({"items": [
            {"id": "me@gmail.com", "summary": "me@gmail.com", "summaryOverride": "Me", "backgroundColor": "#9fe1e7", "accessRole": "owner", "primary": true},
            {"id": "en.usa#holiday@group.v.calendar.google.com", "summary": "Holidays", "accessRole": "reader"},
            {"id": "busy", "summary": "Busy", "accessRole": "freeBusyReader"}
        ]});
        let found: Vec<_> = list["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(calendar)
            .collect();
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].name.as_str(), found[0].read_only, found[0].shown),
            ("Me", false, true)
        );
        assert_eq!((found[1].read_only, found[1].shown), (true, false));
        assert_eq!(found[1].color, "#4285f4");
    }

    #[test]
    fn events_series_and_moved_occurrences_are_read() {
        let series = json!({
            "id": "abc", "etag": "\"1\"", "status": "confirmed", "iCalUID": "abc@google.com",
            "summary": "Standup", "description": "<b>Daily</b><br>notes &amp; more",
            "start": {"dateTime": "2026-10-20T09:00:00+02:00", "timeZone": "Europe/Berlin"},
            "end": {"dateTime": "2026-10-20T09:15:00+02:00", "timeZone": "Europe/Berlin"},
            "recurrence": ["RRULE:FREQ=WEEKLY;BYDAY=MO,TU", "EXDATE;TZID=Europe/Berlin:20261027T090000"],
            "reminders": {"useDefault": true}
        });
        let Some(RemoteChange::Changed(e)) = change(&series, Some(10)) else {
            panic!()
        };
        assert_eq!(e.event.starts, 1_792_479_600);
        assert_eq!(e.event.ends - e.event.starts, 900);
        assert_eq!(e.event.tz.as_deref(), Some("Europe/Berlin"));
        assert_eq!(e.event.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,TU"));
        assert_eq!(e.event.exdates, [1_792_479_600 + 7 * 86_400 + 3600]);
        assert_eq!(e.event.notes, "Daily\nnotes & more");
        assert_eq!(e.event.reminder, Some(10));
        assert_eq!(e.event.uid, "abc@google.com");

        let moved = json!({
            "id": "abc_20261021T070000Z", "status": "confirmed", "recurringEventId": "abc",
            "originalStartTime": {"dateTime": "2026-10-21T09:00:00+02:00"},
            "start": {"date": "2026-10-21"}, "end": {"date": "2026-10-22"},
            "reminders": {"useDefault": false, "overrides": [{"method": "popup", "minutes": 30}, {"method": "email", "minutes": 60}]},
            "organizer": {"email": "ann@work.test", "displayName": "Ann"},
            "attendees": [{"email": "ann@work.test", "responseStatus": "accepted", "organizer": true}, {"email": "me@gmail.com", "self": true, "responseStatus": "tentative"}]
        });
        let Some(RemoteChange::Changed(m)) = change(&moved, None) else {
            panic!()
        };
        assert_eq!(m.series.as_deref(), Some("abc"));
        assert_eq!(m.original_start, Some(1_792_566_000));
        assert!(m.event.all_day);
        assert_eq!(m.event.ends - m.event.starts, 86_400);
        assert_eq!(m.event.uid, "");
        assert_eq!(m.event.reminder, Some(30));
        assert_eq!(m.event.organizer.as_deref(), Some("Ann <ann@work.test>"));
        assert_eq!(m.event.response, Some(Response::Tentative));

        let gone = json!({"id": "abc_x", "status": "cancelled", "recurringEventId": "abc", "originalStartTime": {"date": "2026-10-22"}});
        assert_eq!(
            change(&gone, None),
            Some(RemoteChange::Removed {
                id: "abc_x".into(),
                series: Some("abc".into()),
                original_start: Some(1_792_627_200)
            })
        );
    }

    #[test]
    fn events_are_written_with_their_rule_and_exceptions() {
        let Some(RemoteChange::Changed(e)) = change(
            &json!({
                "id": "abc", "start": {"dateTime": "2026-10-20T07:00:00Z", "timeZone": "Europe/Berlin"},
                "end": {"dateTime": "2026-10-20T08:00:00Z"}, "recurrence": ["RRULE:FREQ=DAILY"],
            }),
            None,
        ) else {
            panic!()
        };
        let mut event = e.event;
        event.exdates = vec![1_792_479_600 + 86_400, 1_792_479_600 + 2 * 86_400];
        event.reminder = Some(15);
        let body = body(&event, &[1_792_479_600 + 2 * 86_400], false);
        assert_eq!(
            body["start"],
            json!({"dateTime": "2026-10-20T07:00:00Z", "timeZone": "Europe/Berlin"})
        );
        assert_eq!(
            body["recurrence"],
            json!([
                "RRULE:FREQ=DAILY",
                "EXDATE;TZID=Europe/Berlin:20261021T090000"
            ])
        );
        assert_eq!(body["reminders"]["overrides"][0]["minutes"], 15);

        event.rrule = None;
        event.all_day = true;
        event.starts = 1_792_454_400;
        event.ends = event.starts + 86_400;
        let single = super::body(&event, &[], true);
        assert_eq!(single["end"], json!({"date": "2026-10-21"}));
        assert!(single.get("recurrence").is_none());
        assert_eq!(super::body(&event, &[], false)["recurrence"], json!([]));
    }
}
