//! Calendars and their events, as the store keeps them and the UI shows them.
//!
//! Times are UTC seconds. An all-day event starts at midnight UTC of its first
//! day and ends at midnight UTC after its last day, so its dates read the same
//! anywhere. A timed event keeps the time zone it was made in (`tz`), so a
//! repeating event stays at the same wall-clock time across daylight saving.

use serde::{Deserialize, Serialize};

use crate::AccountId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    pub id: i64,
    pub name: String,
    /// A CSS colour, such as `#2e7d6b`.
    pub color: String,
    /// The account invitations to it are answered from, if any.
    pub account: Option<AccountId>,
    /// Shown in the calendar view.
    pub visible: bool,
    /// The online account it syncs with ([`CalendarAccount`]); `None` for a
    /// calendar kept only on this computer.
    #[serde(default)]
    pub source: Option<i64>,
    /// An online calendar the user may only look at, such as holidays.
    #[serde(default)]
    pub read_only: bool,
}

/// A Google or Microsoft account whose calendars sync with the app. Kept
/// apart from mail accounts: it signs in with calendar access only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarAccount {
    pub id: i64,
    pub provider: crate::OAuthProvider,
    pub email: String,
    /// When the last sync finished, in UTC seconds.
    pub synced_at: Option<i64>,
    /// Why the last sync failed, if it did.
    pub error: Option<String>,
    /// The sign-in expired or was revoked; signing in again fixes it.
    pub needs_sign_in: bool,
}

/// How someone answered an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Response {
    NeedsAction,
    Accepted,
    Tentative,
    Declined,
}

impl Response {
    /// The iCalendar `PARTSTAT` value.
    pub fn partstat(self) -> &'static str {
        match self {
            Response::NeedsAction => "NEEDS-ACTION",
            Response::Accepted => "ACCEPTED",
            Response::Tentative => "TENTATIVE",
            Response::Declined => "DECLINED",
        }
    }

    pub fn from_partstat(value: &str) -> Response {
        match value.to_ascii_uppercase().as_str() {
            "ACCEPTED" => Response::Accepted,
            "TENTATIVE" => Response::Tentative,
            "DECLINED" => Response::Declined,
            _ => Response::NeedsAction,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    pub email: String,
    pub name: Option<String>,
    pub response: Response,
}

/// An event; for a repeating one, its first occurrence and the rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// 0 for an event not saved yet.
    #[serde(default)]
    pub id: i64,
    pub calendar: i64,
    /// The iCalendar UID; made when saved if empty.
    #[serde(default)]
    pub uid: String,
    pub title: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub notes: String,
    pub starts: i64,
    pub ends: i64,
    pub all_day: bool,
    /// The IANA time zone of a timed event, such as `Europe/Berlin`.
    #[serde(default)]
    pub tz: Option<String>,
    /// An iCalendar RRULE value, such as `FREQ=WEEKLY;BYDAY=MO,WE`.
    #[serde(default)]
    pub rrule: Option<String>,
    /// Starts of occurrences taken out of the series.
    #[serde(default)]
    pub exdates: Vec<i64>,
    /// Who sent the invitation; empty for the user's own events.
    #[serde(default)]
    pub organizer: Option<String>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    /// The user's answer, for an invitation.
    #[serde(default)]
    pub response: Option<Response>,
    /// Minutes before the start to remind, if at all.
    #[serde(default)]
    pub reminder: Option<i64>,
    /// Bumped by the organizer with each change (iCalendar SEQUENCE).
    #[serde(default)]
    pub sequence: i64,
    #[serde(default)]
    pub cancelled: bool,
    /// For one occurrence of an online repeating event, kept as its own
    /// event: the series it belongs to. Deleting it deletes the series.
    #[serde(default)]
    pub series: Option<String>,
}

/// One time an event happens, as the calendar view draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub event: i64,
    pub calendar: i64,
    pub title: String,
    pub location: String,
    pub starts: i64,
    pub ends: i64,
    pub all_day: bool,
    pub repeats: bool,
    pub cancelled: bool,
    pub response: Option<Response>,
}

/// A calendar as an online account lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCalendar {
    pub id: String,
    pub name: String,
    pub color: String,
    pub read_only: bool,
    /// Shown when first added (the provider's own choice).
    pub shown: bool,
}

/// An event as an online calendar has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEvent {
    pub id: String,
    pub etag: Option<String>,
    /// For one occurrence of a series kept as its own event: the series' id.
    pub series: Option<String>,
    /// For such an occurrence: when it was due before it was moved.
    pub original_start: Option<i64>,
    /// `id` and `calendar` are 0; the store fills them in.
    pub event: Event,
}

/// What changed in an online calendar since the last look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteChange {
    Changed(Box<RemoteEvent>),
    /// Gone; for an occurrence of a series, that time is taken out of it.
    Removed {
        id: String,
        series: Option<String>,
        original_start: Option<i64>,
    },
}
