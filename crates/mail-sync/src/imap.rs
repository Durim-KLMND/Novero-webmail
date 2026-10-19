//! A thin IMAP session over `async-imap`: connect, log in, list, select, fetch.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_imap::types::{Fetch, Flag, NameAttribute};
use futures::TryStreamExt;
use mail_core::{Endpoint, Error, Flags, FolderRole, Result, Security};
use rustls::pki_types::ServerName;
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Fields asked for when listing messages. Bodies are fetched separately.
const ENVELOPE_QUERY: &str = "(UID FLAGS RFC822.SIZE ENVELOPE)";

/// Any byte stream async-imap can run over (plain TCP or TLS).
trait Transport: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug> Transport for T {}

type Stream = Box<dyn Transport>;

/// A logged-in IMAP session.
pub struct ImapSession {
    session: async_imap::Session<Stream>,
}

/// A folder as the server lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    pub name: String,
    pub delimiter: Option<String>,
    pub role: FolderRole,
    pub selectable: bool,
}

/// Sync state of a selected folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderStatus {
    pub exists: u32,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
}

/// What the message list needs about one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeSummary {
    pub uid: u32,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub date: Option<String>,
    pub message_id: Option<String>,
    pub size: Option<u32>,
    pub flags: Flags,
}

impl ImapSession {
    /// Connects to `endpoint` and logs in with a password.
    pub async fn connect(endpoint: &Endpoint, username: &str, password: &str) -> Result<Self> {
        let tcp = timeout(
            CONNECT_TIMEOUT,
            TcpStream::connect((endpoint.host.as_str(), endpoint.port)),
        )
        .await
        .map_err(|_| Error::Network(format!("timed out connecting to {}", endpoint.host)))?
        .map_err(|e| Error::Network(e.to_string()))?;

        let stream: Stream = match endpoint.security {
            Security::Insecure => Box::new(tcp),
            Security::ImplicitTls => Box::new(tls(&endpoint.host, tcp).await?),
            Security::StartTls => {
                return Err(Error::Protocol("STARTTLS is not implemented yet".into()));
            }
        };

        let mut client = async_imap::Client::new(stream);
        client
            .read_response()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .ok_or_else(|| Error::Network("server closed the connection before greeting".into()))?;

        let session = client
            .login(username, password)
            .await
            .map_err(|(e, _)| Error::Auth(e.to_string()))?;
        Ok(Self { session })
    }

    /// Lists every folder, with its role from SPECIAL-USE when the server reports one.
    pub async fn list_folders(&mut self) -> Result<Vec<FolderInfo>> {
        let names: Vec<_> = self
            .session
            .list(Some(""), Some("*"))
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        Ok(names
            .iter()
            .map(|n| FolderInfo {
                name: n.name().to_owned(),
                delimiter: n.delimiter().map(str::to_owned),
                role: folder_role(n.name(), n.attributes()),
                selectable: !n.attributes().contains(&NameAttribute::NoSelect),
            })
            .collect())
    }

    /// Selects `folder` read-write and returns its sync state.
    pub async fn select(&mut self, folder: &str) -> Result<FolderStatus> {
        let mailbox = self.session.select(folder).await.map_err(protocol)?;
        Ok(FolderStatus {
            exists: mailbox.exists,
            uid_validity: mailbox.uid_validity,
            uid_next: mailbox.uid_next,
            highest_modseq: mailbox.highest_modseq,
        })
    }

    /// Fetches envelopes of the newest `limit` messages in `folder`, newest first.
    pub async fn fetch_latest_envelopes(
        &mut self,
        folder: &str,
        limit: u32,
    ) -> Result<Vec<EnvelopeSummary>> {
        let status = self.select(folder).await?;
        if status.exists == 0 || limit == 0 {
            return Ok(Vec::new());
        }
        let first = status.exists.saturating_sub(limit - 1).max(1);
        let range = format!("{first}:{}", status.exists);
        let fetched: Vec<Fetch> = self
            .session
            .fetch(range, ENVELOPE_QUERY)
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;

        let mut envelopes: Vec<_> = fetched.iter().filter_map(summarize).collect();
        envelopes.sort_by_key(|e| std::cmp::Reverse(e.uid));
        Ok(envelopes)
    }

