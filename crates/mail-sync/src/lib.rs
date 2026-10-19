//! IMAP sync engine.
//!
//! [`ImapSession`] wraps one IMAP connection. [`sync_account`] sends queued
//! local changes and keeps the cache in step with the server; [`push_changes`]
//! sends changes without a full sync.

pub mod imap;
mod sync;

pub use imap::{FolderStatus, ImapSession};
pub use mail_core::{Error, Result};
pub use sync::{
    ReplayOutcome, Scope, fetch_message, push_changes, replay_ops, sync_account, sync_folder,
};
