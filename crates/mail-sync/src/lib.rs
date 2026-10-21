//! IMAP sync engine.
//!
//! [`ImapSession`] wraps one IMAP connection. [`sync_account`] sends queued
//! local changes and keeps the cache in step with the server; [`push_changes`]
//! sends changes and saved drafts without a full sync.

mod drafts;
mod folders;
pub mod imap;
pub mod names;
mod sync;

pub use drafts::upload_drafts;
pub use folders::{create_folder, delete_folder, rename_folder};
pub use imap::{FolderStatus, IdleChange, ImapSession};
pub use mail_core::{Error, Result};
pub use sync::{
    NewMail, ReplayOutcome, Scope, SyncReport, cached_message, download_message, download_messages,
    fetch_message, push_changes, replay_ops, sync_account, sync_folder,
};
