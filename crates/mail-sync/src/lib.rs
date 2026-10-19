//! IMAP sync engine.
//!
//! [`ImapSession`] wraps one IMAP connection; [`sync_account`] keeps the local
//! cache in step with the server. IDLE, flag sync and deletions are phase 2.

pub mod imap;
mod sync;

pub use imap::{FolderStatus, ImapSession};
pub use mail_core::{Error, Result};
pub use sync::{fetch_message, sync_account};
