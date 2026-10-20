//! Outlook.com and Microsoft 365 calendars through Microsoft Graph.
//!
//! Graph lists each occurrence of a repeating event on its own when asked
//! for a span of time (the calendar view). A pull takes the whole span from
//! a few months back to a year ahead, so it is always complete.

use jiff::tz::TimeZone;
use mail_core::calendar::{Attendee, Event, RemoteCalendar, RemoteChange, RemoteEvent, Response};
use mail_core::{Error, Result};
use reqwest::Method;
use serde_json::{Value, json};
use url::Url;

use crate::http::{Api, text};
use crate::{Pull, Saved, civil, mailbox, zone_of};

const BASE: &str = "https://graph.microsoft.com/v1.0";

/// The span a pull covers, around now.
const BEHIND: i64 = 90 * 86_400;
const AHEAD: i64 = 400 * 86_400;

/// Times in UTC and notes as plain text.
const PREFER: &[&str] = &[
    "outlook.timezone=\"UTC\"",
    "outlook.body-content-type=\"text\"",
];

const FIELDS: &str = "id,changeKey,iCalUId,type,seriesMasterId,originalStart,subject,body,location,start,end,\
isAllDay,isCancelled,isOrganizer,organizer,attendees,responseStatus,isReminderOn,reminderMinutesBeforeStart";

fn url(path: &[&str], query: &[(&str, &str)]) -> Result<String> {
    let mut url = Url::parse(BASE).map_err(|e| Error::Protocol(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| Error::Protocol("bad Graph address".into()))?
        .extend(path);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url.into())
}

/// Follows `@odata.nextLink` pages, collecting `value`.
async fn all(api: &Api<'_>, first: String) -> Result<Option<Vec<Value>>> {
    let mut items = Vec::new();
    let mut next = Some(first);
    while let Some(link) = next.take() {
        let Some(body) = api.get(&link, PREFER).await? else {
            return Ok(None);
        };
        if let Some(values) = body["value"].as_array() {
            items.extend(values.iter().cloned());
        }
        next = text(&body, "@odata.nextLink").map(str::to_owned);
    }
    Ok(Some(items))
}

pub(crate) async fn calendars(api: &Api<'_>) -> Result<Vec<RemoteCalendar>> {
    let items = all(api, url(&["me", "calendars"], &[("$top", "100")])?)
        .await?
        .unwrap_or_default();
    Ok(items.iter().filter_map(calendar).collect())
}

/// Outlook's named colours, for calendars without a hex one.
fn named_color(name: &str) -> &'static str {
    match name {
        "lightBlue" => "#4f9ddb",
        "lightGreen" => "#5fb36b",
        "lightOrange" => "#e8984a",
        "lightGray" => "#8f9496",
        "lightYellow" => "#d6b53c",
        "lightTeal" => "#3fadaa",
        "lightPink" => "#d9739f",
        "lightBrown" => "#a87d5c",
        "lightRed" => "#d9605a",
        _ => "#0078d4",
    }
}

fn calendar(item: &Value) -> Option<RemoteCalendar> {
    let can_edit = item["canEdit"].as_bool().unwrap_or(false);
    Some(RemoteCalendar {
        id: text(item, "id")?.to_owned(),
        name: text(item, "name").unwrap_or("Calendar").to_owned(),
        color: text(item, "hexColor")
            .map(str::to_owned)
            .unwrap_or_else(|| named_color(text(item, "color").unwrap_or("auto")).to_owned()),
        read_only: !can_edit,
        shown: can_edit || item["isDefaultCalendar"].as_bool().unwrap_or(false),
    })
}

pub(crate) async fn pull(api: &Api<'_>, calendar: &str, now: i64) -> Result<Pull> {
    let utc = TimeZone::UTC;
    let from = format!("{}Z", civil(now - BEHIND, &utc));
    let to = format!("{}Z", civil(now + AHEAD, &utc));
    let first = url(
        &["me", "calendars", calendar, "calendarView"],
        &[
            ("startDateTime", &from),
            ("endDateTime", &to),
            ("$top", "250"),
            ("$select", FIELDS),
        ],
    )?;
    let items = all(api, first)
        .await?
        .ok_or_else(|| Error::Rejected("Outlook: that calendar is gone".into()))?;
    Ok(Pull {
        changes: items.iter().filter_map(change).collect(),
        complete: true,
        token: None,
    })
}

