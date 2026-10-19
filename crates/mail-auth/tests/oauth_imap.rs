//! Signing in to IMAP with an OAuth2 access token (XOAUTH2), against Dovecot
//! and the fake provider in `testing/imap-server/oauth_mock.py`.
//!
//! Run with the test servers up: `cargo test -p mail-auth -- --ignored`.

use mail_auth::oauth::{OAuthClient, refresh};
use mail_core::{Endpoint, Error, OAuthProvider, Secret, Security};
use mail_sync::ImapSession;

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

fn provider() -> OAuthClient {
    let base = format!(
        "http://{}:{}",
        env("MAIL_TEST_IMAP_HOST", "127.0.0.1"),
        env("MAIL_TEST_OAUTH_PORT", "18080")
    );
    OAuthClient {
        provider: OAuthProvider::Google,
        auth_url: format!("{base}/authorize"),
        token_url: format!("{base}/token"),
        client_id: "novero-test".into(),
        client_secret: None,
        scopes: vec!["mail".into()],
        extra: Vec::new(),
        redirect_host: "127.0.0.1".into(),
    }
}

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn a_refreshed_access_token_signs_in_to_imap() {
    let tokens = refresh(&provider(), "alice-refresh").await.unwrap();
    let mut session =
        ImapSession::connect(&imap(), "alice", &Secret::AccessToken(tokens.access_token))
            .await
            .unwrap();
    let folders = session.list_folders().await.unwrap();
    assert!(folders.iter().any(|f| f.name == "INBOX"));
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn a_token_the_provider_does_not_know_is_refused() {
    let result =
        ImapSession::connect(&imap(), "alice", &Secret::AccessToken("made-up".into())).await;
    assert!(matches!(result, Err(Error::Auth(_))));
}

#[tokio::test]
#[ignore = "needs the test mail servers: docker compose -f testing/docker-compose.yml up -d"]
async fn a_revoked_refresh_token_asks_to_sign_in_again() {
    let err = refresh(&provider(), "revoked").await.unwrap_err();
    assert!(
        matches!(err, Error::Auth(ref m) if m.contains("sign in again")),
        "{err}"
    );
}
