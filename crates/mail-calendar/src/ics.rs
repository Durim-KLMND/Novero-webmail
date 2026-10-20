//! Reading and writing iCalendar (RFC 5545) for invitations: what arrives
//! as a `text/calendar` part, and the replies and invitations sent back
//! (iTIP, RFC 5546). Only what mail invitations use is covered.

use std::collections::HashMap;

use jiff::civil::{Date, DateTime};
use jiff::tz::{Offset, TimeZone};
use jiff::{Timestamp, Zoned};
use mail_core::calendar::{Attendee, Event, Response};

/// What an invitation asks for (`METHOD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// A new or changed invitation.
    Request,
    /// Someone's answer to an invitation the user sent.
    Reply,
    /// The organizer called it off.
    Cancel,
    /// Just an event to add, with no answer expected.
    Publish,
}

/// An event from a `text/calendar` part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    pub method: Method,
    /// `calendar` is 0: the store picks one.
    pub event: Event,
}

/// One content line: `NAME;PARAM=value:VALUE`.
struct Line {
    name: String,
    params: HashMap<String, String>,
    value: String,
}

/// Joins folded lines (a line starting with a space or tab continues the one before).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
        } else if !raw.is_empty() {
            lines.push(raw.to_owned());
        }
    }
    lines
}

fn split_line(line: &str) -> Option<Line> {
    // The value starts at the first colon outside a quoted parameter.
    let mut quoted = false;
    let colon = line.char_indices().find(|&(_, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        c == ':' && !quoted
    })?;
    let (head, value) = (&line[..colon.0], &line[colon.0 + 1..]);
    let mut parts = Vec::new();
    let mut current = String::new();
    quoted = false;
    for c in head.chars() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    let name = parts.first()?.trim().to_ascii_uppercase();
    let params = parts[1..]
        .iter()
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim().to_owned()))
        .collect();
    Some(Line {
        name,
        params,
        value: value.to_owned(),
    })
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// `mailto:ann@example.com` → `ann@example.com`.
fn address(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix("mailto:")
        .or_else(|| value.strip_prefix("MAILTO:"))
        .unwrap_or(value)
        .to_owned()
}