/// A Graph `dateTimeTimeZone` in UTC (as asked for with `Prefer`).
fn when(value: &Value) -> Option<i64> {
    let text = text(value, "dateTime")?;
    let civil: jiff::civil::DateTime = text.get(..19)?.parse().ok()?;
    let zone = text_zone(value);
    Some(civil.to_zoned(zone).ok()?.timestamp().as_second())
}

fn text_zone(value: &Value) -> TimeZone {
    text(value, "timeZone")
        .and_then(|z| TimeZone::get(z).ok())
        .unwrap_or(TimeZone::UTC)
}

/// All-day events are midnight in the zone they were made in; the nearest
/// UTC midnight is the app's way of writing them.
fn nearest_midnight(seconds: i64) -> i64 {
    (seconds + 43_200).div_euclid(86_400) * 86_400
}

fn response(value: Option<&str>) -> Response {
    match value {
        Some("accepted") | Some("organizer") => Response::Accepted,
        Some("tentativelyAccepted") => Response::Tentative,
        Some("declined") => Response::Declined,
        _ => Response::NeedsAction,
    }
}

pub(crate) fn change(item: &Value) -> Option<RemoteChange> {
    let id = text(item, "id")?.to_owned();
    let instance = matches!(text(item, "type"), Some("occurrence") | Some("exception"));
    let all_day = item["isAllDay"].as_bool().unwrap_or(false);
    let mut starts = when(&item["start"])?;
    let mut ends = when(&item["end"]).unwrap_or(starts);
    if all_day {
        starts = nearest_midnight(starts);
        ends = nearest_midnight(ends).max(starts + 86_400);
    }
    let attendees: Vec<Attendee> = item["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| text(a, "type") != Some("resource"))
        .filter_map(|a| {
            let address = &a["emailAddress"];
            Some(Attendee {
                email: text(address, "address")?.to_owned(),
                name: text(address, "name").map(str::to_owned),
                response: response(text(&a["status"], "response")),
            })
        })
        .collect();
    let organizer = (!attendees.is_empty() && item["isOrganizer"].as_bool() != Some(true))
        .then(|| {
            let address = &item["organizer"]["emailAddress"];
            Some(mailbox(text(address, "address")?, text(address, "name")))
        })
        .flatten();
    let original_start = text(item, "originalStart")
        .and_then(|t| t.parse::<jiff::Timestamp>().ok())
        .map(|t| t.as_second());
    let event = Event {
        id: 0,
        calendar: 0,
        uid: if instance {
            String::new()
        } else {
            text(item, "iCalUId").unwrap_or_default().to_owned()
        },
        title: text(item, "subject").unwrap_or_default().to_owned(),
        location: text(&item["location"], "displayName")
            .unwrap_or_default()
            .to_owned(),
        notes: text(&item["body"], "content")
            .unwrap_or_default()
            .trim()
            .replace("\r\n", "\n"),
        starts,
        ends: ends.max(starts),
        all_day,
        tz: None,
        rrule: None,
        exdates: Vec::new(),
        response: organizer
            .as_ref()
            .map(|_| response(text(&item["responseStatus"], "response"))),
        organizer,
        attendees,
        reminder: item["isReminderOn"]
            .as_bool()
            .unwrap_or(false)
            .then(|| item["reminderMinutesBeforeStart"].as_i64())
            .flatten(),
        sequence: 0,
        cancelled: item["isCancelled"].as_bool().unwrap_or(false),
        series: None,
    };
    Some(RemoteChange::Changed(Box::new(RemoteEvent {
        id,
        etag: text(item, "changeKey").map(str::to_owned),
        series: instance
            .then(|| text(item, "seriesMasterId").map(str::to_owned))
            .flatten(),
        original_start: instance.then_some(original_start).flatten(),
        event,
    })))
}

const WEEKDAYS: &[(&str, &str)] = &[
    ("MO", "monday"),
    ("TU", "tuesday"),
    ("WE", "wednesday"),
    ("TH", "thursday"),
    ("FR", "friday"),
    ("SA", "saturday"),
    ("SU", "sunday"),
];

fn weekday(code: &str) -> Option<&'static str> {
    WEEKDAYS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
}

