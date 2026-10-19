//! Signing in through a provider's own page (OAuth2 authorization code flow
//! with PKCE, RFC 8252 for native apps).
//!
//! The system browser opens the provider's sign-in page, which redirects to a
//! one-time listener on 127.0.0.1. The code it brings is exchanged for an
//! access token, used for IMAP and SMTP (XOAUTH2), and a refresh token, kept
//! in the credential store to get new access tokens.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mail_core::{Error, OAuthProvider, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

/// How long the browser sign-in may take before giving up.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// An app registered with a provider, and where its sign-in lives.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub provider: OAuthProvider,
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    /// Google issues desktop apps a "secret" that is not secret; Microsoft
    /// public clients have none.
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
    /// Extra parameters for the sign-in page.
    pub extra: Vec<(String, String)>,
    /// Host in the redirect address. Microsoft registers `localhost`, Google
    /// accepts `127.0.0.1`; the listener is on 127.0.0.1 either way.
    pub redirect_host: String,
}

impl OAuthClient {
    /// The built-in client for `provider`, if this build has a client id for
    /// it. Ids come from `NOVERO_GOOGLE_CLIENT_ID` (with
    /// `NOVERO_GOOGLE_CLIENT_SECRET`) and `NOVERO_MICROSOFT_CLIENT_ID`, set at
    /// build time or, for development, when the app starts.
    pub fn for_provider(provider: OAuthProvider) -> Option<Self> {
        let var = |runtime: &str, built: Option<&'static str>| {
            std::env::var(runtime)
                .ok()
                .or(built.map(str::to_owned))
                .filter(|v| !v.trim().is_empty())
        };
        let strings = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
        match provider {
            OAuthProvider::Google => Some(Self {
                provider,
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
                client_id: var(
                    "NOVERO_GOOGLE_CLIENT_ID",
                    option_env!("NOVERO_GOOGLE_CLIENT_ID"),
                )?,
                client_secret: Some(var(
                    "NOVERO_GOOGLE_CLIENT_SECRET",
                    option_env!("NOVERO_GOOGLE_CLIENT_SECRET"),
                )?),
                scopes: strings(&["https://mail.google.com/", "openid", "email"]),
                // A refresh token is only issued with offline access, and
                // only again on later sign-ins when consent is asked again.
                extra: vec![
                    ("access_type".into(), "offline".into()),
                    ("prompt".into(), "consent".into()),
                ],
                redirect_host: "127.0.0.1".into(),
            }),
            OAuthProvider::Microsoft => Some(Self {
                provider,
                auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
                token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
                client_id: var(
                    "NOVERO_MICROSOFT_CLIENT_ID",
                    option_env!("NOVERO_MICROSOFT_CLIENT_ID"),
                )?,
                client_secret: None,
                scopes: strings(&[
                    "https://outlook.office.com/IMAP.AccessAsUser.All",
                    "https://outlook.office.com/SMTP.Send",
                    "offline_access",
                    "openid",
                    "email",
                ]),
                extra: vec![("prompt".into(), "select_account".into())],
                redirect_host: "localhost".into(),
            }),
        }
    }
}

/// What a sign-in or refresh returns.
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    /// Unix time the access token stops working.
    pub expires_at: i64,
    /// Present after sign-in, and after a refresh when the provider rotates it.
    pub refresh_token: Option<String>,
    /// The address that signed in, when the provider says.
    pub email: Option<String>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("expires_at", &self.expires_at)
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// Signs in through the provider's page: `open` is given the address to show
/// in the browser. Waits until the browser comes back.
pub async fn sign_in(
    client: &OAuthClient,
    login_hint: &str,
    open: impl FnOnce(&str) -> Result<()>,
) -> Result<Tokens> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| Error::Network(format!("could not wait for the sign-in page: {e}")))?;
    let port = listener.local_addr().map_err(io)?.port();
    let redirect_uri = format!("http://{}:{port}", client.redirect_host);
    let verifier = random_string(32)?;
    let state = random_string(16)?;

    let mut url = Url::parse(&client.auth_url)
        .map_err(|e| Error::Invalid(format!("bad sign-in address: {e}")))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", &client.scopes.join(" "))
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("login_hint", login_hint)
        .extend_pairs(&client.extra);
    open(url.as_str())?;

    let code = tokio::time::timeout(SIGN_IN_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| Error::Auth("the sign-in page was not completed in time".into()))??;

    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", client.client_id.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret));
    }
    let tokens = request_tokens(client, &form).await?;
    if tokens.refresh_token.is_none() {
        return Err(Error::Auth(format!(
            "{} did not allow offline access, so Novero Mail could not stay signed in",
            client.provider.label()
        )));
    }
    Ok(tokens)
}

