//! Requests to the calendar services, and what their errors mean.

use mail_core::{Error, OAuthProvider, Result};
use reqwest::{Method, StatusCode};
use serde_json::Value;

pub(crate) struct Api<'a> {
    pub client: &'a reqwest::Client,
    pub token: &'a str,
    pub provider: OAuthProvider,
}

impl Api<'_> {
    fn service(&self) -> &'static str {
        match self.provider {
            OAuthProvider::Google => "Google Calendar",
            OAuthProvider::Microsoft => "Outlook",
        }
    }

    /// Sends a request and reads the JSON answer. `None` when what was asked
    /// for is not there (404) or gone (410).
    pub async fn send(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
        prefer: &[&str],
    ) -> Result<Option<Value>> {
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(self.token)
            .header("Accept", "application/json");
        for p in prefer {
            request = request.header("Prefer", *p);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|e| Error::Network(format!("could not reach {}: {e}", self.service())))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            if text.trim().is_empty() {
                return Ok(Some(Value::Null));
            }
            return serde_json::from_str(&text).map(Some).map_err(|e| {
                Error::Protocol(format!("{} sent an unreadable answer: {e}", self.service()))
            });
        }
        if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
            return Ok(None);
        }
        Err(self.failure(status, &text))
    }

    fn failure(&self, status: StatusCode, text: &str) -> Error {
        let reason = serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| status.to_string());
        let service = self.service();
        match status {
            StatusCode::UNAUTHORIZED => Error::Auth(format!(
                "{service} sign-in has expired; sign in again ({reason})"
            )),
            StatusCode::TOO_MANY_REQUESTS => {
                Error::Network(format!("{service} asked to slow down: {reason}"))
            }
            s if s.is_server_error() => {
                Error::Network(format!("{service} is not available: {reason}"))
            }
            _ => Error::Rejected(format!("{service}: {reason}")),
        }
    }

    pub async fn get(&self, url: &str, prefer: &[&str]) -> Result<Option<Value>> {
        self.send(Method::GET, url, None, prefer).await
    }
}

/// The text of a JSON string field, if it has some.
pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|s| !s.is_empty())
}