    /// Ends the session politely.
    pub async fn logout(mut self) -> Result<()> {
        self.session.logout().await.map_err(protocol)
    }
}

async fn tls(host: &str, tcp: TcpStream) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|b| b.with_platform_verifier())
        .map_err(|e| Error::Network(e.to_string()))?
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned())
        .map_err(|e| Error::Network(format!("invalid host name {host}: {e}")))?;
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| Error::Network(e.to_string()))
}

fn protocol(err: async_imap::error::Error) -> Error {
    Error::Protocol(err.to_string())
}

/// Maps a LIST response to a folder role. INBOX is matched by name (RFC 3501 says it is
/// case-insensitive); everything else comes from SPECIAL-USE attributes (RFC 6154).
fn folder_role(name: &str, attributes: &[NameAttribute<'_>]) -> FolderRole {
    if name.eq_ignore_ascii_case("INBOX") {
        return FolderRole::Inbox;
    }
    for attribute in attributes {
        let role = match attribute {
            NameAttribute::Sent => FolderRole::Sent,
            NameAttribute::Drafts => FolderRole::Drafts,
            NameAttribute::Trash => FolderRole::Trash,
            NameAttribute::Junk => FolderRole::Junk,
            NameAttribute::Archive => FolderRole::Archive,
            _ => continue,
        };
        return role;
    }
    FolderRole::Other
}

/// Turns a FETCH response into a summary. Header words stay as sent: decoding
/// RFC 2047 encoded words belongs to `mail-mime`.
fn summarize(fetch: &Fetch) -> Option<EnvelopeSummary> {
    let uid = fetch.uid?;
    let envelope = fetch.envelope();
    let text = |bytes: Option<&[u8]>| bytes.map(|b| String::from_utf8_lossy(b).into_owned());
    let from = envelope
        .and_then(|e| e.from.as_ref())
        .and_then(|list| list.first())
        .map(|a| {
            let mailbox = text(a.mailbox.as_deref()).unwrap_or_default();
            let host = text(a.host.as_deref()).unwrap_or_default();
            match text(a.name.as_deref()) {
                Some(name) => format!("{name} <{mailbox}@{host}>"),
                None => format!("{mailbox}@{host}"),
            }
        });
    Some(EnvelopeSummary {
        uid,
        subject: text(envelope.and_then(|e| e.subject.as_deref())),
        from,
        date: text(envelope.and_then(|e| e.date.as_deref())),
        message_id: text(envelope.and_then(|e| e.message_id.as_deref())),
        size: fetch.size,
        flags: flags(fetch.flags()),
    })
}

fn flags<'a>(iter: impl Iterator<Item = Flag<'a>>) -> Flags {
    let mut flags = Flags::default();
    for flag in iter {
        match flag {
            Flag::Seen => flags.seen = true,
            Flag::Flagged => flags.flagged = true,
            Flag::Answered => flags.answered = true,
            Flag::Draft => flags.draft = true,
            Flag::Deleted => flags.deleted = true,
            _ => {}
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_is_matched_by_name_in_any_case() {
        assert_eq!(folder_role("inbox", &[]), FolderRole::Inbox);
        assert_eq!(folder_role("INBOX", &[]), FolderRole::Inbox);
    }

    #[test]
    fn special_use_attributes_set_the_role() {
        let attrs = [NameAttribute::Marked, NameAttribute::Sent];
        assert_eq!(folder_role("Sent Items", &attrs), FolderRole::Sent);
        assert_eq!(
            folder_role("Papierkorb", &[NameAttribute::Trash]),
            FolderRole::Trash
        );
        assert_eq!(folder_role("Projects", &[]), FolderRole::Other);
    }

    #[test]
    fn known_flags_are_kept_and_others_ignored() {
        let f = flags([Flag::Seen, Flag::Custom("$Label1".into()), Flag::Answered].into_iter());
        assert!(f.seen && f.answered && !f.flagged && !f.deleted);
    }
}