fn unsupported() -> Error {
    Error::Invalid("Outlook can't repeat an event this way; choose another repeat.".into())
}

/// An iCalendar rule as Graph's `patternedRecurrence`, starting at the
/// event's start in its zone.
pub(crate) fn recurrence(rule: &str, event: &Event) -> Result<Value> {
    let (zone, zone_name) = zone_of(event);
    let start = jiff::Timestamp::from_second(event.starts)
        .map_err(|_| unsupported())?
        .to_zoned(if event.all_day {
            TimeZone::UTC
        } else {
            zone.clone()
        });
    let mut parts = std::collections::HashMap::new();
    for part in rule.split(';') {
        if let Some((k, v)) = part.split_once('=') {
            parts.insert(k.trim().to_ascii_uppercase(), v.trim().to_ascii_uppercase());
        }
    }
    let interval: i64 = parts
        .get("INTERVAL")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let start_day = WEEKDAYS[start.weekday().to_monday_zero_offset() as usize].1;
    let days = |list: &str| -> Result<Vec<&'static str>> {
        list.split(',')
            .map(|d| {
                weekday(d.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit()))
                    .ok_or_else(unsupported)
            })
            .collect()
    };
    let pattern = match parts.get("FREQ").map(String::as_str) {
        Some("DAILY") => json!({ "type": "daily", "interval": interval }),
        Some("WEEKLY") => {
            let on = match parts.get("BYDAY") {
                Some(list) => days(list)?,
                None => vec![start_day],
            };
            json!({ "type": "weekly", "interval": interval, "daysOfWeek": on, "firstDayOfWeek": "monday" })
        }
        Some("MONTHLY") => match (parts.get("BYMONTHDAY"), parts.get("BYDAY")) {
            (Some(day), None) => json!({
                "type": "absoluteMonthly", "interval": interval,
                "dayOfMonth": day.parse::<i64>().map_err(|_| unsupported())?,
            }),
            (None, Some(by)) => {
                let (n, code) = by.split_at(by.len().saturating_sub(2));
                let index = match n {
                    "1" | "+1" => "first",
                    "2" | "+2" => "second",
                    "3" | "+3" => "third",
                    "4" | "+4" => "fourth",
                    "-1" => "last",
                    _ => return Err(unsupported()),
                };
                json!({
                    "type": "relativeMonthly", "interval": interval,
                    "daysOfWeek": [weekday(code).ok_or_else(unsupported)?], "index": index,
                })
            }
            (None, None) => {
                json!({ "type": "absoluteMonthly", "interval": interval, "dayOfMonth": start.day() })
            }
            _ => return Err(unsupported()),
        },
        Some("YEARLY") if !parts.contains_key("BYDAY") => json!({
            "type": "absoluteYearly", "interval": interval,
            "month": parts.get("BYMONTH").and_then(|m| m.parse::<i64>().ok()).unwrap_or(start.month() as i64),
            "dayOfMonth": parts.get("BYMONTHDAY").and_then(|d| d.parse::<i64>().ok()).unwrap_or(start.day() as i64),
        }),
        _ => return Err(unsupported()),
    };
    let start_date = start.strftime("%Y-%m-%d").to_string();
    let range = if let Some(count) = parts.get("COUNT") {
        json!({ "type": "numbered", "startDate": start_date,
                "numberOfOccurrences": count.parse::<i64>().map_err(|_| unsupported())? })
    } else if let Some(until) = parts.get("UNTIL") {
        let date = until.get(..8).ok_or_else(unsupported)?;
        json!({ "type": "endDate", "startDate": start_date,
                "endDate": format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..8]) })
    } else {
        json!({ "type": "noEnd", "startDate": start_date })
    };
    let mut recurrence = json!({ "pattern": pattern, "range": range });
    if !event.all_day {
        recurrence["range"]["recurrenceTimeZone"] = json!(zone_name);
    }
    Ok(recurrence)
}