/// A new access token from a refresh token. A refused refresh token is an
/// [`Error::Auth`]: the person has to sign in again.
pub async fn refresh(client: &OAuthClient, refresh_token: &str) -> Result<Tokens> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client.client_id.as_str()),
    ];
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret));
    }
    request_tokens(client, &form).await
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn request_tokens(client: &OAuthClient, form: &[(&str, &str)]) -> Result<Tokens> {
    let response = crate::http::client()?
        .post(&client.token_url)
        .form(form)
        .send()
        .await
        .map_err(|e| Error::Network(format!("could not reach {}: {e}", client.provider.label())))?;
    let status = response.status();
    let body: TokenResponse = response.json().await.map_err(|e| {
        Error::Protocol(format!(
            "{} sent an unreadable answer ({status}): {e}",
            client.provider.label()
        ))
    })?;
    let Some(access_token) = body.access_token.filter(|_| status.is_success()) else {
        let reason = body
            .error_description
            .or(body.error.clone())
            .unwrap_or_else(|| status.to_string());
        return Err(if status.is_server_error() {
            Error::Network(format!(
                "{} is not available: {reason}",
                client.provider.label()
            ))
        } else if body.error.as_deref() == Some("invalid_grant") {
            Error::Auth(format!(
                "{} sign-in has expired or was revoked; sign in again ({reason})",
                client.provider.label()
            ))
        } else {
            Error::Auth(format!(
                "{} refused the sign-in: {reason}",
                client.provider.label()
            ))
        });
    };
    Ok(Tokens {
        access_token,
        // Unknown lifetimes are treated as short, so a fresh token is fetched soon.
        expires_at: now() + body.expires_in.unwrap_or(300),
        refresh_token: body.refresh_token,
        email: body.id_token.as_deref().and_then(id_token_email),
    })
}

/// Accepts browser requests until the one carrying the sign-in result.
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (stream, _) = listener.accept().await.map_err(io)?;
        if let Some(result) = answer_redirect(stream, state).await {
            return result;
        }
    }
}

/// Reads one request. `None` for requests that are not the redirect, such
/// as the browser asking for a favicon.
async fn answer_redirect(stream: TcpStream, state: &str) -> Option<Result<String>> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.ok()?;
    // Skip the headers; nothing in them is needed.
    let mut header = String::new();
    while reader.read_line(&mut header).await.ok()? > 2 {
        header.clear();
    }
    let target = line.split_whitespace().nth(1)?;
    let url = Url::parse(&format!("http://localhost{target}")).ok()?;
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    let result = if param("state").as_deref() != Some(state) {
        if param("code").is_none() && param("error").is_none() {
            let _ = respond(reader.get_mut(), "404 Not Found", "").await;
            return None;
        }
        Err(Error::Auth(
            "the sign-in answer did not match; try again".into(),
        ))
    } else if let Some(code) = param("code") {
        Ok(code)
    } else {
        Err(match param("error").as_deref() {
            Some("access_denied") => Error::Auth("sign-in was cancelled".into()),
            other => Error::Auth(format!(
                "sign-in failed: {}",
                param("error_description")
                    .or(other.map(str::to_owned))
                    .unwrap_or_else(|| "no reason given".into())
            )),
        })
    };
    let page = match &result {
        Ok(_) => page(
            "You're signed in",
            "You can close this tab and go back to Novero Mail.",
        ),
        Err(err) => page(
            "Sign-in did not finish",
            &format!("{err}. Go back to Novero Mail to try again."),
        ),
    };
    let _ = respond(reader.get_mut(), "200 OK", &page).await;
    Some(result)
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

/// The small page the browser shows after signing in.
fn page(title: &str, text: &str) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    format!(
        "<!doctype html><meta charset=utf-8><title>{t} · Novero Mail</title>\
         <style>body{{font:16px/1.5 system-ui,sans-serif;margin:0;display:grid;\
         place-items:center;min-height:100vh;background:#f6f6f4;color:#1d1d1b}}\
         main{{max-width:28rem;padding:24px}}h1{{color:#004b4a;font-size:22px}}\
         @media (prefers-color-scheme:dark){{body{{background:#111111;color:#eeeeec}}\
         h1{{color:#5cbdb4}}}}</style><main><h1>{t}</h1><p>{x}</p></main>",
        t = escape(title),
        x = escape(text)
    )
}

