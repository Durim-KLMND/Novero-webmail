//! Expanding an event's repeat rule (a subset of iCalendar RRULE: DAILY,
//! WEEKLY with BYDAY, MONTHLY by day of month or nth weekday, YEARLY, with
//! INTERVAL, COUNT and UNTIL) into the times it happens.

use jiff::civil::{Date, DateTime, Weekday};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned};
use mail_core::calendar::Event;

/// Rules are followed this many periods at most, so a bad one can't hang.
const MAX_PERIODS: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<u32>,
    until: Option<i64>,
    /// Weekdays, with an optional position in the month (`2TU`, `-1FR`).
    by_day: Vec<(Option<i8>, Weekday)>,
    by_month_day: Vec<i8>,
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

/// UNTIL as UTC seconds: a date means the end of that day.
fn until(value: &str, tz: &TimeZone) -> Option<i64> {
    let value = value.trim();
    if value.len() == 8 {
        let date: Date = format!("{}-{}-{}", &value[..4], &value[4..6], &value[6..])
            .parse()
            .ok()?;
        let end = date.tomorrow().ok()?.to_zoned(tz.clone()).ok()?;
        return Some(end.timestamp().as_second() - 1);
    }
    crate::ics::utc_or_local(value, tz)
}

fn parse_rule(text: &str, tz: &TimeZone) -> Option<Rule> {
    let mut rule = Rule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
    };
    let mut freq = None;
    for part in text.split(';') {
        let (key, value) = part.split_once('=')?;
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match value.to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                })
            }
            "INTERVAL" => rule.interval = value.parse::<i64>().ok()?.max(1),
            "COUNT" => rule.count = value.parse().ok(),
            "UNTIL" => rule.until = until(value, tz),
            "BYDAY" => {
                for day in value.split(',') {
                    let day = day.trim().to_ascii_uppercase();
                    let (nth, code) = day.split_at(day.len().saturating_sub(2));
                    let nth = if nth.is_empty() {
                        None
                    } else {
                        Some(nth.parse().ok()?)
                    };
                    rule.by_day.push((nth, weekday(code)?));
                }
            }
            "BYMONTHDAY" => {
                for day in value.split(',') {
                    rule.by_month_day.push(day.trim().parse().ok()?);
                }
            }
            _ => {}
        }
    }
    rule.freq = freq?;
    Some(rule)
}

/// The days a period (the `n`th since the first) has an occurrence on.
fn days_in_period(rule: &Rule, first: Date, n: i64) -> Vec<Date> {
    let step = n * rule.interval;
    let mut days = match rule.freq {
        Freq::Daily => first
            .checked_add(Span::new().days(step))
            .into_iter()
            .collect(),
        Freq::Weekly => {
            let monday = first
                .checked_sub(Span::new().days(i64::from(first.weekday().to_monday_zero_offset())))
                .and_then(|d| d.checked_add(Span::new().weeks(step)));
            let Ok(monday) = monday else {
                return Vec::new();
            };
            let wanted: Vec<Weekday> = if rule.by_day.is_empty() {
                vec![first.weekday()]
            } else {
                rule.by_day.iter().map(|(_, d)| *d).collect()
            };
            wanted
                .into_iter()
                .filter_map(|d| {
                    monday
                        .checked_add(Span::new().days(i64::from(d.to_monday_zero_offset())))
                        .ok()
                })
                .collect()
        }
        Freq::Monthly => {
            let Ok(month) = first.first_of_month().checked_add(Span::new().months(step)) else {
                return Vec::new();
            };
            if !rule.by_month_day.is_empty() {
                rule.by_month_day
                    .iter()
                    .filter_map(|d| month_day(month, *d))
                    .collect()
            } else if !rule.by_day.is_empty() {
                let mut days = Vec::new();
                for (nth, day) in &rule.by_day {
                    match nth {
                        Some(nth) => days.extend(month.nth_weekday_of_month(*nth, *day).ok()),
                        None => (1..=5)
                            .for_each(|i| days.extend(month.nth_weekday_of_month(i, *day).ok())),
                    }
                }
                days
            } else {
                month_day(month, first.day()).into_iter().collect()
            }
        }
        Freq::Yearly => Date::new(
            first.year().saturating_add(step as i16),
            first.month(),
            first.day(),
        )
        .into_iter()
        .collect(),
    };
    days.sort();
    days.dedup();
    days
}