fn time(seconds: i64, event: &Event) -> Value {
    if event.all_day {
        return json!({ "dateTime": civil(seconds, &TimeZone::UTC), "timeZone": "UTC" });
    }
    let (zone, name) = zone_of(event);
    json!({ "dateTime": civil(seconds, &zone), "timeZone": name })
}

pub(crate) fn body(event: &Event) -> Result<Value> {
    let mut body = json!({
        "subject": event.title,
        "body": { "contentType": "text", "content": event.notes },
        "location": { "displayName": event.location },
        "isAllDay": event.all_day,
        "start": time(event.starts, event),
        "end": time(event.ends, event),
        "isReminderOn": event.reminder.is_some(),
    });
    if let Some(minutes) = event.reminder {
        body["reminderMinutesBeforeStart"] = json!(minutes);
    }
    if event.series.is_none()
        && let Some(rule) = &event.rrule
    {
        body["recurrence"] = recurrence(rule, event)?;
    }
    Ok(body)
}

fn saved(body: &Value) -> Result<Saved> {
    Ok(Saved {
        id: text(body, "id")
            .ok_or_else(|| Error::Protocol("Outlook did not say which event it saved".into()))?
            .to_owned(),
        etag: text(body, "changeKey").map(str::to_owned),
    })
}

pub(crate) async fn create(api: &Api<'_>, calendar: &str, event: &Event) -> Result<Saved> {
    let body = body(event)?;
    let answer = api
        .send(
            Method::POST,
            &url(&["me", "calendars", calendar, "events"], &[])?,
            Some(&body),
            &[],
        )
        .await?
        .ok_or_else(|| Error::Rejected("Outlook: that calendar is gone".into()))?;
    saved(&answer)
}

pub(crate) async fn update(api: &Api<'_>, id: &str, event: &Event) -> Result<Option<Saved>> {
    let body = body(event)?;
    match api
        .send(
            Method::PATCH,
            &url(&["me", "events", id], &[])?,
            Some(&body),
            &[],
        )
        .await?
    {
        Some(answer) => saved(&answer).map(Some),
        None => Ok(None),
    }
}

