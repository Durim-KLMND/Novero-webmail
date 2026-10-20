//! The HTTP client for sign-in and settings lookups.

use std::time::Duration;

use mail_core::{Error, Result};

/// One request may take this long, so a slow server cannot stall setup.
const TIMEOUT: Duration = Duration::from_secs(8);

/// A client with TLS through rustls and the system's certificates, like the
/// IMAP and SMTP connections. Requests are rare (setup and token refresh), so
/// each caller builds its own rather than sharing a pool across runtimes.
pub(crate) fn client() -> Result<reqwest::Client> {
    https_client(TIMEOUT)
}

/// A client like [`client`] whose requests may take up to `timeout`, for
/// other services the app talks to, such as online calendars.
pub fn https_client(timeout: Duration) -> Result<reqwest::Client> {
    // reqwest is built without a crypto provider of its own; use ring, as
    // the rest of the app does. Fails harmlessly if one is installed already.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("NoveroMail/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Error::Network(format!("could not set up HTTPS: {e}")))
}