/// Day `day` of `month` (negative counts from the end), if it has one.
fn month_day(month: Date, day: i8) -> Option<Date> {
    let len = month.days_in_month();
    let day = if day < 0 { len + 1 + day } else { day };
    if day < 1 || day > len {
        return None;
    }
    Date::new(month.year(), month.month(), day).ok()
}

/// The (start, end) of each time `event` happens that overlaps `from..to`,
/// leaving out removed occurrences.
pub fn occurrences(event: &Event, from: i64, to: i64) -> Vec<(i64, i64)> {
    let length = (event.ends - event.starts).max(0);
    let overlaps = |s: i64| s < to && s + length.max(1) > from;
    let Some(rule_text) = event.rrule.as_deref().filter(|r| !r.trim().is_empty()) else {
        return if overlaps(event.starts) {
            vec![(event.starts, event.ends)]
        } else {
            Vec::new()
        };
    };
    let tz = zone(event);
    let Some(rule) = parse_rule(rule_text, &tz) else {
        return if overlaps(event.starts) {
            vec![(event.starts, event.ends)]
        } else {
            Vec::new()
        };
    };
    let Ok(first) = Timestamp::from_second(event.starts).map(|t| t.to_zoned(tz.clone())) else {
        return Vec::new();
    };
    let time = first.time();
    let start_date = first.date();
    let mut found = Vec::new();
    let mut counted = 0u32;
    'periods: for n in 0..MAX_PERIODS {
        for day in days_in_period(&rule, start_date, n) {
            if day < start_date {
                continue;
            }
            let Some(start) = at(day, time, &tz) else {
                continue;
            };
            if start < event.starts {
                continue;
            }
            if rule.until.is_some_and(|u| start > u) || rule.count.is_some_and(|c| counted >= c) {
                break 'periods;
            }
            counted += 1;
            if start >= to {
                break 'periods;
            }
            if overlaps(start) && !event.exdates.contains(&start) {
                found.push((start, start + length));
            }
        }
    }
    found
}

/// When the last occurrence ends, or `None` for a series without end.
pub fn last_end(event: &Event) -> Option<i64> {
    let Some(rule) = event.rrule.as_deref().filter(|r| !r.trim().is_empty()) else {
        return Some(event.ends);
    };
    match parse_rule(rule, &zone(event)) {
        None => Some(event.ends),
        Some(r) if r.count.is_none() && r.until.is_none() => None,
        Some(_) => {
            let far = event.starts + 200 * 366 * 86_400;
            occurrences(event, event.starts, far)
                .last()
                .map(|(_, end)| *end)
                .or(Some(event.ends))
        }
    }
}

/// A wall-clock time on `day` in `tz`, as UTC seconds.
fn at(day: Date, time: jiff::civil::Time, tz: &TimeZone) -> Option<i64> {
    let zoned: Zoned = DateTime::from_parts(day, time).to_zoned(tz.clone()).ok()?;
    Some(zoned.timestamp().as_second())
}

