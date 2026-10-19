//! Finding IMAP and SMTP settings from just an email address.
//!
//! In order, the first answer wins:
//! 1. the built-in list of providers ([`crate::providers`]);
//! 2. the provider's own autoconfig file, then Thunderbird's ISPDB;
//! 3. the domain's MX records, for custom domains at a known provider or
//!    one listed in the ISPDB (Google Workspace, Microsoft 365, Fastmail);
//! 4. DNS SRV records (RFC 6186 and RFC 8314);
//! 5. a guess of `imap.<domain>` and `smtp.<domain>`.

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use mail_core::{Endpoint, OAuthProvider, Security};
use serde::Serialize;

use crate::providers::{known_for_domain, known_for_mx, oauth_for_host};

/// Thunderbird's database of provider settings.
const ISPDB: &str = "https://autoconfig.thunderbird.net/v1.1";

/// Settings found for an address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovered {
    /// The provider's name, or the domain when nothing better is known.
    pub name: String,
    pub imap: Endpoint,
    pub smtp: Endpoint,
    /// The login name when it is not the email address itself.
    pub username: Option<String>,
    /// The provider's own sign-in page, when it has one Novero Mail supports.
    pub oauth: Option<OAuthProvider>,
    /// What to know before signing in with a password.
    pub note: Option<String>,
    pub source: Source,
}

/// Where settings came from, so the setup screen can say how sure they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Known,
    Autoconfig,
    Dns,
    /// Nothing was found; these are common names to try.
    Guess,
}

/// Looks up settings. Lookups that fail or time out are skipped.
pub struct Discoverer {
    ispdb: String,
    /// The provider's own autoconfig addresses are tried (off in tests).
    own_autoconfig: bool,
    resolver: Option<TokioResolver>,
}

impl Default for Discoverer {
    fn default() -> Self {
        Self::new()
    }
}

impl Discoverer {
    pub fn new() -> Self {
        Self {
            ispdb: ISPDB.into(),
            own_autoconfig: true,
            resolver: TokioResolver::builder_tokio().and_then(|b| b.build()).ok(),
        }
    }

    /// Settings for `email`, or `None` if it is not an address.
    pub async fn discover(&self, email: &str) -> Option<Discovered> {
        let email = email.trim();
        let (_, domain) = email.rsplit_once('@')?;
        let domain = domain.to_ascii_lowercase();
        if domain.is_empty() || !domain.contains('.') || domain.contains(['/', '?', '#', ' ']) {
            return None;
        }
        if let Some(known) = known_for_domain(&domain) {
            return Some(known);
        }

        let (autoconfig, mx, srv) = tokio::join!(
            self.autoconfig(email, &domain),
            self.mx(&domain),
            self.srv(&domain),
        );
        if let Some(found) = autoconfig {
            return Some(found);
        }
        if let Some(mx) = mx {
            if let Some(known) = known_for_mx(&mx) {
                return Some(known);
            }
            let host = base_domain(&mx);
            if host != domain
                && let Some(found) = self.fetch(format!("{}/{host}", self.ispdb), email).await
            {
                return Some(found);
            }
        }
        if let Some(found) = srv {
            return Some(found);
        }
        Some(guess(&domain))
    }

    /// The provider's own autoconfig file, else the ISPDB entry.
    async fn autoconfig(&self, email: &str, domain: &str) -> Option<Discovered> {
        let own = async {
            if !self.own_autoconfig {
                return (None, None);
            }
            let query: String = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("emailaddress", email)
                .finish();
            tokio::join!(
                self.fetch(
                    format!("https://autoconfig.{domain}/mail/config-v1.1.xml?{query}"),
                    email
                ),
                self.fetch(
                    format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"),
                    email
                ),
            )
        };
        let ((subdomain, well_known), ispdb) =
            tokio::join!(own, self.fetch(format!("{}/{domain}", self.ispdb), email));
        subdomain.or(well_known).or(ispdb)
    }

