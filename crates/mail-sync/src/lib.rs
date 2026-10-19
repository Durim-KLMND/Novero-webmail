//! IMAP sync engine.
//!
//! Phase 0 holds a spike: [`imap::ImapSession`] logs in, lists folders and
//! fetches the newest envelopes. The per-account sync actor (phase 2) is
//! built on top of it.

pub mod imap;

pub use imap::{EnvelopeSummary, FolderInfo, FolderStatus, ImapSession};
pub use mail_core::{Error, Result};
