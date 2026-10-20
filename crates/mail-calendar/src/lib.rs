//! Calendar logic without storage: reading invitations (.ics), writing
//! invitations and replies, and expanding repeating events into occurrences.

pub mod ics;
pub mod recur;

pub use ics::{Invitation, Method, parse, reply, request};
pub use recur::{last_end, occurrences};

/// The time zone the computer is set to, as an IANA name, if it has one.
pub fn system_zone() -> Option<String> {
    jiff::tz::TimeZone::system().iana_name().map(str::to_owned)
}

/// A new, globally unique event UID.
pub fn new_uid() -> String {
    format!("{}@noveromail", uuid::Uuid::new_v4())
}