    async fn fetch(&self, url: String, email: &str) -> Option<Discovered> {
        let response = crate::http::client().ok()?.get(&url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        parse_config(&response.text().await.ok()?, email)
    }

    /// The most preferred MX host.
    async fn mx(&self, domain: &str) -> Option<String> {
        let lookup = self.resolver.as_ref()?.mx_lookup(fqdn(domain)).await.ok()?;
        lookup
            .answers()
            .iter()
            .filter_map(|r| match &r.data {
                RData::MX(mx) => Some((mx.preference, mx.exchange.to_utf8())),
                _ => None,
            })
            .min()
            .map(|(_, host)| host.trim_end_matches('.').to_ascii_lowercase())
    }

    /// Settings from SRV records; both IMAP and SMTP must be found.
    async fn srv(&self, domain: &str) -> Option<Discovered> {
        let (imaps, imap, submissions, submission) = tokio::join!(
            self.srv_record("_imaps._tcp", domain),
            self.srv_record("_imap._tcp", domain),
            self.srv_record("_submissions._tcp", domain),
            self.srv_record("_submission._tcp", domain),
        );
        let with = |found: Option<(String, u16)>, security| {
            found.map(|(host, port)| Endpoint {
                host,
                port,
                security,
            })
        };
        let incoming = with(imaps, Security::ImplicitTls).or(with(imap, Security::StartTls))?;
        let outgoing =
            with(submissions, Security::ImplicitTls).or(with(submission, Security::StartTls))?;
        Some(Discovered {
            name: domain.to_owned(),
            oauth: oauth_for_host(&incoming.host),
            imap: incoming,
            smtp: outgoing,
            username: None,
            note: None,
            source: Source::Dns,
        })
    }

    async fn srv_record(&self, service: &str, domain: &str) -> Option<(String, u16)> {
        let name = fqdn(&format!("{service}.{domain}"));
        let lookup = self.resolver.as_ref()?.srv_lookup(name).await.ok()?;
        lookup
            .answers()
            .iter()
            .filter_map(|r| match &r.data {
                RData::SRV(srv) => Some(srv),
                _ => None,
            })
            .min_by_key(|srv| (srv.priority, u16::MAX - srv.weight))
            .map(|srv| {
                (
                    srv.target.to_utf8().trim_end_matches('.').to_owned(),
                    srv.port,
                )
            })
            // A target of "." means the service is not offered (RFC 2782).
            .filter(|(host, _)| !host.is_empty())
    }
}

fn fqdn(name: &str) -> String {
    format!("{}.", name.trim_end_matches('.'))
}

/// `example.com` for `mx1.mail.example.com`. Good enough to look an MX
/// host's provider up in the ISPDB.
fn base_domain(host: &str) -> String {
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    labels[labels.len().saturating_sub(2)..].join(".")
}

fn guess(domain: &str) -> Discovered {
    let tls = |host: String, port| Endpoint {
        host,
        port,
        security: Security::ImplicitTls,
    };
    Discovered {
        name: domain.to_owned(),
        imap: tls(format!("imap.{domain}"), 993),
        smtp: tls(format!("smtp.{domain}"), 465),
        username: None,
        oauth: None,
        note: Some(
            "No settings were found for this address, so these server names are a guess. Check them with your provider if signing in fails."
                .into(),
        ),
        source: Source::Guess,
    }
}

/// Reads a Thunderbird autoconfig file (clientConfig 1.1).
pub fn parse_config(xml: &str, email: &str) -> Option<Discovered> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let provider = doc
        .descendants()
        .find(|n| n.has_tag_name("emailProvider"))?;
    let (local, domain) = email.rsplit_once('@')?;
    let fill = |s: &str| {
        s.replace("%EMAILADDRESS%", email)
            .replace("%EMAILLOCALPART%", local)
            .replace("%EMAILDOMAIN%", domain)
    };
    let child = |node: roxmltree::Node, name: &str| {
        node.children()
            .find(|c| c.has_tag_name(name))
            .and_then(|c| c.text())
            .map(|t| fill(t.trim()))
    };
    // Of the servers listed, the most secure one; the order breaks ties.
    let best = |tag: &str, kind: &str| {
        provider
            .children()
            .filter(|n| n.has_tag_name(tag) && n.attribute("type") == Some(kind))
            .filter_map(|n| {
                let security = match child(n, "socketType")?.to_ascii_uppercase().as_str() {
                    "SSL" | "TLS" => Security::ImplicitTls,
                    "STARTTLS" => Security::StartTls,
                    "PLAIN" => Security::Insecure,
                    _ => return None,
                };
                let endpoint = Endpoint {
                    host: child(n, "hostname")?,
                    port: child(n, "port")?.parse().ok()?,
                    security,
                };
                Some((endpoint, child(n, "username")))
            })
            .min_by_key(|(e, _)| match e.security {
                Security::ImplicitTls => 0,
                Security::StartTls => 1,
                Security::Insecure => 2,
            })
    };
    let (imap, username) = best("incomingServer", "imap")?;
    let (smtp, _) = best("outgoingServer", "smtp")?;
    let name = child(provider, "displayName")
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| domain.to_owned());
    Some(Discovered {
        name,
        oauth: oauth_for_host(&imap.host),
        imap,
        smtp,
        username: username.filter(|u| !u.eq_ignore_ascii_case(email) && !u.is_empty()),
        note: None,
        source: Source::Autoconfig,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const CONFIG: &str = r#"<?xml version="1.0"?>
<clientConfig version="1.1">
  <emailProvider id="example.org">
    <domain>example.org</domain>
    <displayName>Example Mail</displayName>
    <incomingServer type="pop3">
      <hostname>pop.example.org</hostname><port>995</port><socketType>SSL</socketType>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>imap.example.org</hostname><port>143</port><socketType>STARTTLS</socketType>
      <username>%EMAILLOCALPART%</username>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>mail.%EMAILDOMAIN%</hostname><port>993</port><socketType>SSL</socketType>
      <username>%EMAILLOCALPART%</username><authentication>password-cleartext</authentication>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.example.org</hostname><port>587</port><socketType>STARTTLS</socketType>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

    #[test]
    fn autoconfig_files_are_read_and_the_most_secure_server_wins() {
        let found = parse_config(CONFIG, "me@example.org").unwrap();
        assert_eq!(found.name, "Example Mail");
        assert_eq!(
            found.imap,
            Endpoint {
                host: "mail.example.org".into(),
                port: 993,
                security: Security::ImplicitTls
            }
        );
        assert_eq!(found.smtp.port, 587);
        assert_eq!(found.smtp.security, Security::StartTls);
        assert_eq!(found.username.as_deref(), Some("me"));
        assert_eq!(found.source, Source::Autoconfig);
        assert_eq!(found.oauth, None);
    }

    #[test]
    fn broken_or_incomplete_files_are_ignored() {
        assert_eq!(parse_config("not xml", "me@example.org"), None);
        let no_smtp = CONFIG.replace("outgoingServer", "somethingElse");
        assert_eq!(parse_config(&no_smtp, "me@example.org"), None);
    }

    #[test]
    fn autoconfig_for_gmail_hosts_offers_google_sign_in() {
        let xml = CONFIG.replace("mail.%EMAILDOMAIN%", "imap.gmail.com");
        let found = parse_config(&xml, "me@workspace.example").unwrap();
        assert_eq!(found.oauth, Some(OAuthProvider::Google));
    }

    #[test]
    fn mx_hosts_are_reduced_to_their_domain() {
        assert_eq!(
            base_domain("in1-smtp.messagingengine.com."),
            "messagingengine.com"
        );
        assert_eq!(base_domain("localhost"), "localhost");
    }

    #[tokio::test]
    async fn known_providers_answer_without_the_network() {
        let found = Discoverer {
            ispdb: "http://127.0.0.1:9".into(),
            own_autoconfig: false,
            resolver: None,
        }
        .discover(" someone@icloud.com ")
        .await
        .unwrap();
        assert_eq!(found.name, "iCloud Mail");
        assert_eq!(found.source, Source::Known);
    }

    #[tokio::test]
    async fn the_ispdb_is_asked_and_unknown_domains_get_a_guess() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let (status, body) = if request.starts_with("GET /example.org ") {
                    ("200 OK", CONFIG)
                } else {
                    ("404 Not Found", "")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let discoverer = Discoverer {
            ispdb: base,
            own_autoconfig: false,
            resolver: None,
        };
        let found = discoverer.discover("me@example.org").await.unwrap();
        assert_eq!(found.name, "Example Mail");

        let guess = discoverer.discover("me@nowhere.example").await.unwrap();
        assert_eq!(guess.source, Source::Guess);
        assert_eq!(guess.imap.host, "imap.nowhere.example");

        assert_eq!(discoverer.discover("not an address").await, None);
        assert_eq!(discoverer.discover("me@localhost").await, None);
    }
}
