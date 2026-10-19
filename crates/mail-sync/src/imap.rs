//! A thin IMAP session over `async-imap`: connect, log in, list, select, fetch.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_imap::types::{Fetch, Flag, NameAttribute};
use futures::TryStreamExt;
use mail_core::{
    Endpoint, Error, Flags, FolderRole, MessageHeader, RemoteFolder, Result, Security,
};
use rustls::pki_types::ServerName;
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Fields fetched for the message list. Bodies are fetched on open.
const HEADER_QUERY: &str = "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID CONTENT-TYPE)])";

/// Any byte stream async-imap can run over (plain TCP or TLS).
trait Transport: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug> Transport for T {}

type Stream = Box<dyn Transport>;

/// A logged-in IMAP session.
pub struct ImapSession {
    session: async_imap::Session<Stream>,
}

/// Sync state of a selected folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderStatus {
    pub exists: u32,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
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
        .map_err(|e| Error::Network(format!("could not reach {}: {e}", endpoint.host)))?;

        let client = match endpoint.security {
            Security::Insecure => greeted(Box::new(tcp)).await?,
            Security::ImplicitTls => greeted(Box::new(tls(&endpoint.host, tcp).await?)).await?,
            Security::StartTls => {
                let mut plain = async_imap::Client::new(tcp);
                read_greeting(&mut plain).await?;
                plain
                    .run_command_and_check_ok("STARTTLS", None)
                    .await
                    .map_err(|e| Error::Network(format!("STARTTLS refused: {e}")))?;
                let stream: Stream = Box::new(tls(&endpoint.host, plain.into_inner()).await?);
                // No second greeting after STARTTLS (RFC 3501 section 6.2.1).
                async_imap::Client::new(stream)
            }
        };

        let session = client
            .login(username, password)
            .await
            .map_err(|(e, _)| Error::Auth(e.to_string()))?;
        Ok(Self { session })
    }

    /// Lists every folder, with its role from SPECIAL-USE when the server reports one.
    pub async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>> {
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
            .map(|n| RemoteFolder {
                name: n.name().to_owned(),
                delimiter: n.delimiter().map(str::to_owned),
                role: folder_role(n.name(), n.attributes()),
                selectable: !n.attributes().contains(&NameAttribute::NoSelect),
            })
            .collect())
    }

    /// Selects `folder` and returns its sync state.
    pub async fn select(&mut self, folder: &str) -> Result<FolderStatus> {
        let mailbox = self.session.select(folder).await.map_err(protocol)?;
        Ok(FolderStatus {
            exists: mailbox.exists,
            uid_validity: mailbox.uid_validity,
            uid_next: mailbox.uid_next,
            highest_modseq: mailbox.highest_modseq,
        })
    }

    /// UIDs in the selected folder matching an IMAP search, e.g. `UID 1:99` or `ALL`.
    pub async fn search_uids(&mut self, query: &str) -> Result<Vec<u32>> {
        let mut uids: Vec<u32> = self
            .session
            .uid_search(query)
            .await
            .map_err(protocol)?
            .into_iter()
            .collect();
        uids.sort_unstable();
        Ok(uids)
    }

    /// Headers for a UID set in the selected folder, e.g. `5,7,9` or `100:*`.
    pub async fn fetch_headers(&mut self, uid_set: &str) -> Result<Vec<MessageHeader>> {
        let fetched: Vec<Fetch> = self
            .session
            .uid_fetch(uid_set, HEADER_QUERY)
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        Ok(fetched.iter().filter_map(header).collect())
    }

    /// Headers of the newest `limit` messages in `folder`, newest first.
    pub async fn fetch_latest_headers(
        &mut self,
        folder: &str,
        limit: u32,
    ) -> Result<Vec<MessageHeader>> {
        let status = self.select(folder).await?;
        if status.exists == 0 || limit == 0 {
            return Ok(Vec::new());
        }
        let first = status.exists.saturating_sub(limit - 1).max(1);
        let fetched: Vec<Fetch> = self
            .session
            .fetch(format!("{first}:{}", status.exists), HEADER_QUERY)
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        let mut headers: Vec<_> = fetched.iter().filter_map(header).collect();
        headers.sort_by_key(|h| std::cmp::Reverse(h.uid));
        Ok(headers)
    }

    /// The full raw message. Uses BODY.PEEK so the server does not mark it read.
    pub async fn fetch_raw(&mut self, folder: &str, uid: u32) -> Result<Vec<u8>> {
        self.select(folder).await?;
        let fetched: Vec<Fetch> = self
            .session
            .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        fetched
            .iter()
            .find(|f| f.uid == Some(uid))
            .and_then(|f| f.body())
            .map(<[u8]>::to_vec)
            .ok_or_else(|| Error::Protocol(format!("message {uid} is no longer on the server")))
    }

    /// Ends the session politely.
    pub async fn logout(mut self) -> Result<()> {
        self.session.logout().await.map_err(protocol)
    }
}

async fn greeted(stream: Stream) -> Result<async_imap::Client<Stream>> {
    let mut client = async_imap::Client::new(stream);
    read_greeting(&mut client).await?;
    Ok(client)
}

async fn read_greeting<T>(client: &mut async_imap::Client<T>) -> Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug,
{
    client
        .read_response()
        .await
        .map_err(|e| Error::Network(e.to_string()))?
        .ok_or_else(|| Error::Network("server closed the connection before greeting".into()))?;
    Ok(())
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
        .map_err(|e| Error::Network(format!("secure connection to {host} failed: {e}")))
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

/// Turns a FETCH response into a header row; fields are decoded by `mail-mime`.
fn header(fetch: &Fetch) -> Option<MessageHeader> {
    let uid = fetch.uid?;
    let parsed = fetch
        .header()
        .map(mail_mime::parse_header_fields)
        .unwrap_or_default();
    Some(MessageHeader {
        uid,
        size: fetch.size,
        flags: flags(fetch.flags()),
        ..parsed
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
