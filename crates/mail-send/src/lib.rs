//! SMTP submission and the outbox.
//!
//! Sending is local-first, like every other change: a message is queued in
//! the outbox, and [`flush_outbox`] sends it once the server is reachable. A
//! connection problem leaves it queued for the next attempt; a message the
//! server refuses waits for the user. After sending, a copy goes to the Sent
//! folder, unless the provider saves one itself.

use std::sync::Mutex;
use std::time::Duration;

use lettre::address::{Address, Envelope};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use mail_core::{Account, Endpoint, Event, FolderRole, Security};
use mail_store::{OutboxState, Store};
use mail_sync::ImapSession;

pub use mail_core::{Error, Result};

/// How long one SMTP exchange may take. Large attachments over a slow link
/// need a while.
const SEND_TIMEOUT: Duration = Duration::from_secs(120);

/// Sends one message. The envelope comes from `from` and `recipients`, not
/// from the headers, so Bcc recipients get it without being named in it.
pub async fn send_raw(
    endpoint: &Endpoint,
    username: &str,
    password: &str,
    from: &str,
    recipients: &[String],
    raw: &[u8],
) -> Result<()> {
    let address = |a: &str| {
        a.parse::<Address>()
            .map_err(|e| Error::Invalid(format!("“{a}” is not a valid email address: {e}")))
    };
    let to = recipients
        .iter()
        .map(|r| address(r))
        .collect::<Result<Vec<_>>>()?;
    let envelope = Envelope::new(Some(address(from)?), to)
        .map_err(|e| Error::Invalid(format!("the message has no recipients: {e}")))?;

    let tls = || {
        TlsParameters::builder(endpoint.host.clone())
            .build_rustls()
            .map_err(|e| Error::Network(format!("TLS setup failed: {e}")))
    };
    let tls = match endpoint.security {
        Security::ImplicitTls => Tls::Wrapper(tls()?),
        Security::StartTls => Tls::Required(tls()?),
        Security::Insecure => Tls::None,
    };
    let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(endpoint.host.as_str())
        .port(endpoint.port)
        .tls(tls)
        .credentials(Credentials::new(username.to_owned(), password.to_owned()))
        .timeout(Some(SEND_TIMEOUT))
        .build();
    transport
        .send_raw(&envelope, raw)
        .await
        .map(|_| ())
        .map_err(|e| smtp_error(&endpoint.host, e))
}

/// Sorts SMTP failures into what happens next: a refused message waits for
/// the user (`Rejected`), a bad login stops sending (`Auth`), and anything
/// else is retried (`Network`).
fn smtp_error(host: &str, err: lettre::transport::smtp::Error) -> Error {
    let code = err.status().map(|c| c.to_string());
    if err.is_permanent() {
        return match code.as_deref() {
            Some(code) if is_login_code(code) => Error::Auth(err.to_string()),
            _ => Error::Rejected(err.to_string()),
        };
    }
    if err.is_transient() {
        return Error::Network(format!("{host} asked to try again later: {err}"));
    }
    Error::Network(format!("could not send through {host}: {err}"))
}

/// 530, 534 and 535 are about the login, not the message.
fn is_login_code(code: &str) -> bool {
    matches!(code, "530" | "534" | "535")
}

/// What a pass over the outbox did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OutboxOutcome {
    pub sent: u32,
    /// Why the server refused messages; they wait in the outbox.
    pub refused: Vec<String>,
    /// The Sent folder, when copies were saved to it.
    pub saved_to: Option<String>,
}

/// Sends every queued message of the account, then saves copies of sent ones
/// to the Sent folder. Stops at the first connection problem, so messages go
/// out in order and none is lost.
pub async fn flush_outbox(
    store: &Mutex<Store>,
    account: &Account,
    password: &str,
    on_event: impl Fn(Event),
) -> Result<OutboxOutcome> {
    let mut outcome = OutboxOutcome::default();
    let items = store.lock().unwrap().outbox(account.id)?;
    for item in items.iter().filter(|i| i.state == OutboxState::Queued) {
        let raw = store.lock().unwrap().outbox_file(item, false)?;
        let result = send_raw(
            &account.smtp,
            &account.username,
            password,
            &account.email,
            &item.recipients,
            &raw,
        )
        .await;
        let store = store.lock().unwrap();
        match result {
            Ok(()) => {
                store.outbox_sent(item.id)?;
                outcome.sent += 1;
            }
            Err(Error::Rejected(reason) | Error::Invalid(reason)) => {
                store.outbox_failed(item.id, &reason)?;
                outcome.refused.push(reason);
            }
            Err(err) => {
                store.outbox_attempt_failed(item.id, &err.to_string())?;
                return Err(err);
            }
        }
    }
    outcome.saved_to = save_sent_copies(store, account, password, &on_event).await?;
    Ok(outcome)
}

/// Appends sent messages to the Sent folder and clears them from the outbox.
async fn save_sent_copies(
    store: &Mutex<Store>,
    account: &Account,
    password: &str,
    on_event: &impl Fn(Event),
) -> Result<Option<String>> {
    let sent: Vec<_> = store
        .lock()
        .unwrap()
        .outbox(account.id)?
        .into_iter()
        .filter(|i| i.state == OutboxState::Sent)
        .collect();
    if sent.is_empty() {
        return Ok(None);
    }
    let folder = store
        .lock()
        .unwrap()
        .folder_with_role(account.id, FolderRole::Sent)?;
    let Some(folder) = folder.filter(|_| !saves_sent_itself(account)) else {
        let store = store.lock().unwrap();
        for item in &sent {
            store.outbox_remove(item.id)?;
        }
        return Ok(None);
    };

    let mut session = ImapSession::connect(&account.imap, &account.username, password).await?;
    for item in &sent {
        let copy = store.lock().unwrap().outbox_file(item, true)?;
        session.append(&folder, Some("(\\Seen)"), &copy).await?;
        store.lock().unwrap().outbox_remove(item.id)?;
    }
    let folder_id = store.lock().unwrap().folder_id(account.id, &folder)?;
    if let Some(id) = folder_id {
        mail_sync::sync_folder(store, &mut session, account.id, id, &folder, on_event).await?;
    }
    session.logout().await?;
    Ok(Some(folder))
}

/// Gmail files every message sent through its SMTP server in Sent already;
/// a second copy would show twice.
fn saves_sent_itself(account: &Account) -> bool {
    let host = account.imap.host.to_ascii_lowercase();
    host.ends_with("gmail.com") || host.ends_with("googlemail.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_is_recognised_by_its_imap_host() {
        let endpoint = |host: &str| Endpoint {
            host: host.into(),
            port: 993,
            security: Security::ImplicitTls,
        };
        let account = |host: &str| Account {
            id: mail_core::AccountId(1),
            display_name: String::new(),
            email: String::new(),
            username: String::new(),
            imap: endpoint(host),
            smtp: endpoint(host),
        };
        assert!(saves_sent_itself(&account("imap.gmail.com")));
        assert!(saves_sent_itself(&account("IMAP.GoogleMail.com")));
        assert!(!saves_sent_itself(&account("imap.fastmail.com")));
    }

    #[test]
    fn login_codes_stop_sending_instead_of_failing_the_message() {
        assert!(is_login_code("535"));
        assert!(!is_login_code("550"));
    }
}
