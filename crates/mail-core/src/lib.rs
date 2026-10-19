//! Types shared by every crate in the mail client.

use serde::{Deserialize, Serialize};

/// Local identifier of a configured account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub i64);

/// Local identifier of a mailbox folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FolderId(pub i64);

/// How a connection is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    ImplicitTls,
    /// Plain connection upgraded with STARTTLS (IMAP 143, SMTP 587).
    StartTls,
    /// No encryption. Only allowed behind an explicit per-account switch.
    Insecure,
}

/// A server endpoint for IMAP or SMTP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// Role of a folder, from IMAP SPECIAL-USE (RFC 6154).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    Other,
}

/// Message flags the client tracks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
    pub deleted: bool,
}

/// Events the core sends to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    SyncStarted {
        account: AccountId,
    },
    SyncFinished {
        account: AccountId,
    },
    FolderChanged {
        account: AccountId,
        folder: FolderId,
    },
    Error {
        account: Option<AccountId>,
        message: String,
    },
}

/// Errors shared across crates.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("storage error: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_with_a_type_tag() {
        let json = serde_json::to_string(&Event::SyncStarted {
            account: AccountId(1),
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"syncStarted","account":1}"#);
    }
}
