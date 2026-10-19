//! Integration tests against the Dovecot test server in `testing/`.
//!
//! Start it with `docker compose -f testing/docker-compose.yml up -d --build --wait`,
//! then run `cargo test -p mail-sync -- --ignored`. Override the address with
//! `MAIL_TEST_IMAP_HOST` and `MAIL_TEST_IMAP_PORT`.

use std::sync::Mutex;

use mail_core::{Endpoint, Event, FolderRole, NewAccount, Secret, Security};
use mail_store::Store;
use mail_sync::{ImapSession, Scope, fetch_message, sync_account};

/// Alice's password on the test servers.
fn secret() -> Secret {
    Secret::password("secret")
}

fn endpoint() -> Endpoint {
    Endpoint {
        host: std::env::var("MAIL_TEST_IMAP_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MAIL_TEST_IMAP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(1143),
        security: Security::Insecure,
    }
}

fn store_with_alice(dir: &std::path::Path) -> (Mutex<Store>, mail_core::Account) {
    let store = Store::open_in_memory(dir).unwrap();
    let account = store
        .add_account(&NewAccount {
            auth: Default::default(),
            display_name: "Alice".into(),
            email: "alice@example.test".into(),
            username: "alice".into(),
            imap: endpoint(),
            smtp: endpoint(),
        })
        .unwrap();
    (Mutex::new(store), account)
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn lists_folders_with_special_use_roles() {
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let folders = session.list_folders().await.unwrap();
    let role = |name: &str| folders.iter().find(|f| f.name == name).map(|f| f.role);
    assert_eq!(role("INBOX"), Some(FolderRole::Inbox));
    assert_eq!(role("Sent"), Some(FolderRole::Sent));
    assert_eq!(role("Drafts"), Some(FolderRole::Drafts));
    assert_eq!(role("Trash"), Some(FolderRole::Trash));
    assert_eq!(role("Archive"), Some(FolderRole::Archive));
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn fetches_the_newest_hundred_headers() {
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let headers = session.fetch_latest_headers("INBOX", 100).await.unwrap();

    assert_eq!(headers.len(), 100);
    assert!(
        headers.windows(2).all(|w| w[0].uid > w[1].uid),
        "newest first"
    );
    let newest = &headers[0];
    assert_eq!(newest.subject.as_deref(), Some("Test message 150"));
    assert_eq!(
        newest.message_id.as_deref(),
        Some("<seed-150@example.test>")
    );
    assert_eq!(
        newest.from.as_deref(),
        Some("Sender 3 <sender3@example.test>")
    );
    assert!(newest.date.is_some());
    assert!(newest.flags.seen, "message 150 is seeded as read");
    assert!(!headers[1].flags.seen, "message 149 is seeded as unread");
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn reports_folder_state_on_select() {
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let status = session.select("INBOX").await.unwrap();
    assert_eq!(status.exists, 150);
    assert!(status.uid_validity.is_some());
    assert_eq!(status.uid_next, Some(151));
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn wrong_password_is_an_auth_error() {
    let result = ImapSession::connect(&endpoint(), "alice", &Secret::password("wrong")).await;
    match result {
        Err(mail_core::Error::Auth(reason)) => {
            // The server's own words, without the response code.
            assert_eq!(reason, "the server said “Authentication failed.”");
        }
        Err(other) => panic!("expected a login error, got {other}"),
        Ok(_) => panic!("a wrong password was accepted"),
    }
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn full_sync_fills_every_folder_and_a_second_sync_adds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = store_with_alice(dir.path());

    let events = Mutex::new(Vec::new());
    sync_account(&store, &account, &secret(), Scope::All, |e| {
        events.lock().unwrap().push(e)
    })
    .await
    .unwrap();

    let folders = store.lock().unwrap().list_folders(account.id).unwrap();
    let count = |name: &str| folders.iter().find(|f| f.name == name).unwrap().total;
    assert_eq!(folders[0].name, "INBOX", "inbox sorts first");
    assert_eq!(count("INBOX"), 150);
    assert_eq!(count("Sent"), 5);
    assert_eq!(folders[0].unread, 100);

    let events = events.into_inner().unwrap();
    assert!(events.contains(&Event::SyncProgress {
        account: account.id,
        folder: "INBOX".into(),
        done: 150,
        total: 150,
    }));

    let again = Mutex::new(Vec::new());
    sync_account(&store, &account, &secret(), Scope::All, |e| {
        again.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let fetched: u32 = again
        .into_inner()
        .unwrap()
        .iter()
        .map(|e| match e {
            Event::SyncProgress { total, .. } => *total,
            _ => 0,
        })
        .sum();
    assert_eq!(fetched, 0, "nothing new to fetch");
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn interrupted_sync_is_completed_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = store_with_alice(dir.path());

    // Pretend a first sync stopped after the newest 20 inbox messages.
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let remote = session.list_folders().await.unwrap();
    let newest = session.fetch_latest_headers("INBOX", 20).await.unwrap();
    session.logout().await.unwrap();
    let inbox = {
        let mut s = store.lock().unwrap();
        let ids = s.sync_folders(account.id, &remote).unwrap();
        let inbox = ids.iter().find(|(_, f)| f.name == "INBOX").unwrap().0;
        s.insert_headers(inbox, &newest).unwrap();
        inbox
    };

    sync_account(&store, &account, &secret(), Scope::All, |_| {})
        .await
        .unwrap();
    assert_eq!(
        store.lock().unwrap().folder_state(inbox).unwrap().count,
        150
    );
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn opening_a_message_fetches_and_caches_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = store_with_alice(dir.path());
    sync_account(&store, &account, &secret(), Scope::All, |_| {})
        .await
        .unwrap();

    let inbox = store.lock().unwrap().list_folders(account.id).unwrap()[0].id;
    let id = store
        .lock()
        .unwrap()
        .list_messages(inbox, 0, 1)
        .unwrap()
        .items[0]
        .id;
    let raw = fetch_message(&store, &account, &secret(), id)
        .await
        .unwrap();
    let parsed = mail_mime::parse_message(&raw, false).unwrap();
    assert_eq!(parsed.subject.as_deref(), Some("Test message 150"));
    assert_eq!(
        parsed.text.as_deref().map(str::trim),
        Some("Body of test message 150.")
    );

    // The second open reads from disk, even with a wrong password.
    let again = fetch_message(&store, &account, &Secret::password("wrong"), id)
        .await
        .unwrap();
    assert_eq!(again, raw);
}
