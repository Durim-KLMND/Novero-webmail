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

/// A flag the user can change on a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlagKind {
    Seen,
    Flagged,
    /// Set on a message once a reply to it is sent.
    Answered,
}

impl FlagKind {
    /// The IMAP system flag.
    pub fn imap_name(self) -> &'static str {
        match self {
            FlagKind::Seen => "\\Seen",
            FlagKind::Flagged => "\\Flagged",
            FlagKind::Answered => "\\Answered",
        }
    }

    /// The bit in [`Flags::to_bits`].
    pub fn bit(self) -> i64 {
        match self {
            FlagKind::Seen => 1,
            FlagKind::Flagged => 2,
            FlagKind::Answered => 4,
        }
    }
}

/// A change made locally that still has to reach the server.
///
/// Messages are addressed by folder name, UIDVALIDITY and UID, so an operation
/// stays valid while offline and is dropped if the folder was rebuilt meanwhile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Op {
    SetFlag {
        folder: String,
        uid_validity: Option<u32>,
        uids: Vec<u32>,
        flag: FlagKind,
        value: bool,
    },
    /// Move to another folder of the same account.
    Move {
        folder: String,
        uid_validity: Option<u32>,
        uids: Vec<u32>,
        to: String,
    },
    /// Delete permanently.
    Expunge {
        folder: String,
        uid_validity: Option<u32>,
        uids: Vec<u32>,
    },
}

impl Op {
    /// The folder the operation acts on.
    pub fn folder(&self) -> &str {
        match self {
            Op::SetFlag { folder, .. } | Op::Move { folder, .. } | Op::Expunge { folder, .. } => {
                folder
            }
        }
    }

    pub fn uid_validity(&self) -> Option<u32> {
        match self {
            Op::SetFlag { uid_validity, .. }
            | Op::Move { uid_validity, .. }
            | Op::Expunge { uid_validity, .. } => *uid_validity,
        }
    }

    pub fn uids(&self) -> &[u32] {
        match self {
            Op::SetFlag { uids, .. } | Op::Move { uids, .. } | Op::Expunge { uids, .. } => uids,
        }
    }

    /// Short name stored in the queue's `kind` column.
    pub fn kind(&self) -> &'static str {
        match self {
            Op::SetFlag { .. } => "set_flag",
            Op::Move { .. } => "move",
            Op::Expunge { .. } => "expunge",
        }
    }
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
    /// Local changes still waiting for the server, and why the last attempt failed.
    PendingChanges {
        account: AccountId,
        pending: u32,
        error: Option<String>,
    },
    /// Messages changed without a full sync (a local change or a server push).
    MailboxChanged {
        account: AccountId,
    },
    /// Messages waiting to send, those the server refused, and why the last
    /// attempt failed if it did.
    OutboxChanged {
        account: AccountId,
        queued: u32,
        failed: u32,
        error: Option<String>,
    },
}

/// The parts of a message being written that the user edits. Addresses are
/// kept as typed (comma separated) and checked only when sending.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftFields {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
}

/// A file attached to a draft. The file itself is kept by the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftAttachment {
    pub name: String,
    pub mime_type: String,
    pub size: u64,
}

/// A message being written: saved locally as the user types, copied to the
/// Drafts folder, and turned into outgoing mail when sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: i64,
    pub account: AccountId,
    /// Message-ID without angle brackets. It stays the same across saves, so
    /// the copy in the Drafts folder can be found and replaced.
    pub message_id: String,
    #[serde(flatten)]
    pub fields: DraftFields,
    pub attachments: Vec<DraftAttachment>,
    /// Threading headers of a reply, without angle brackets.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// The cached message this replies to, marked answered once sent.
    pub replying_to: Option<i64>,
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
    /// The server answered NO to a command, e.g. a folder that no longer exists.
    #[error("the server refused: {0}")]
    Rejected(String),
    #[error("storage error: {0}")]
    Storage(String),
    /// Something the user entered cannot be used, e.g. a malformed address.
    #[error("{0}")]
    Invalid(String),
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
    fn ops_serialize_with_a_kind_tag() {
        let op = Op::SetFlag {
            folder: "INBOX".into(),
            uid_validity: Some(7),
            uids: vec![1, 2],
            flag: FlagKind::Seen,
            value: true,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.starts_with(r#"{"kind":"setFlag""#), "{json}");
        assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
        assert_eq!(op.folder(), "INBOX");
        assert_eq!(
            FlagKind::Flagged.bit(),
            Flags {
                flagged: true,
                ..Flags::default()
            }
            .to_bits()
        );
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
