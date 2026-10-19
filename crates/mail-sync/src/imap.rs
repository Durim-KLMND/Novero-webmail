//! A thin IMAP session over `async-imap`: connect, log in, list, select, fetch,
//! change flags, move, delete and IDLE.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_imap::extensions::idle::IdleResponse;
use async_imap::types::{Capabilities, Fetch, Flag, NameAttribute};
use futures::TryStreamExt;
use mail_core::{
    Endpoint, Error, FlagKind, Flags, FolderRole, MessageHeader, RemoteFolder, Result, Secret,
    Security,
};
use rustls::pki_types::ServerName;
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Fields fetched for the message list. Bodies are fetched on open.
const HEADER_QUERY: &str = "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID CONTENT-TYPE REFERENCES IN-REPLY-TO)])";

/// Any byte stream async-imap can run over (plain TCP or TLS).
trait Transport: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug> Transport for T {}

type Stream = Box<dyn Transport>;

/// A logged-in IMAP session.
pub struct ImapSession {
    session: async_imap::Session<Stream>,
    caps: Capabilities,
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
    /// Connects to `endpoint` and logs in with a password, or with an OAuth2
    /// access token (SASL XOAUTH2).
    pub async fn connect(endpoint: &Endpoint, username: &str, secret: &Secret) -> Result<Self> {
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

        let mut session = match secret {
            Secret::Password(password) => client
                .login(username, password)
                .await
                .map_err(|(e, _)| refused(e))?,
            Secret::AccessToken(token) => client
                .authenticate("XOAUTH2", XOAuth2::new(username, token))
                .await
                .map_err(|(e, _)| refused(e))?,
        };
        let caps = session.capabilities().await.map_err(protocol)?;
        Ok(Self { session, caps })
    }