/// The zone an event repeats in: UTC for all-day events (their times are
/// UTC midnights), else its own zone, else the computer's.
fn zone(event: &Event) -> TimeZone {
    if event.all_day {
        return TimeZone::UTC;
    }
    event
        .tz
        .as_deref()
        .and_then(|name| TimeZone::get(name).ok())
        .unwrap_or_else(TimeZone::system)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(text: &str) -> i64 {
        text.parse::<Zoned>().unwrap().timestamp().as_second()
    }

    fn event(starts: &str, minutes: i64, rrule: &str) -> Event {
        let starts = ts(starts);
        Event {
            id: 1,
            calendar: 1,
            uid: "x".into(),
            title: "Standup".into(),
            location: String::new(),
            notes: String::new(),
            starts,
            ends: starts + minutes * 60,
            all_day: false,
            tz: Some("Europe/Berlin".into()),
            rrule: Some(rrule.into()),
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

    fn starts(event: &Event, from: &str, to: &str) -> Vec<String> {
        occurrences(event, ts(from), ts(to))
            .into_iter()
            .map(|(s, _)| {
                Timestamp::from_second(s)
                    .unwrap()
                    .to_zoned(TimeZone::get("Europe/Berlin").unwrap())
                    .strftime("%a %Y-%m-%d %H:%M")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn weekly_on_days_keeps_wall_clock_time_across_daylight_saving() {
        let e = event(
            "2026-10-19T09:00[Europe/Berlin]",
            15,
            "FREQ=WEEKLY;BYDAY=MO,TH",
        );
        assert_eq!(
            starts(
                &e,
                "2026-10-19T00:00[Europe/Berlin]",
                "2026-11-03T00:00[Europe/Berlin]"
            ),
            [
                "Mon 2026-10-19 09:00",
                "Thu 2026-10-22 09:00",
                "Mon 2026-10-26 09:00",
                "Thu 2026-10-29 09:00",
                "Mon 2026-11-02 09:00"
            ]
        );
    }

    #[test]
    fn count_until_interval_and_removed_dates() {
        let mut e = event(
            "2026-01-01T08:00[Europe/Berlin]",
            30,
            "FREQ=DAILY;INTERVAL=2;COUNT=3",
        );
        assert_eq!(
            starts(
                &e,
                "2025-12-01T00:00[Europe/Berlin]",
                "2027-01-01T00:00[Europe/Berlin]"
            ),
            [
                "Thu 2026-01-01 08:00",
                "Sat 2026-01-03 08:00",
                "Mon 2026-01-05 08:00"
            ]
        );
        e.rrule = Some("FREQ=DAILY;UNTIL=20260103".into());
        e.exdates = vec![ts("2026-01-02T08:00[Europe/Berlin]")];
        assert_eq!(
            starts(
                &e,
                "2025-12-01T00:00[Europe/Berlin]",
                "2027-01-01T00:00[Europe/Berlin]"
            ),
            ["Thu 2026-01-01 08:00", "Sat 2026-01-03 08:00"]
        );
    }

    #[test]
    fn monthly_by_position_and_day_and_yearly() {
        let e = event(
            "2026-01-27T10:00[Europe/Berlin]",
            60,
            "FREQ=MONTHLY;BYDAY=-1TU;COUNT=3",
        );
        assert_eq!(
            starts(
                &e,
                "2026-01-01T00:00[Europe/Berlin]",
                "2027-01-01T00:00[Europe/Berlin]"
            ),
            [
                "Tue 2026-01-27 10:00",
                "Tue 2026-02-24 10:00",
                "Tue 2026-03-31 10:00"
            ]
        );
        let e = event(
            "2026-01-31T10:00[Europe/Berlin]",
            60,
            "FREQ=MONTHLY;COUNT=3",
        );
        // Months without a 31st are skipped, as iCalendar says.
        assert_eq!(
            starts(
                &e,
                "2026-01-01T00:00[Europe/Berlin]",
                "2027-01-01T00:00[Europe/Berlin]"
            ),
            [
                "Sat 2026-01-31 10:00",
                "Tue 2026-03-31 10:00",
                "Sun 2026-05-31 10:00"
            ]
        );
        let e = event("2024-02-29T10:00[Europe/Berlin]", 60, "FREQ=YEARLY;COUNT=2");
        assert_eq!(
            starts(
                &e,
                "2024-01-01T00:00[Europe/Berlin]",
                "2030-01-01T00:00[Europe/Berlin]"
            ),
            ["Thu 2024-02-29 10:00", "Tue 2028-02-29 10:00"]
        );
    }

    #[test]
    fn last_end_of_a_series() {
        let e = event("2026-01-01T08:00[Europe/Berlin]", 30, "FREQ=DAILY;COUNT=3");
        assert_eq!(last_end(&e), Some(ts("2026-01-03T08:30[Europe/Berlin]")));
        assert_eq!(
            last_end(&event("2026-01-01T08:00[Europe/Berlin]", 30, "FREQ=WEEKLY")),
            None
        );
        let mut single = e.clone();
        single.rrule = None;
        assert_eq!(last_end(&single), Some(single.ends));
    }

    #[test]
    fn only_the_asked_range_and_single_events() {
        let e = event("2026-10-19T09:00[Europe/Berlin]", 60, "FREQ=DAILY");
        assert_eq!(
            starts(
                &e,
                "2027-03-01T09:30[Europe/Berlin]",
                "2027-03-02T09:00[Europe/Berlin]"
            ),
            ["Mon 2027-03-01 09:00"]
        );
        let mut single = e.clone();
        single.rrule = None;
        assert_eq!(
            occurrences(&single, single.starts - 10, single.starts + 10).len(),
            1
        );
        assert!(occurrences(&single, single.ends, single.ends + 10).is_empty());
    }
}