pub(crate) async fn delete(api: &Api<'_>, id: &str) -> Result<()> {
    api.send(Method::DELETE, &url(&["me", "events", id], &[])?, None, &[])
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendars_get_a_colour_and_access() {
        let mine = calendar(&json!({"id": "A", "name": "Calendar", "color": "auto", "hexColor": "", "canEdit": true, "isDefaultCalendar": true})).unwrap();
        assert_eq!(
            (mine.color.as_str(), mine.read_only, mine.shown),
            ("#0078d4", false, true)
        );
        let holidays = calendar(
            &json!({"id": "B", "name": "Holidays", "color": "lightGreen", "canEdit": false}),
        )
        .unwrap();
        assert_eq!(
            (holidays.color.as_str(), holidays.read_only, holidays.shown),
            ("#5fb36b", true, false)
        );
    }

    #[test]
    fn occurrences_and_all_day_events_are_read() {
        let item = json!({
            "id": "AAMk-occ", "changeKey": "ck1", "iCalUId": "040000", "type": "occurrence",
            "seriesMasterId": "AAMk-master", "originalStart": "2026-10-20T07:00:00Z",
            "subject": "Standup", "body": {"contentType": "text", "content": "Notes\r\nhere"},
            "location": {"displayName": "Room 2"},
            "start": {"dateTime": "2026-10-20T07:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-10-20T07:15:00.0000000", "timeZone": "UTC"},
            "isAllDay": false, "isCancelled": false, "isOrganizer": true,
            "isReminderOn": true, "reminderMinutesBeforeStart": 15,
            "attendees": [{"type": "required", "emailAddress": {"address": "bo@work.test", "name": "Bo"}, "status": {"response": "accepted"}}]
        });
        let Some(RemoteChange::Changed(e)) = change(&item) else {
            panic!()
        };
        assert_eq!(
            (e.event.starts, e.event.ends),
            (1_792_479_600, 1_792_479_600 + 900)
        );
        assert_eq!(e.series.as_deref(), Some("AAMk-master"));
        assert_eq!(e.original_start, Some(1_792_479_600));
        assert_eq!(e.event.uid, "");
        assert_eq!(e.event.notes, "Notes\nhere");
        assert_eq!(e.event.reminder, Some(15));
        assert_eq!(e.event.organizer, None, "the user's own meeting");
        assert_eq!(e.etag.as_deref(), Some("ck1"));

        // Midnight in Berlin, given in UTC.
        let day = json!({
            "id": "x", "type": "singleInstance", "iCalUId": "uid-1", "isAllDay": true, "subject": "Trip",
            "start": {"dateTime": "2026-10-19T22:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-10-21T22:00:00.0000000", "timeZone": "UTC"},
            "isOrganizer": false, "organizer": {"emailAddress": {"address": "ann@work.test", "name": "Ann"}},
            "attendees": [{"emailAddress": {"address": "me@hotmail.com"}, "status": {"response": "none"}}],
            "responseStatus": {"response": "tentativelyAccepted"}
        });
        let Some(RemoteChange::Changed(d)) = change(&day) else {
            panic!()
        };
        assert_eq!(
            (d.event.starts, d.event.ends),
            (1_792_454_400, 1_792_454_400 + 2 * 86_400)
        );
        assert!(d.series.is_none());
        assert_eq!(d.event.uid, "uid-1");
        assert_eq!(d.event.organizer.as_deref(), Some("Ann <ann@work.test>"));
        assert_eq!(d.event.response, Some(Response::Tentative));
    }

    fn event(rrule: &str) -> Event {
        Event {
            id: 1,
            calendar: 1,
            uid: "u".into(),
            title: "Gym".into(),
            location: String::new(),
            notes: String::new(),
            // Tuesday 20 October 2026, 09:00 in Berlin.
            starts: 1_792_479_600,
            ends: 1_792_483_200,
            all_day: false,
            tz: Some("Europe/Berlin".into()),
            rrule: Some(rrule.into()),
            exdates: Vec::new(),
            organizer: None,
            attendees: Vec::new(),
            response: None,
            reminder: Some(10),
            sequence: 0,
            cancelled: false,
            series: None,
        }
    }

    #[test]
    fn repeat_rules_become_graph_patterns() {
        let e = event("FREQ=WEEKLY;BYDAY=MO,WE");
        let body = body(&e).unwrap();
        assert_eq!(
            body["start"],
            json!({"dateTime": "2026-10-20T09:00:00", "timeZone": "Europe/Berlin"})
        );
        assert_eq!(
            body["recurrence"]["pattern"]["daysOfWeek"],
            json!(["monday", "wednesday"])
        );
        assert_eq!(
            body["recurrence"]["range"],
            json!({"type": "noEnd", "startDate": "2026-10-20", "recurrenceTimeZone": "Europe/Berlin"})
        );
        assert_eq!(body["reminderMinutesBeforeStart"], 10);

        let weekly = recurrence("FREQ=WEEKLY", &e).unwrap();
        assert_eq!(weekly["pattern"]["daysOfWeek"], json!(["tuesday"]));
        let monthly = recurrence("FREQ=MONTHLY;BYMONTHDAY=20;COUNT=5", &e).unwrap();
        assert_eq!(monthly["pattern"]["dayOfMonth"], 20);
        assert_eq!(monthly["range"]["numberOfOccurrences"], 5);
        let third = recurrence("FREQ=MONTHLY;BYDAY=3TU;UNTIL=20271231T000000Z", &e).unwrap();
        assert_eq!(third["pattern"]["index"], "third");
        assert_eq!(third["range"]["endDate"], "2027-12-31");
        let yearly = recurrence("FREQ=YEARLY;INTERVAL=2", &e).unwrap();
        assert_eq!(
            (
                yearly["pattern"]["month"].as_i64(),
                yearly["pattern"]["interval"].as_i64()
            ),
            (Some(10), Some(2))
        );
        assert!(recurrence("FREQ=HOURLY", &e).is_err());
    }
}