    /// Whether the server advertised a capability, e.g. `CONDSTORE` or `MOVE`.
    pub fn has(&self, capability: &str) -> bool {
        self.caps.has_str(capability)
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

    /// Selects `folder` and returns its sync state. With CONDSTORE the status
    /// carries HIGHESTMODSEQ, so flag changes can be fetched incrementally.
    pub async fn select(&mut self, folder: &str) -> Result<FolderStatus> {
        let mailbox = if self.has("CONDSTORE") {
            self.session.select_condstore(folder).await
        } else {
            self.session.select(folder).await
        }
        .map_err(protocol)?;
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

    /// Flags of messages in the selected folder. With `changed_since` (CONDSTORE)
    /// only messages whose flags changed after that mod-sequence are returned.
    pub async fn fetch_flags(
        &mut self,
        uid_set: &str,
        changed_since: Option<u64>,
    ) -> Result<Vec<(u32, Flags)>> {
        let query = match changed_since {
            Some(modseq) => format!("(UID FLAGS) (CHANGEDSINCE {modseq})"),
            None => "(UID FLAGS)".to_owned(),
        };
        let fetched: Vec<Fetch> = self
            .session
            .uid_fetch(uid_set, query)
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        Ok(fetched
            .iter()
            .filter_map(|f| Some((f.uid?, flags(f.flags()))))
            .collect())
    }

    /// Sets or clears a flag on messages in the selected folder.
    pub async fn set_flag(&mut self, uid_set: &str, flag: FlagKind, value: bool) -> Result<()> {
        let sign = if value { '+' } else { '-' };
        self.store(
            uid_set,
            &format!("{sign}FLAGS.SILENT ({})", flag.imap_name()),
        )
        .await
    }

    /// Moves messages from the selected folder to `to`. Uses MOVE (RFC 6851)
    /// when offered, otherwise COPY, flag deleted and expunge.
    pub async fn move_uids(&mut self, uid_set: &str, to: &str) -> Result<()> {
        if self.has("MOVE") {
            return self.session.uid_mv(uid_set, to).await.map_err(protocol);
        }
        self.session.uid_copy(uid_set, to).await.map_err(protocol)?;
        self.expunge_uids(uid_set).await
    }

    /// Deletes messages in the selected folder permanently. Without UIDPLUS a
    /// plain EXPUNGE also removes other messages already flagged deleted.
    pub async fn expunge_uids(&mut self, uid_set: &str) -> Result<()> {
        self.store(uid_set, "+FLAGS.SILENT (\\Deleted)").await?;
        if self.has("UIDPLUS") {
            let _: Vec<_> = self
                .session
                .uid_expunge(uid_set)
                .await
                .map_err(protocol)?
                .try_collect()
                .await
                .map_err(protocol)?;
        } else {
            let _: Vec<_> = self
                .session
                .expunge()
                .await
                .map_err(protocol)?
                .try_collect()
                .await
                .map_err(protocol)?;
        }
        Ok(())
    }

    /// Adds a message to `folder`, e.g. a sent copy or a draft.
    pub async fn append(&mut self, folder: &str, flags: Option<&str>, raw: &[u8]) -> Result<()> {
        self.session
            .append(folder, flags, None, raw)
            .await
            .map_err(protocol)
    }

    pub async fn create_folder(&mut self, name: &str) -> Result<()> {
        self.session.create(name).await.map_err(protocol)
    }

    pub async fn delete_folder(&mut self, name: &str) -> Result<()> {
        self.session.delete(name).await.map_err(protocol)
    }

    /// Waits in IDLE (RFC 2177) on the selected folder until the server reports
    /// a change or `wait` passes. Returns the session and whether something changed.
    pub async fn idle(self, wait: Duration) -> Result<(Self, bool)> {
        let caps = self.caps;
        let mut handle = self.session.idle();
        handle.init().await.map_err(protocol)?;
        let changed = {
            let (event, _stop) = handle.wait_with_timeout(wait);
            matches!(event.await.map_err(protocol)?, IdleResponse::NewData(_))
        };
        let session = handle.done().await.map_err(protocol)?;
        Ok((Self { session, caps }, changed))
    }

    async fn store(&mut self, uid_set: &str, query: &str) -> Result<()> {
        let _: Vec<Fetch> = self
            .session
            .uid_store(uid_set, query)
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        Ok(())
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

    /// Full raw messages for a UID set in the selected folder, with their
    /// UIDs. Uses BODY.PEEK so nothing is marked read.
    pub async fn fetch_raw_many(&mut self, uid_set: &str) -> Result<Vec<(u32, Vec<u8>)>> {
        let fetched: Vec<Fetch> = self
            .session
            .uid_fetch(uid_set, "(UID BODY.PEEK[])")
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        Ok(fetched
            .iter()
            .filter_map(|f| Some((f.uid?, f.body()?.to_vec())))
            .collect())
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

/// A refused login, with the server's own words when it gave any.
fn refused(err: async_imap::error::Error) -> Error {
    use async_imap::error::Error::{Bad, No};
    let reason = match &err {
        // The text looks like `code: None, info: Some("[AUTHENTICATIONFAILED] Bad password")`.
        No(text) | Bad(text) => text
            .split_once("info: Some(\"")
            .and_then(|(_, rest)| rest.rsplit_once("\")"))
            .map(|(info, _)| info.trim())
            .map(|info| match info.strip_prefix('[') {
                Some(rest) => rest.split_once(']').map_or(info, |(_, t)| t.trim()),
                None => info,
            })
            .filter(|info| !info.is_empty())
            .map(|info| format!("the server said “{info}”")),
        _ => None,
    };
    Error::Auth(reason.unwrap_or_else(|| err.to_string()))
}

/// SASL XOAUTH2, as Gmail and Outlook define it. When the token is refused
/// the server sends an error challenge, answered with an empty response.
struct XOAuth2 {
    initial: Option<String>,
}

impl XOAuth2 {
    fn new(username: &str, token: &str) -> Self {
        Self {
            initial: Some(xoauth2(username, token)),
        }
    }
}

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        self.initial.take().unwrap_or_default()
    }
}

/// The XOAUTH2 initial response, before base64.
pub fn xoauth2(username: &str, token: &str) -> String {
    format!("user={username}\x01auth=Bearer {token}\x01\x01")
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

/// Sorts IMAP errors into the kinds callers act on: a lost connection is
/// retried later, a refusal is final.
fn protocol(err: async_imap::error::Error) -> Error {
    use async_imap::error::Error as E;
    match err {
        E::Io(e) => Error::Network(e.to_string()),
        E::ConnectionLost => Error::Network("connection lost".into()),
        E::No(reason) => Error::Rejected(reason),
        other => Error::Protocol(other.to_string()),
    }
}

/// Writes UIDs as a compact IMAP set, e.g. `[1, 2, 3, 7]` as `1:3,7`.
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut out = String::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        if !out.is_empty() {
            out.push(',');
        }
        if start == end {
            out.push_str(&start.to_string());
        } else {
            out.push_str(&format!("{start}:{end}"));
        }
        i += 1;
    }
    out
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
    fn uid_sets_collapse_runs() {
        assert_eq!(uid_set(&[7, 1, 2, 3, 3, 10, 11]), "1:3,7,10:11");
        assert_eq!(uid_set(&[5]), "5");
        assert_eq!(uid_set(&[]), "");
    }

    #[test]
    fn known_flags_are_kept_and_others_ignored() {
        let f = flags([Flag::Seen, Flag::Custom("$Label1".into()), Flag::Answered].into_iter());
        assert!(f.seen && f.answered && !f.flagged && !f.deleted);
    }
}