/// Windows zone names Outlook writes, and their IANA zones.
const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("Romance Standard Time", "Europe/Paris"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("FLE Standard Time", "Europe/Kyiv"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("India Standard Time", "Asia/Kolkata"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("Eastern Standard Time", "America/New_York"),
    ("Central Standard Time", "America/Chicago"),
    ("Mountain Standard Time", "America/Denver"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("UTC", "UTC"),
];

/// The IANA zone for a TZID, if it names one directly or by its Windows name.
fn iana(tzid: &str) -> Option<String> {
    let tzid = tzid.trim().trim_matches('"');
    if TimeZone::get(tzid).is_ok() {
        return Some(tzid.to_owned());
    }
    WINDOWS_ZONES
        .iter()
        .find(|(windows, _)| windows.eq_ignore_ascii_case(tzid))
        .map(|(_, zone)| (*zone).to_owned())
}

/// Zones defined in the file (VTIMEZONE) by their standard-time offset, for
/// TZIDs no zone database knows.
type Offsets = HashMap<String, i32>;

fn zone_for(tzid: Option<&str>, offsets: &Offsets) -> (TimeZone, Option<String>) {
    match tzid {
        Some(id) => {
            if let Some(name) = iana(id)
                && let Ok(tz) = TimeZone::get(&name)
            {
                return (tz, Some(name));
            }
            if let Some(seconds) = offsets.get(id.trim_matches('"'))
                && let Ok(offset) = Offset::from_seconds(*seconds)
            {
                return (TimeZone::fixed(offset), None);
            }
            (TimeZone::system(), crate::system_zone())
        }
        None => (TimeZone::system(), crate::system_zone()),
    }
}

/// `20261019T090000Z` (UTC) or `20261019T090000` (in `tz`) as UTC seconds.
pub(crate) fn utc_or_local(value: &str, tz: &TimeZone) -> Option<i64> {
    let value = value.trim();
    let (text, utc) = match value.strip_suffix('Z') {
        Some(t) => (t, true),
        None => (value, false),
    };
    if text.len() < 15 {
        return None;
    }
    let iso = format!(
        "{}-{}-{}T{}:{}:{}",
        &text[..4],
        &text[4..6],
        &text[6..8],
        &text[9..11],
        &text[11..13],
        &text[13..15]
    );
    let civil: DateTime = iso.parse().ok()?;
    let zone = if utc { TimeZone::UTC } else { tz.clone() };
    Some(civil.to_zoned(zone).ok()?.timestamp().as_second())
}

fn date_value(value: &str) -> Option<Date> {
    let v = value.trim();
    if v.len() < 8 {
        return None;
    }
    format!("{}-{}-{}", &v[..4], &v[4..6], &v[6..8])
        .parse()
        .ok()
}

fn utc_midnight(date: Date) -> Option<i64> {
    Some(date.to_zoned(TimeZone::UTC).ok()?.timestamp().as_second())
}

/// A DTSTART/DTEND line: (UTC seconds, all-day, IANA zone).
fn when(line: &Line, offsets: &Offsets) -> Option<(i64, bool, Option<String>)> {
    let is_date = line
        .params
        .get("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("DATE"))
        || (line.value.trim().len() == 8);
    if is_date {
        return Some((utc_midnight(date_value(&line.value)?)?, true, None));
    }
    let (tz, name) = if line.value.trim().ends_with('Z') {
        (TimeZone::UTC, None)
    } else {
        zone_for(line.params.get("TZID").map(String::as_str), offsets)
    };
    Some((utc_or_local(&line.value, &tz)?, false, name))
}

/// `PT1H30M`, `P1D`, `-PT15M` as seconds.
fn duration(text: &str) -> Option<i64> {
    let text = text.trim();
    let (sign, rest) = match text.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut total = 0i64;
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'T' => {}
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += n * match unit {
                    'W' => 7 * 86_400,
                    'D' => 86_400,
                    'H' => 3_600,
                    'M' => 60,
                    'S' => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(sign * total)
}

/// The invitation in a `text/calendar` part, if it holds an event.
pub fn parse(text: &str) -> Option<Invitation> {
    let lines: Vec<Line> = unfold(text).iter().filter_map(|l| split_line(l)).collect();
    let mut method = Method::Publish;
    let mut offsets = Offsets::new();
    // VTIMEZONE: remember each zone's standard offset.
    let mut zone_id: Option<String> = None;
    let mut in_standard = false;
    let mut events: Vec<&[Line]> = Vec::new();
    let mut event_start = None;
    for (i, line) in lines.iter().enumerate() {
        match (
            line.name.as_str(),
            line.value.trim().to_ascii_uppercase().as_str(),
        ) {
            ("METHOD", m) => {
                method = match m {
                    "REQUEST" => Method::Request,
                    "REPLY" => Method::Reply,
                    "CANCEL" => Method::Cancel,
                    _ => Method::Publish,
                }
            }
            ("BEGIN", "VEVENT") => event_start = Some(i + 1),
            ("END", "VEVENT") => {
                if let Some(s) = event_start.take() {
                    events.push(&lines[s..i]);
                }
            }
            ("BEGIN", "STANDARD") => in_standard = true,
            ("END", "STANDARD") => in_standard = false,
            ("END", "VTIMEZONE") => zone_id = None,
            ("TZID", _) if event_start.is_none() => zone_id = Some(line.value.trim().to_owned()),
            ("TZOFFSETTO", _) if in_standard => {
                if let (Some(id), Some(seconds)) = (&zone_id, offset(&line.value)) {
                    offsets.insert(id.clone(), seconds);
                }
            }
            _ => {}
        }
    }
    // The series itself, not a changed single occurrence of it.
    let body = events
        .iter()
        .find(|e| !e.iter().any(|l| l.name == "RECURRENCE-ID"))
        .or(events.first())?;
    let mut event = Event {
        id: 0,
        calendar: 0,
        uid: String::new(),
        title: String::new(),
        location: String::new(),
        notes: String::new(),
        starts: 0,
        ends: 0,
        all_day: false,
        tz: None,
        rrule: None,
        exdates: Vec::new(),
        organizer: None,
        attendees: Vec::new(),
        response: None,
        reminder: None,
        sequence: 0,
        cancelled: method == Method::Cancel,
    };
    let mut start = None;
    let mut end = None;
    let mut length = None;
    let mut in_alarm = false;
    for line in body.iter() {
        match line.name.as_str() {
            "BEGIN" if line.value.eq_ignore_ascii_case("VALARM") => in_alarm = true,
            "END" if line.value.eq_ignore_ascii_case("VALARM") => in_alarm = false,
            "TRIGGER" if in_alarm => {
                if let Some(seconds) = duration(&line.value).filter(|s| *s <= 0) {
                    event.reminder = Some(-seconds / 60);
                }
            }
            _ if in_alarm => {}
            "UID" => event.uid = line.value.trim().to_owned(),
            "SUMMARY" => event.title = unescape(&line.value),
            "LOCATION" => event.location = unescape(&line.value),
            "DESCRIPTION" => event.notes = unescape(&line.value),
            "DTSTART" => start = when(line, &offsets),
            "DTEND" => end = when(line, &offsets),
            "DURATION" => length = duration(&line.value),
            "RRULE" => event.rrule = Some(line.value.trim().to_owned()),
            "EXDATE" => {
                for value in line.value.split(',') {
                    let one = Line {
                        name: String::new(),
                        params: line.params.clone(),
                        value: value.to_owned(),
                    };
                    if let Some((at, _, _)) = when(&one, &offsets) {
                        event.exdates.push(at);
                    }
                }
            }
            "SEQUENCE" => event.sequence = line.value.trim().parse().unwrap_or(0),
            "STATUS" => {
                if line.value.trim().eq_ignore_ascii_case("CANCELLED") {
                    event.cancelled = true;
                }
            }
            "ORGANIZER" => {
                let email = address(&line.value);
                event.organizer = Some(match line.params.get("CN") {
                    Some(name) if !name.is_empty() => format!("{name} <{email}>"),
                    _ => email,
                });
            }
            "ATTENDEE" => event.attendees.push(Attendee {
                email: address(&line.value),
                name: line.params.get("CN").filter(|n| !n.is_empty()).cloned(),
                response: line
                    .params
                    .get("PARTSTAT")
                    .map(|p| Response::from_partstat(p))
                    .unwrap_or(Response::NeedsAction),
            }),
            _ => {}
        }
    }
    let (starts, all_day, tz) = start?;
    event.starts = starts;
    event.all_day = all_day;
    event.tz = tz;
    event.ends = match (end, length) {
        (Some((e, _, _)), _) if e >= starts => e,
        (_, Some(len)) => starts + len.max(0),
        _ if all_day => starts + 86_400,
        _ => starts,
    };
    if event.uid.is_empty() {
        return None;
    }
    Some(Invitation { method, event })
}

/// `+0100` / `-0530` as seconds.
fn offset(text: &str) -> Option<i32> {
    let t = text.trim();
    let sign = if t.starts_with('-') { -1 } else { 1 };
    let digits = t.trim_start_matches(['+', '-']);
    if digits.len() < 4 {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..4].parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

/// Folds a content line at 75 octets, as iCalendar requires.
fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        let len = c.len_utf8();
        if width + len > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += len;
    }
    out.push_str("\r\n");
}

fn stamp(seconds: i64) -> String {
    Timestamp::from_second(seconds)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string()
}

/// DTSTART/DTEND/EXDATE lines for a time of `event`.
fn time_line(name: &str, seconds: i64, event: &Event) -> String {
    if event.all_day {
        let day = Timestamp::from_second(seconds)
            .unwrap_or(Timestamp::UNIX_EPOCH)
            .to_zoned(TimeZone::UTC)
            .strftime("%Y%m%d")
            .to_string();
        return format!("{name};VALUE=DATE:{day}");
    }
    // A repeating event keeps its zone, so it repeats at the same local time.
    if event.rrule.is_some()
        && let Some(zone) = event.tz.as_deref()
        && let Ok(tz) = TimeZone::get(zone)
        && let Ok(at) = Timestamp::from_second(seconds)
    {
        let local: Zoned = at.to_zoned(tz);
        return format!("{name};TZID={zone}:{}", local.strftime("%Y%m%dT%H%M%S"));
    }
    format!("{name}:{}", stamp(seconds))
}

/// A mailbox written as `Ann Lee <ann@example.com>` → (`ann@…`, `Ann Lee`).
fn split_mailbox(text: &str) -> (String, Option<String>) {
    match text.rsplit_once('<') {
        Some((name, rest)) => {
            let name = name.trim().trim_matches('"').trim();
            (
                rest.trim_end_matches('>').trim().to_owned(),
                (!name.is_empty()).then(|| name.to_owned()),
            )
        }
        None => (text.trim().to_owned(), None),
    }
}

fn person(prop: &str, email: &str, name: Option<&str>, extra: &str) -> String {
    let cn = name
        .map(|n| format!(";CN=\"{}\"", n.replace('"', "'")))
        .unwrap_or_default();
    format!("{prop}{cn}{extra}:mailto:{email}")
}

fn calendar(method: &str, body: &[String]) -> String {
    let mut out = String::new();
    for line in [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//Novero Mail//EN",
    ] {
        fold(line, &mut out);
    }
    fold(&format!("METHOD:{method}"), &mut out);
    for line in body {
        fold(line, &mut out);
    }
    fold("END:VCALENDAR", &mut out);
    out
}

/// An invitation to `event`'s attendees from `organizer` (`Name <email>`),
/// or its cancellation.
pub fn request(event: &Event, organizer: &str, cancel: bool, now: i64) -> String {
    let (org_email, org_name) = split_mailbox(organizer);
    let mut body = vec![
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}", event.uid),
        format!("DTSTAMP:{}", stamp(now)),
        format!("SEQUENCE:{}", event.sequence),
        time_line("DTSTART", event.starts, event),
        time_line("DTEND", event.ends, event),
        format!("SUMMARY:{}", escape(&event.title)),
    ];
    if !event.location.is_empty() {
        body.push(format!("LOCATION:{}", escape(&event.location)));
    }
    if !event.notes.is_empty() {
        body.push(format!("DESCRIPTION:{}", escape(&event.notes)));
    }
    if let Some(rule) = &event.rrule {
        body.push(format!("RRULE:{rule}"));
    }
    for ex in &event.exdates {
        body.push(time_line("EXDATE", *ex, event));
    }
    body.push(person("ORGANIZER", &org_email, org_name.as_deref(), ""));
    for a in &event.attendees {
        body.push(person(
            "ATTENDEE",
            &a.email,
            a.name.as_deref(),
            ";ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE",
        ));
    }
    body.push(format!(
        "STATUS:{}",
        if cancel { "CANCELLED" } else { "CONFIRMED" }
    ));
    body.push("END:VEVENT".to_owned());
    calendar(if cancel { "CANCEL" } else { "REQUEST" }, &body)
}

/// The user's answer to an invitation, from `me` (`Name <email>`).
pub fn reply(event: &Event, me: &str, response: Response, now: i64) -> String {
    let (email, name) = split_mailbox(me);
    let organizer = event.organizer.as_deref().map(split_mailbox);
    let mut body = vec![
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}", event.uid),
        format!("DTSTAMP:{}", stamp(now)),
        format!("SEQUENCE:{}", event.sequence),
        time_line("DTSTART", event.starts, event),
        time_line("DTEND", event.ends, event),
        format!("SUMMARY:{}", escape(&event.title)),
    ];
    if let Some((org_email, org_name)) = &organizer {
        body.push(person("ORGANIZER", org_email, org_name.as_deref(), ""));
    }
    body.push(person(
        "ATTENDEE",
        &email,
        name.as_deref(),
        &format!(";PARTSTAT={}", response.partstat()),
    ));
    body.push("END:VEVENT".to_owned());
    calendar("REPLY", &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTLOOK: &str = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nPRODID:Microsoft Exchange Server 2010\r\nVERSION:2.0\r\n\
BEGIN:VTIMEZONE\r\nTZID:W. Europe Standard Time\r\nBEGIN:STANDARD\r\nDTSTART:16010101T030000\r\nTZOFFSETFROM:+0200\r\n\
TZOFFSETTO:+0100\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\n\
END:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nORGANIZER;CN=\"Lee, Ann\":mailto:ann@work.test\r\n\
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Durim K:mailto:durim@example.test\r\n\
DESCRIPTION;LANGUAGE=en-US:Agenda:\\n1. Budget\\, travel\r\nUID:040000008200E00074C5B7101A82E008\r\n\
SUMMARY;LANGUAGE=en-US:Quarterly planning\r\nDTSTART;TZID=W. Europe Standard Time:20261021T140000\r\n\
DTEND;TZID=W. Europe Standard Time:20261021T150000\r\nSEQUENCE:2\r\nLOCATION;LANGUAGE=en-US:Room 4 / \r\n Teams\r\n\
RRULE:FREQ=WEEKLY;COUNT=4;BYDAY=WE\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

    fn utc(text: &str) -> i64 {
        text.parse::<Timestamp>().unwrap().as_second()
    }

    #[test]
    fn reads_an_outlook_invitation() {
        let inv = parse(OUTLOOK).unwrap();
        assert_eq!(inv.method, Method::Request);
        let e = inv.event;
        assert_eq!(e.uid, "040000008200E00074C5B7101A82E008");
        assert_eq!(e.title, "Quarterly planning");
        assert_eq!(e.location, "Room 4 / Teams");
        assert_eq!(e.notes, "Agenda:\n1. Budget, travel");
        assert_eq!(e.starts, utc("2026-10-21T12:00:00Z"));
        assert_eq!(e.ends - e.starts, 3600);
        assert_eq!(e.tz.as_deref(), Some("Europe/Berlin"));
        assert_eq!(e.rrule.as_deref(), Some("FREQ=WEEKLY;COUNT=4;BYDAY=WE"));
        assert_eq!(e.organizer.as_deref(), Some("Lee, Ann <ann@work.test>"));
        assert_eq!(e.attendees.len(), 1);
        assert_eq!(e.attendees[0].email, "durim@example.test");
        assert_eq!(e.attendees[0].name.as_deref(), Some("Durim K"));
        assert_eq!(e.sequence, 2);
        assert_eq!(e.reminder, Some(15));
        assert!(!e.all_day && !e.cancelled);
    }

    #[test]
    fn reads_all_day_utc_cancelled_and_unknown_zones() {
        let text = "BEGIN:VCALENDAR\nMETHOD:CANCEL\nBEGIN:VEVENT\nUID:a@b\nDTSTART;VALUE=DATE:20261224\nSUMMARY:Holiday\nEND:VEVENT\nEND:VCALENDAR\n";
        let inv = parse(text).unwrap();
        assert_eq!(inv.method, Method::Cancel);
        assert!(inv.event.cancelled && inv.event.all_day);
        assert_eq!(inv.event.starts, utc("2026-12-24T00:00:00Z"));
        assert_eq!(inv.event.ends, utc("2026-12-25T00:00:00Z"));

        let text = "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:Custom Zone\nBEGIN:STANDARD\nTZOFFSETTO:+0530\nEND:STANDARD\nEND:VTIMEZONE\n\
BEGIN:VEVENT\nUID:c@d\nDTSTART;TZID=Custom Zone:20261021T100000\nDURATION:PT45M\nEND:VEVENT\nEND:VCALENDAR";
        let e = parse(text).unwrap().event;
        assert_eq!(e.starts, utc("2026-10-21T04:30:00Z"));
        assert_eq!(e.ends - e.starts, 45 * 60);

        let utc_text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:e@f\nDTSTART:20261021T100000Z\nDTEND:20261021T110000Z\nEND:VEVENT\nEND:VCALENDAR";
        assert_eq!(
            parse(utc_text).unwrap().event.starts,
            utc("2026-10-21T10:00:00Z")
        );
        assert!(parse("BEGIN:VCALENDAR\nEND:VCALENDAR").is_none());
    }

    #[test]
    fn writes_replies_and_requests_that_read_back() {
        let inv = parse(OUTLOOK).unwrap();
        let text = reply(
            &inv.event,
            "Durim K <durim@example.test>",
            Response::Accepted,
            0,
        );
        assert!(text.contains("METHOD:REPLY\r\n"));
        assert!(
            text.contains("ATTENDEE;CN=\"Durim K\";PARTSTAT=ACCEPTED:mailto:durim@example.test")
        );
        assert!(text.contains("ORGANIZER;CN=\"Lee, Ann\":mailto:ann@work.test"));
        let back = parse(&text).unwrap();
        assert_eq!(back.method, Method::Reply);
        assert_eq!(back.event.uid, inv.event.uid);
        assert_eq!(back.event.attendees[0].response, Response::Accepted);
        assert_eq!(back.event.starts, inv.event.starts);

        let mut mine = inv.event.clone();
        mine.notes = "Long notes ".repeat(20);
        mine.attendees = vec![Attendee {
            email: "bo@example.test".into(),
            name: Some("Bo".into()),
            response: Response::NeedsAction,
        }];
        let text = request(&mine, "Durim K <durim@example.test>", false, 0);
        assert!(text.lines().all(|l| l.len() <= 75));
        assert!(text.contains("DTSTART;TZID=Europe/Berlin:20261021T140000"));
        let back = parse(&text).unwrap();
        assert_eq!(back.method, Method::Request);
        assert_eq!(back.event.notes, mine.notes);
        assert_eq!(back.event.starts, mine.starts);
        assert_eq!(
            back.event.organizer.as_deref(),
            Some("Durim K <durim@example.test>")
        );
        assert!(
            parse(&request(&mine, "durim@example.test", true, 0))
                .unwrap()
                .event
                .cancelled
        );
    }
}
