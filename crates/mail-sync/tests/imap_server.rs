//! Integration tests against the Dovecot test server in `testing/`.
//!
//! Start it with `docker compose -f testing/docker-compose.yml up -d --build --wait`,
//! then run `cargo test -p mail-sync -- --ignored`. Override the address with
//! `MAIL_TEST_IMAP_HOST` and `MAIL_TEST_IMAP_PORT`.

use mail_core::{Endpoint, FolderRole, Security};
use mail_sync::ImapSession;

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

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn lists_folders_with_special_use_roles() {
    let mut session = ImapSession::connect(&endpoint(), "alice", "secret")
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
async fn fetches_the_newest_hundred_envelopes() {
    let mut session = ImapSession::connect(&endpoint(), "alice", "secret")
        .await
        .unwrap();
    let envelopes = session.fetch_latest_envelopes("INBOX", 100).await.unwrap();

    assert_eq!(envelopes.len(), 100);
    assert!(
        envelopes.windows(2).all(|w| w[0].uid > w[1].uid),
        "newest first"
    );
    let newest = &envelopes[0];
    assert_eq!(newest.subject.as_deref(), Some("Test message 150"));
    assert_eq!(
        newest.message_id.as_deref(),
        Some("<seed-150@example.test>")
    );
    assert_eq!(
        newest.from.as_deref(),
        Some("Sender 3 <sender3@example.test>")
    );
    assert!(newest.flags.seen, "message 150 is seeded as read");
    assert!(!envelopes[1].flags.seen, "message 149 is seeded as unread");
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn reports_condstore_state_on_select() {
    let mut session = ImapSession::connect(&endpoint(), "alice", "secret")
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
    let result = ImapSession::connect(&endpoint(), "alice", "wrong").await;
    assert!(matches!(result, Err(mail_core::Error::Auth(_))));
}
