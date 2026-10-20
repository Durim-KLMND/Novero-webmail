//! Integration tests for sending: SMTP through Mailpit, refused messages,
//! retrying after a connection problem, and the copy saved to Sent.
//!
//! Run with the test servers up: `cargo test -p mail-send -- --ignored`.
//! The Sent folder here is a scratch folder, so the seeded mailbox stays as it is.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mail_core::{
    Account, AccountId, Draft, DraftFields, Endpoint, Error, FolderRole, NewAccount, Secret,
    Security,
};
use mail_mime::{Sender, build_message};
use mail_send::{check_login, flush_outbox, send_raw};
use mail_store::{NewDraft, OutboxState, Store};
use mail_sync::ImapSession;
use serde_json::Value;

/// Alice's password on the test servers.
fn secret() -> Secret {
    Secret::password("secret")
}

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

fn imap() -> Endpoint {
    Endpoint {
        host: env("MAIL_TEST_IMAP_HOST", "127.0.0.1"),
        port: env("MAIL_TEST_IMAP_PORT", "1143").parse().unwrap(),
        security: Security::Insecure,
    }
}

fn smtp() -> Endpoint {
    Endpoint {
        host: env("MAIL_TEST_SMTP_HOST", "127.0.0.1"),
        port: env("MAIL_TEST_SMTP_PORT", "1025").parse().unwrap(),
        security: Security::Insecure,
    }
}

/// Unique per test run, so tests can find their own messages.
fn unique(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{label}-{}-{nanos}", std::process::id())
}

