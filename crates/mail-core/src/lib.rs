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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    Other,
}

impl Security {
    /// Name stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Security::ImplicitTls => "implicit_tls",
            Security::StartTls => "starttls",
            Security::Insecure => "insecure",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "implicit_tls" => Some(Security::ImplicitTls),
            "starttls" => Some(Security::StartTls),
            "insecure" => Some(Security::Insecure),
            _ => None,
        }
    }
}

impl FolderRole {
    /// Name stored in the database; `None` for [`FolderRole::Other`].
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            FolderRole::Inbox => Some("inbox"),
            FolderRole::Sent => Some("sent"),
            FolderRole::Drafts => Some("drafts"),
            FolderRole::Trash => Some("trash"),
            FolderRole::Junk => Some("junk"),
            FolderRole::Archive => Some("archive"),
            FolderRole::Other => None,
        }
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("inbox") => FolderRole::Inbox,
            Some("sent") => FolderRole::Sent,
            Some("drafts") => FolderRole::Drafts,
            Some("trash") => FolderRole::Trash,
            Some("junk") => FolderRole::Junk,
            Some("archive") => FolderRole::Archive,
            _ => FolderRole::Other,
        }
    }

    /// Sidebar order: inbox first, then the special folders, then the rest.
    pub fn sort_rank(self) -> u8 {
        match self {
            FolderRole::Inbox => 0,
            FolderRole::Drafts => 1,
            FolderRole::Sent => 2,
            FolderRole::Archive => 3,
            FolderRole::Junk => 4,
            FolderRole::Trash => 5,
            FolderRole::Other => 6,
        }
    }
}

/// A configured mail account. The password lives in the OS credential store, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: AccountId,
    pub display_name: String,
    pub email: String,
    pub username: String,
    pub imap: Endpoint,
    pub smtp: Endpoint,
}

/// What is needed to create an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccount {
    pub display_name: String,
    pub email: String,
    pub username: String,
    pub imap: Endpoint,
    pub smtp: Endpoint,
}

/// A folder as the server lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFolder {
    pub name: String,
    pub delimiter: Option<String>,
    pub role: FolderRole,
    pub selectable: bool,
}

/// Header fields of one message, as shown in the message list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageHeader {
    pub uid: u32,
    pub message_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub subject: Option<String>,
    /// Seconds since the Unix epoch, from the Date header.
    pub date: Option<i64>,
    pub size: Option<u32>,
    pub flags: Flags,
    /// Guessed from a multipart/mixed Content-Type until bodies are fetched.
    pub has_attachments: bool,
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
    /// Headers fetched so far in one folder during a sync.
    SyncProgress {
        account: AccountId,
        folder: String,
        done: u32,
        total: u32,
    },
    SyncFinished {
        account: AccountId,
        error: Option<String>,
    },
}

impl Flags {
    /// Packs the flags into the integer stored in the database.
    pub fn to_bits(self) -> i64 {
        (self.seen as i64)
            | (self.flagged as i64) << 1
            | (self.answered as i64) << 2
            | (self.draft as i64) << 3
            | (self.deleted as i64) << 4
    }

    pub fn from_bits(bits: i64) -> Self {
        Flags {
            seen: bits & 1 != 0,
            flagged: bits & 2 != 0,
            answered: bits & 4 != 0,
            draft: bits & 8 != 0,
            deleted: bits & 16 != 0,
        }
    }
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

impl Serialize for Error {
    /// Errors cross to the UI as their message.
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

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

    #[test]
    fn flags_round_trip_through_bits() {
        let flags = Flags {
            seen: true,
            draft: true,
            ..Flags::default()
        };
        assert_eq!(Flags::from_bits(flags.to_bits()), flags);
        assert_eq!(Flags::from_bits(0), Flags::default());
    }

    #[test]
    fn database_names_round_trip() {
        for s in [
            Security::ImplicitTls,
            Security::StartTls,
            Security::Insecure,
        ] {
            assert_eq!(Security::parse(s.as_str()), Some(s));
        }
        for r in [FolderRole::Inbox, FolderRole::Sent, FolderRole::Other] {
            assert_eq!(FolderRole::parse(r.as_str()), r);
        }
    }
}