/// The signed-in address from an OpenID Connect id token. The token comes
/// straight from the provider over TLS, so its signature is not checked here.
fn id_token_email(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let json = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&json).ok()?;
    ["email", "preferred_username"]
        .iter()
        .filter_map(|k| claims[k].as_str())
        .find(|v| v.contains('@'))
        .map(str::to_owned)
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn random_string(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| Error::Auth(format!("no randomness available: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

fn io(e: std::io::Error) -> Error {
    Error::Network(e.to_string())
}

/// Seconds since 1970.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_challenge_matches_rfc_7636() {
        // Appendix B of RFC 7636.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_email_is_read_from_the_id_token() {
        let claims = URL_SAFE_NO_PAD.encode(r#"{"sub":"1","email":"me@gmail.com"}"#);
        assert_eq!(
            id_token_email(&format!("x.{claims}.y")).as_deref(),
            Some("me@gmail.com")
        );
        let ms = URL_SAFE_NO_PAD.encode(r#"{"preferred_username":"me@outlook.com"}"#);
        assert_eq!(
            id_token_email(&format!("x.{ms}.y")).as_deref(),
            Some("me@outlook.com")
        );
        assert_eq!(id_token_email("garbage"), None);
    }

    /// A fake provider: its sign-in page approves at once and redirects back,
    /// and its token endpoint checks the PKCE verifier.
    async fn fake_provider() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            let challenge_sent = Arc::new(Mutex::new(String::new()));
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).await.unwrap();
                    if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    if header.len() <= 2 {
                        break;
                    }
                }
                let mut body = vec![0; length];
                tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body)
                    .await
                    .unwrap();
                let body = String::from_utf8(body).unwrap();
                log.lock().unwrap().push(format!("{}{body}", line.trim()));
                let target = line.split_whitespace().nth(1).unwrap().to_owned();
                let url = Url::parse(&format!("http://x{target}")).unwrap();
                let (status, headers, reply) = if url.path() == "/authorize" {
                    let q: std::collections::HashMap<_, _> = url.query_pairs().collect();
                    *challenge_sent.lock().unwrap() = q["code_challenge"].to_string();
                    let back = format!("{}?code=the-code&state={}", q["redirect_uri"], q["state"]);
                    ("302 Found", format!("Location: {back}\r\n"), String::new())
                } else {
                    let f: std::collections::HashMap<_, _> =
                        url::form_urlencoded::parse(body.as_bytes()).collect();
                    let ok = match f["grant_type"].as_ref() {
                        "authorization_code" => {
                            f["code"] == "the-code"
                                && challenge(&f["code_verifier"]) == *challenge_sent.lock().unwrap()
                        }
                        _ => f["refresh_token"] == "the-refresh-token",
                    };
                    if ok {
                        let id = URL_SAFE_NO_PAD.encode(r#"{"email":"alice@example.test"}"#);
                        (
                            "200 OK",
                            String::new(),
                            format!(
                                r#"{{"access_token":"the-access-token","expires_in":3600,"refresh_token":"the-refresh-token","id_token":"h.{id}.s"}}"#
                            ),
                        )
                    } else {
                        (
                            "400 Bad Request",
                            String::new(),
                            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#.into(),
                        )
                    }
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = reader.get_mut().write_all(response.as_bytes()).await;
            }
        });
        (base, seen)
    }

    fn fake_client(base: &str) -> OAuthClient {
        OAuthClient {
            provider: OAuthProvider::Google,
            auth_url: format!("{base}/authorize"),
            token_url: format!("{base}/token"),
            client_id: "client".into(),
            client_secret: Some("not-secret".into()),
            scopes: vec!["mail".into()],
            extra: vec![("access_type".into(), "offline".into())],
            redirect_host: "127.0.0.1".into(),
        }
    }

    #[tokio::test]
    async fn signing_in_through_the_browser_returns_tokens() {
        let (base, seen) = fake_provider().await;
        let client = fake_client(&base);
        let tokens = sign_in(&client, "alice@example.test", |url| {
            assert!(url.contains("login_hint=alice%40example.test"));
            assert!(url.contains("code_challenge_method=S256"));
            assert!(url.contains("access_type=offline"));
            // Stands in for the browser: follows the redirect back to the app.
            let url = url.to_owned();
            tokio::spawn(async move { crate::http::client().unwrap().get(url).send().await });
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(tokens.access_token, "the-access-token");
        assert_eq!(tokens.refresh_token.as_deref(), Some("the-refresh-token"));
        assert_eq!(tokens.email.as_deref(), Some("alice@example.test"));
        assert!(tokens.expires_at > now() + 3000);
        let token_request = seen.lock().unwrap().last().unwrap().clone();
        assert!(token_request.contains("client_secret=not-secret"));
    }

    #[tokio::test]
    async fn refreshing_works_and_a_revoked_token_asks_to_sign_in_again() {
        let (base, _) = fake_provider().await;
        let client = fake_client(&base);
        let tokens = refresh(&client, "the-refresh-token").await.unwrap();
        assert_eq!(tokens.access_token, "the-access-token");
        let err = refresh(&client, "revoked").await.unwrap_err();
        assert!(
            matches!(err, Error::Auth(ref m) if m.contains("sign in again")),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_cancelled_sign_in_is_reported() {
        let client = fake_client("http://127.0.0.1:9");
        let err = sign_in(&client, "a@b.test", |url| {
            let url = Url::parse(url).unwrap();
            let q: std::collections::HashMap<_, _> = url.query_pairs().collect();
            let back = format!(
                "{}/?error=access_denied&state={}",
                q["redirect_uri"], q["state"]
            );
            tokio::spawn(async move { crate::http::client().unwrap().get(back).send().await });
            Ok(())
        })
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "authentication failed: sign-in was cancelled"
        );
    }

    #[test]
    fn providers_without_a_client_id_offer_no_sign_in_page() {
        // Unset in the test environment, unless a developer exported one.
        if std::env::var("NOVERO_MICROSOFT_CLIENT_ID").is_err()
            && option_env!("NOVERO_MICROSOFT_CLIENT_ID").is_none()
        {
            assert!(OAuthClient::for_provider(OAuthProvider::Microsoft).is_none());
        }
    }
}
