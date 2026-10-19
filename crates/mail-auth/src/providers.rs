//! Built-in settings for common mail providers, found by the address's
//! domain or, for custom domains hosted by a provider, by its MX records.
//!
//! This list answers instantly and offline, and knows what autoconfig files
//! do not say: which providers sign in through their own page, and what
//! people need to know before using a password.

use mail_core::{Endpoint, OAuthProvider, Security};

use crate::discover::{Discovered, Source};

pub(crate) struct Known {
    name: &'static str,
    /// Address domains this provider hosts.
    domains: &'static [&'static str],
    /// MX hosts end in one of these when a custom domain uses this provider.
    mx: &'static [&'static str],
    imap: (&'static str, u16, Security),
    smtp: (&'static str, u16, Security),
    oauth: Option<OAuthProvider>,
    /// Shown when signing in with a password.
    note: Option<&'static str>,
}

use Security::{ImplicitTls, StartTls};

const KNOWN: &[Known] = &[
    Known {
        name: "Gmail",
        domains: &["gmail.com", "googlemail.com"],
        mx: &["google.com", "googlemail.com"],
        imap: ("imap.gmail.com", 993, ImplicitTls),
        smtp: ("smtp.gmail.com", 465, ImplicitTls),
        oauth: Some(OAuthProvider::Google),
        note: Some(
            "With a password, Gmail needs an app password: turn on 2-Step Verification, then create one at myaccount.google.com/apppasswords.",
        ),
    },
    Known {
        name: "Outlook.com",
        domains: &[
            "outlook.com",
            "hotmail.com",
            "live.com",
            "msn.com",
            "hotmail.co.uk",
            "live.co.uk",
            "outlook.de",
            "hotmail.de",
            "hotmail.fr",
            "live.fr",
        ],
        mx: &["olc.protection.outlook.com"],
        imap: ("outlook.office365.com", 993, ImplicitTls),
        smtp: ("smtp-mail.outlook.com", 587, StartTls),
        oauth: Some(OAuthProvider::Microsoft),
        note: Some(
            "Microsoft accounts no longer accept passwords from mail apps. Sign in with Microsoft instead.",
        ),
    },
    Known {
        name: "Microsoft 365",
        domains: &[],
        mx: &["mail.protection.outlook.com"],
        imap: ("outlook.office365.com", 993, ImplicitTls),
        smtp: ("smtp.office365.com", 587, StartTls),
        oauth: Some(OAuthProvider::Microsoft),
        note: Some(
            "Microsoft 365 no longer accepts passwords from mail apps. Sign in with Microsoft instead. If sending fails, your IT admin may need to allow SMTP for your mailbox.",
        ),
    },
    Known {
        name: "iCloud Mail",
        domains: &["icloud.com", "me.com", "mac.com"],
        mx: &["mail.icloud.com"],
        imap: ("imap.mail.me.com", 993, ImplicitTls),
        smtp: ("smtp.mail.me.com", 587, StartTls),
        oauth: None,
        note: Some(
            "iCloud needs an app-specific password: create one under Sign-In and Security at account.apple.com.",
        ),
    },
    Known {
        name: "Fastmail",
        domains: &[
            "fastmail.com",
            "fastmail.fm",
            "fastmail.net",
            "messagingengine.com",
        ],
        mx: &["messagingengine.com"],
        imap: ("imap.fastmail.com", 993, ImplicitTls),
        smtp: ("smtp.fastmail.com", 465, ImplicitTls),
        oauth: None,
        note: Some(
            "Fastmail needs an app password: create one under Settings, Privacy & Security, Manage app passwords.",
        ),
    },
    Known {
        name: "Yahoo Mail",
        domains: &[
            "yahoo.com",
            "ymail.com",
            "rocketmail.com",
            "yahoo.co.uk",
            "yahoo.fr",
            "yahoo.de",
        ],
        mx: &[],
        imap: ("imap.mail.yahoo.com", 993, ImplicitTls),
        smtp: ("smtp.mail.yahoo.com", 465, ImplicitTls),
        oauth: None,
        note: Some("Yahoo needs an app password from your Yahoo account security page."),
    },
    Known {
        name: "AOL Mail",
        domains: &["aol.com"],
        mx: &[],
        imap: ("imap.aol.com", 993, ImplicitTls),
        smtp: ("smtp.aol.com", 465, ImplicitTls),
        oauth: None,
        note: Some("AOL needs an app password from your account security page."),
    },
    Known {
        name: "GMX",
        domains: &["gmx.com"],
        mx: &[],
        imap: ("imap.gmx.com", 993, ImplicitTls),
        smtp: ("mail.gmx.com", 465, ImplicitTls),
        oauth: None,
        note: Some("Turn on IMAP access in your GMX settings first."),
    },
    Known {
        name: "GMX",
        domains: &["gmx.net", "gmx.de", "gmx.at", "gmx.ch"],
        mx: &["gmx.net"],
        imap: ("imap.gmx.net", 993, ImplicitTls),
        smtp: ("mail.gmx.net", 465, ImplicitTls),
        oauth: None,
        note: Some("Turn on IMAP access in your GMX settings first."),
    },
    Known {
        name: "Proton Mail",
        domains: &["proton.me", "protonmail.com", "protonmail.ch", "pm.me"],
        mx: &["protonmail.ch"],
        imap: ("127.0.0.1", 1143, StartTls),
        smtp: ("127.0.0.1", 1025, StartTls),
        oauth: None,
        note: Some(
            "Proton Mail works through Proton Mail Bridge running on this computer. Use the password Bridge shows, and install Bridge's certificate in Windows (in Bridge: Settings, Export TLS certificates).",
        ),
    },
];

impl Known {
    fn discovered(&self) -> Discovered {
        let endpoint = |(host, port, security): (&str, u16, Security)| Endpoint {
            host: host.to_owned(),
            port,
            security,
        };
        Discovered {
            name: self.name.to_owned(),
            imap: endpoint(self.imap),
            smtp: endpoint(self.smtp),
            username: None,
            oauth: self.oauth,
            note: self.note.map(str::to_owned),
            source: Source::Known,
        }
    }
}

/// Settings for addresses at a provider's own domain, e.g. gmail.com.
pub fn known_for_domain(domain: &str) -> Option<Discovered> {
    let domain = domain.to_ascii_lowercase();
    KNOWN
        .iter()
        .find(|k| k.domains.contains(&domain.as_str()))
        .map(Known::discovered)
}

/// Settings for a custom domain whose mail goes to `mx_host`, e.g. a Google
/// Workspace domain with MX `aspmx.l.google.com`. The longest match wins, so
/// Outlook.com's own MX hosts are not taken for Microsoft 365.
pub fn known_for_mx(mx_host: &str) -> Option<Discovered> {
    let host = mx_host.trim_end_matches('.').to_ascii_lowercase();
    KNOWN
        .iter()
        .flat_map(|k| k.mx.iter().map(move |suffix| (k, *suffix)))
        .filter(|(_, suffix)| host == *suffix || host.ends_with(&format!(".{suffix}")))
        .max_by_key(|(_, suffix)| suffix.len())
        .map(|(k, _)| k.discovered())
}

/// The provider whose sign-in page works for an IMAP server, if any. Lets
/// settings found by autoconfig offer the same sign-in as the built-in list.
pub fn oauth_for_host(imap_host: &str) -> Option<OAuthProvider> {
    match imap_host.to_ascii_lowercase().as_str() {
        "imap.gmail.com" | "imap.googlemail.com" => Some(OAuthProvider::Google),
        "outlook.office365.com" | "imap-mail.outlook.com" => Some(OAuthProvider::Microsoft),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_domains_get_their_provider() {
        let gmail = known_for_domain("GMail.com").unwrap();
        assert_eq!(gmail.name, "Gmail");
        assert_eq!(gmail.imap.host, "imap.gmail.com");
        assert_eq!(gmail.oauth, Some(OAuthProvider::Google));
        let outlook = known_for_domain("hotmail.com").unwrap();
        assert_eq!(outlook.smtp.port, 587);
        assert_eq!(outlook.smtp.security, Security::StartTls);
        assert_eq!(known_for_domain("example.org"), None);
    }

    #[test]
    fn custom_domains_are_matched_by_mx() {
        assert_eq!(known_for_mx("aspmx.l.google.com.").unwrap().name, "Gmail");
        assert_eq!(
            known_for_mx("contoso-com.mail.protection.outlook.com")
                .unwrap()
                .name,
            "Microsoft 365"
        );
        assert_eq!(
            known_for_mx("outlook-com.olc.protection.outlook.com")
                .unwrap()
                .name,
            "Outlook.com"
        );
        assert_eq!(
            known_for_mx("in1-smtp.messagingengine.com").unwrap().name,
            "Fastmail"
        );
        assert_eq!(known_for_mx("mx.example.org"), None);
        // A suffix only matches at a label boundary.
        assert_eq!(known_for_mx("notgoogle.com"), None);
    }

    #[test]
    fn sign_in_pages_are_found_by_imap_host() {
        assert_eq!(
            oauth_for_host("IMAP.gmail.com"),
            Some(OAuthProvider::Google)
        );
        assert_eq!(
            oauth_for_host("outlook.office365.com"),
            Some(OAuthProvider::Microsoft)
        );
        assert_eq!(oauth_for_host("imap.fastmail.com"), None);
    }
}
