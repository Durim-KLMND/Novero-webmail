//! Email addresses as they are typed and stored: `Name <a@b>` lists.

use serde::{Deserialize, Serialize};

/// One address: an optional display name and the address itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mailbox {
    pub name: Option<String>,
    pub email: String,
}

impl Mailbox {
    /// The address as it is shown and typed: `Name <a@b>`, or just `a@b`.
    pub fn display(&self) -> String {
        match &self.name {
            Some(name) if name.contains([',', ';', '<', '>', '"', '@']) => {
                format!("\"{}\" <{}>", name.replace('"', "'"), self.email)
            }
            Some(name) => format!("{name} <{}>", self.email),
            None => self.email.clone(),
        }
    }
}

/// Splits an address list on commas and semicolons outside quotes and
/// angle brackets, dropping empty entries.
pub fn split(input: &str) -> impl Iterator<Item = &str> {
    let mut entries = Vec::new();
    let (mut quoted, mut angle, mut start) = (false, false, 0);
    for (i, c) in input.char_indices() {
        match c {
            '"' if !angle => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                entries.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    entries.push(&input[start..]);
    entries.into_iter().map(str::trim).filter(|e| !e.is_empty())
}

/// One entry of a list: `Name <a@b>`, `"Name" <a@b>` or `a@b`. `None` when
/// the address isn't valid.
pub fn parse_mailbox(entry: &str) -> Option<Mailbox> {
    let (name, email) = match (entry.rfind('<'), entry.ends_with('>')) {
        (Some(open), true) => {
            let name = entry[..open].trim().trim_matches('"').trim();
            (
                (!name.is_empty()).then(|| name.to_owned()),
                &entry[open + 1..entry.len() - 1],
            )
        }
        _ => (None, entry),
    };
    let email = email.trim();
    valid_email(email).then(|| Mailbox {
        name,
        email: email.to_owned(),
    })
}

/// Whether `email` looks like `local@domain`.
pub fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    let allowed = |c: char| !c.is_whitespace() && !"<>()[],;:\"\\@".contains(c);
    !local.is_empty()
        && !domain.is_empty()
        && local.chars().all(allowed)
        && domain.chars().all(allowed)
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
}

/// The valid addresses of a list, skipping anything else: for lists still
/// being typed, and headers as they were stored.
pub fn parse_lenient(input: &str) -> Vec<Mailbox> {
    split(input).filter_map(parse_mailbox).collect()
}