/// Every message Mailpit holds, newest first.
fn mailpit_messages() -> Vec<Value> {
    let host = env("MAIL_TEST_SMTP_HOST", "127.0.0.1");
    let port = env("MAIL_TEST_MAILPIT_PORT", "8025");
    let mut stream = TcpStream::connect(format!("{host}:{port}")).unwrap();
    write!(
        stream,
        "GET /api/v1/messages?limit=1000 HTTP/1.0\r\nHost: {host}\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let body = response.split_once("\r\n\r\n").unwrap().1;
    let json: Value = serde_json::from_str(body).unwrap();
    json["messages"].as_array().unwrap().clone()
}

fn received(message_id: &str) -> Option<Value> {
    mailpit_messages()
        .into_iter()
        .find(|m| m["MessageID"].as_str() == Some(message_id))
}

fn addresses(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|a| {
            a.iter()
                .map(|x| x["Address"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn draft(message_id: &str, fields: DraftFields) -> Draft {
    Draft {
        id: 0,
        account: AccountId(0),
        message_id: message_id.into(),
        fields,
        attachments: Vec::new(),
        in_reply_to: None,
        references: Vec::new(),
        replying_to: None,
    }
}

const ALICE: Sender<'static> = Sender {
    name: "Alice",
    email: "alice@example.test",
};

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn mail_is_sent_with_bcc_hidden_from_the_headers() {
    let id = format!("{}@example.test", unique("bcc"));
    let d = draft(
        &id,
        DraftFields {
            to: "Bob <bob@example.test>".into(),
            bcc: "dave@example.test".into(),
            subject: "Hello".into(),
            body: "Hi Bob".into(),
            ..DraftFields::default()
        },
    );
    let built = build_message(ALICE, &d, &[], 0, true).unwrap();
    send_raw(
        &smtp(),
        "alice",
        &secret(),
        "alice@example.test",
        &built.recipients,
        &built.raw,
    )
    .await
    .unwrap();

    let message = received(&id).expect("Mailpit received the message");
    assert_eq!(message["Subject"], "Hello");
    assert_eq!(addresses(&message["To"]), ["bob@example.test"]);
    // Mailpit lists envelope recipients missing from the headers as Bcc.
    assert_eq!(addresses(&message["Bcc"]), ["dave@example.test"]);
}

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn a_wrong_password_is_a_login_error() {
    let err = send_raw(
        &smtp(),
        "alice",
        &Secret::password("wrong"),
        "alice@example.test",
        &["bob@example.test".into()],
        b"Subject: x\r\n\r\nx\r\n",
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Auth(_)), "{err}");
}

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn the_login_is_checked_without_sending() {
    check_login(&smtp(), "alice", &secret()).await.unwrap();
    let err = check_login(&smtp(), "alice", &Secret::password("wrong"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Auth(_)), "{err}");
    let nowhere = Endpoint { port: 9, ..smtp() };
    let err = check_login(&nowhere, "alice", &secret()).await.unwrap_err();
    assert!(matches!(err, Error::Network(_)), "{err}");
}

/// A store with Alice's account. A scratch folder stands in for Sent.
async fn setup(dir: &std::path::Path, smtp: Endpoint) -> (Mutex<Store>, Account, String) {
    let mut store = Store::open_in_memory(dir).unwrap();
    let account = store
        .add_account(&NewAccount {
            auth: Default::default(),
            display_name: "Alice".into(),
            email: "alice@example.test".into(),
            username: "alice".into(),
            imap: imap(),
            smtp,
        })
        .unwrap();
    let sent = unique("Test-Sent");
    let mut session = ImapSession::connect(&imap(), "alice", &secret())
        .await
        .unwrap();
    session.create_folder(&sent).await.unwrap();
    let mut remote = session.list_folders().await.unwrap();
    session.logout().await.unwrap();
    for folder in &mut remote {
        if folder.role == FolderRole::Sent {
            folder.role = FolderRole::Other;
        }
        if folder.name == sent {
            folder.role = FolderRole::Sent;
        }
    }
    store.sync_folders(account.id, &remote).unwrap();
    (Mutex::new(store), account, sent)
}

async fn drop_folder(name: &str) {
    let mut session = ImapSession::connect(&imap(), "alice", &secret())
        .await
        .unwrap();
    let _ = session.delete_folder(name).await;
    let _ = session.logout().await;
}

fn queue(store: &Mutex<Store>, account: &Account, message_id: &str, to: &str) -> i64 {
    let mut store = store.lock().unwrap();
    let draft = store
        .create_draft(NewDraft {
            fields: DraftFields {
                to: to.into(),
                subject: "Queued".into(),
                body: "Sent from the outbox".into(),
                ..DraftFields::default()
            },
            ..NewDraft::new(account.id, message_id.into())
        })
        .unwrap();
    let built = build_message(ALICE, &draft, &[], 0, true).unwrap();
    store
        .queue_outgoing(&draft, &built.raw, &built.copy, &built.recipients)
        .unwrap()
}

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn mail_queued_offline_sends_after_reconnecting_and_lands_in_sent() {
    let dir = tempfile::tempdir().unwrap();
    // Port 1 refuses connections, like a network that is down.
    let offline = Endpoint { port: 1, ..smtp() };
    let (store, mut account, sent) = setup(dir.path(), offline.clone()).await;
    let id = format!("{}@example.test", unique("outbox"));
    let item = queue(&store, &account, &id, "bob@example.test");

    let err = flush_outbox(&store, &account, &secret(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Network(_)), "{err}");
    let waiting = store.lock().unwrap().outbox_item(item).unwrap().unwrap();
    assert_eq!(waiting.state, OutboxState::Queued);
    assert_eq!(waiting.attempts, 1);
    assert!(received(&id).is_none());

    account.smtp = smtp();
    let outcome = flush_outbox(&store, &account, &secret(), |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.sent, 1);
    assert_eq!(outcome.saved_to.as_deref(), Some(sent.as_str()));
    assert!(received(&id).is_some());
    assert!(store.lock().unwrap().outbox(account.id).unwrap().is_empty());

    // The copy is on the server, read, and already in the cache.
    let mut session = ImapSession::connect(&imap(), "alice", &secret())
        .await
        .unwrap();
    session.select(&sent).await.unwrap();
    let uids = session
        .search_uids(&format!("HEADER Message-ID \"<{id}>\" SEEN"))
        .await
        .unwrap();
    assert_eq!(uids.len(), 1);
    session.logout().await.unwrap();
    let folder = store
        .lock()
        .unwrap()
        .folder_id(account.id, &sent)
        .unwrap()
        .unwrap();
    assert_eq!(store.lock().unwrap().folder_state(folder).unwrap().count, 1);
    drop_folder(&sent).await;
}

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn a_refused_message_waits_and_the_rest_still_send() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account, sent) = setup(dir.path(), smtp()).await;
    let refused_id = format!("{}@example.test", unique("refused"));
    let ok_id = format!("{}@example.test", unique("ok"));
    let refused = queue(&store, &account, &refused_id, "someone@elsewhere.invalid");
    queue(&store, &account, &ok_id, "bob@example.test");

    let outcome = flush_outbox(&store, &account, &secret(), |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.sent, 1);
    assert_eq!(outcome.refused.len(), 1);
    assert!(received(&ok_id).is_some());
    assert!(received(&refused_id).is_none());

    let item = store.lock().unwrap().outbox_item(refused).unwrap().unwrap();
    assert_eq!(item.state, OutboxState::Failed);
    assert!(item.last_error.is_some());
    assert_eq!(
        store.lock().unwrap().outbox_counts(account.id).unwrap(),
        (0, 1, 0)
    );

    // It stays put on the next pass until the user retries it.
    let again = flush_outbox(&store, &account, &secret(), |_| {})
        .await
        .unwrap();
    assert_eq!((again.sent, again.refused.len()), (0, 0));
    drop_folder(&sent).await;
}
