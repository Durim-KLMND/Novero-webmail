//! Checks the TLS path against a real server: `cargo run -p mail-sync --example tls_probe -- imap.gmail.com`.
//! Bad credentials are used on purpose, so success means an authentication error, not a network one.

use mail_core::{Endpoint, Security};

#[tokio::main]
async fn main() {
    let host = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "imap.gmail.com".into());
    let endpoint = Endpoint {
        host,
        port: 993,
        security: Security::ImplicitTls,
    };
    match mail_sync::ImapSession::connect(
        &endpoint,
        "probe@example.invalid",
        &mail_core::Secret::password("x"),
    )
    .await
    {
        Err(mail_core::Error::Auth(e)) => println!("TLS ok, login refused as expected: {e}"),
        other => println!("unexpected: {:?}", other.map(|_| ())),
    }
}
