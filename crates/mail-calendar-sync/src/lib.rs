//! Two-way sync with online calendars: Google Calendar, and Outlook.com and
//! Microsoft 365 calendars through Microsoft Graph.
//!
//! [`Client`] lists an account's calendars, pulls their events and sends
//! changes made in the app. Google keeps a repeating event as one series with
//! its rule, plus the occurrences that were moved; Graph lists each
//! occurrence on its own within a window of time, which the app keeps as
//! separate events tied to their series.

mod google;
mod http;
mod microsoft;

use std::time::Duration;

use mail_core::calendar::{Event, RemoteCalendar, RemoteChange};
use mail_core::{OAuthProvider, Result};

/// A pull of a calendar may page through a lot of events.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The events a pull found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
    pub changes: Vec<RemoteChange>,
    /// `changes` is everything the calendar has; events it leaves out are gone.
    pub complete: bool,
    /// Where the next pull starts, if the provider keeps track.
    pub token: Option<String>,
}

/// An event the server saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    pub etag: Option<String>,
}

/// An account's calendars, through its access token.
pub struct Client {
    provider: OAuthProvider,
    http: reqwest::Client,
    token: String,
}

impl Client {
    pub fn new(provider: OAuthProvider, access_token: &str) -> Result<Self> {
        Ok(Self {
            provider,
            http: mail_auth::https_client(TIMEOUT)?,
            token: access_token.to_owned(),
        })
    }

    pub(crate) fn http(&self) -> http::Api<'_> {
        http::Api {
            client: &self.http,
            token: &self.token,
            provider: self.provider,
        }
    }

    pub async fn calendars(&self) -> Result<Vec<RemoteCalendar>> {
        match self.provider {
            OAuthProvider::Google => google::calendars(&self.http()).await,
            OAuthProvider::Microsoft => microsoft::calendars(&self.http()).await,
        }
    }

    /// The calendar's events: changes since `since` (from the last pull's
    /// token), or all of them. `None` when the token is too old to use:
    /// pull everything again.
    pub async fn pull(&self, calendar: &str, since: Option<&str>) -> Result<Option<Pull>> {
        match self.provider {
            OAuthProvider::Google => google::pull(&self.http(), calendar, since).await,
            OAuthProvider::Microsoft => microsoft::pull(&self.http(), calendar, now())
                .await
                .map(Some),
        }
    }

    /// Adds an event. `moved` are starts of occurrences of the series kept
    /// as their own events, which aren't exceptions to send.
    pub async fn create(&self, calendar: &str, event: &Event, moved: &[i64]) -> Result<Saved> {
        match self.provider {
            OAuthProvider::Google => google::create(&self.http(), calendar, event, moved).await,
            OAuthProvider::Microsoft => microsoft::create(&self.http(), calendar, event).await,
        }
    }

    /// Changes an event; `None` when the server no longer has it.
    pub async fn update(
        &self,
        calendar: &str,
        id: &str,
        event: &Event,
        moved: &[i64],
    ) -> Result<Option<Saved>> {
        match self.provider {
            OAuthProvider::Google => google::update(&self.http(), calendar, id, event, moved).await,
            OAuthProvider::Microsoft => microsoft::update(&self.http(), id, event).await,
        }
    }

    /// Deletes an event, or a whole series. One already gone is fine.
    pub async fn delete(&self, calendar: &str, id: &str) -> Result<()> {
        match self.provider {
            OAuthProvider::Google => google::delete(&self.http(), calendar, id).await,
            OAuthProvider::Microsoft => microsoft::delete(&self.http(), id).await,
        }
    }

    /// Whether the app keeps this saved event as the server lists it, or the
    /// server lists it differently (Graph's repeating events come back as
    /// occurrences), so the calendar should be pulled in full.
    pub fn keeps_series(&self) -> bool {
        self.provider == OAuthProvider::Google
    }
}

fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// UTC seconds as a civil date and time in `zone` (`2026-10-20T09:00:00`).
pub(crate) fn civil(seconds: i64, zone: &jiff::tz::TimeZone) -> String {
    jiff::Timestamp::from_second(seconds)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(zone.clone())
        .strftime("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

/// The UTC date of an all-day event's time (`2026-10-20`).
pub(crate) fn utc_date(seconds: i64) -> String {
    civil(seconds, &jiff::tz::TimeZone::UTC)[..10].to_owned()
}

/// `2026-10-20` as UTC midnight.
pub(crate) fn date_seconds(text: &str) -> Option<i64> {
    let date: jiff::civil::Date = text.get(..10)?.parse().ok()?;
    Some(
        date.to_zoned(jiff::tz::TimeZone::UTC)
            .ok()?
            .timestamp()
            .as_second(),
    )
}

/// An event's zone, or UTC.
pub(crate) fn zone_of(event: &Event) -> (jiff::tz::TimeZone, String) {
    event
        .tz
        .as_deref()
        .and_then(|name| Some((jiff::tz::TimeZone::get(name).ok()?, name.to_owned())))
        .unwrap_or((jiff::tz::TimeZone::UTC, "UTC".to_owned()))
}

/// A person as the app writes organizers: `Ann Lee <ann@example.com>`.
pub(crate) fn mailbox(email: &str, name: Option<&str>) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty() && *n != email) {
        Some(name) => format!("{name} <{email}>"),
        None => email.to_owned(),
    }
}
