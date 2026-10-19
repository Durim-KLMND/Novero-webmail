//! Known server settings for common mail providers, looked up by email domain.
//!
//! This is a small built-in list so onboarding can fill in servers for most
//! people. Full autodiscovery (Mozilla ISPDB, DNS SRV, well-known URLs) is
//! phase 4.

use mail_core::{Endpoint, Security};
use serde::Serialize;

/// Suggested settings for an email address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub name: &'static str,
    pub imap: Endpoint,
    pub smtp: Endpoint,
    /// Something the person must know before signing in, such as needing an app password.
    pub note: Option<&'static str>,
}

struct Known {
    name: &'static str,
    domains: &'static [&'static str],
    imap: &'static str,
    smtp: &'static str,
    note: Option<&'static str>,
}

const KNOWN: &[Known] = &[
    Known {
        name: "Gmail",
        domains: &["gmail.com", "googlemail.com"],
        imap: "imap.gmail.com",
        smtp: "smtp.gmail.com",
        note: Some(
            "Gmail needs an app password: turn on 2-Step Verification, then create one at myaccount.google.com/apppasswords.",
        ),
    },
    Known {
        name: "Outlook.com",
        domains: &["outlook.com", "hotmail.com", "live.com", "msn.com"],
        imap: "outlook.office365.com",
        smtp: "smtp-mail.outlook.com",
        note: Some(
            "Microsoft accounts require Microsoft sign-in (OAuth), which comes in a later update. Password sign-in will likely be refused.",
        ),
    },
    Known {
        name: "iCloud Mail",
        domains: &["icloud.com", "me.com", "mac.com"],
        imap: "imap.mail.me.com",
        smtp: "smtp.mail.me.com",
        note: Some("iCloud needs an app-specific password from account.apple.com."),
    },
    Known {
        name: "Yahoo Mail",
        domains: &["yahoo.com", "ymail.com"],
        imap: "imap.mail.yahoo.com",
        smtp: "smtp.mail.yahoo.com",
        note: Some("Yahoo needs an app password from your Yahoo account security page."),
    },
    Known {
        name: "Fastmail",
        domains: &["fastmail.com", "fastmail.fm"],
        imap: "imap.fastmail.com",
        smtp: "smtp.fastmail.com",
        note: Some("Fastmail needs an app password from Settings, Privacy & Security."),
    },
    Known {
        name: "GMX",
        domains: &["gmx.com", "gmx.net", "gmx.de"],
        imap: "imap.gmx.com",
        smtp: "mail.gmx.com",
        note: Some("Turn on IMAP access in your GMX settings first."),
    },
    Known {
        name: "AOL Mail",
        domains: &["aol.com"],
        imap: "imap.aol.com",
        smtp: "smtp.aol.com",
        note: Some("AOL needs an app password from your account security page."),
    },
];

/// Settings for `email`: a known provider, or a guess of `imap.<domain>` and `smtp.<domain>`.
/// Returns `None` when the address has no domain.
pub fn preset_for(email: &str) -> Option<ProviderPreset> {
    let domain = email.trim().rsplit_once('@')?.1.to_ascii_lowercase();
    if domain.is_empty() || !domain.contains('.') {
        return None;
    }
    let tls = |host: String, port| Endpoint {
        host,
        port,
        security: Security::ImplicitTls,
    };
    if let Some(known) = KNOWN.iter().find(|k| k.domains.contains(&domain.as_str())) {
        return Some(ProviderPreset {
            name: known.name,
            imap: tls(known.imap.to_owned(), 993),
            smtp: tls(known.smtp.to_owned(), 465),
            note: known.note,
        });
    }
    Some(ProviderPreset {
        name: "Your provider",
        imap: tls(format!("imap.{domain}"), 993),
        smtp: tls(format!("smtp.{domain}"), 465),
        note: Some(
            "These server names are a guess. Check them with your provider if sign-in fails.",
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_domains_get_their_provider() {
        let gmail = preset_for("Someone@GMail.com").unwrap();
        assert_eq!(gmail.name, "Gmail");
        assert_eq!(gmail.imap.host, "imap.gmail.com");
        assert_eq!(gmail.imap.port, 993);
        assert_eq!(gmail.smtp.port, 465);
        assert!(gmail.note.is_some());
    }

    #[test]
    fn unknown_domains_get_a_guess() {
        let guess = preset_for("me@example.org").unwrap();
        assert_eq!(guess.imap.host, "imap.example.org");
        assert_eq!(guess.smtp.host, "smtp.example.org");
    }

    #[test]
    fn addresses_without_a_domain_get_nothing() {
        assert_eq!(preset_for("not-an-address"), None);
        assert_eq!(preset_for("me@"), None);
        assert_eq!(preset_for("me@localhost"), None);
    }
}
