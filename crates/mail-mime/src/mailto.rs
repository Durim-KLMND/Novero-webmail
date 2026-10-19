//! `mailto:` links (RFC 6068), which Windows hands to the default email app.

use mail_core::DraftFields;

/// The draft a `mailto:` link asks for. Anything that isn't a `mailto:`
/// link gives `None`; unknown fields such as `In-Reply-To` are ignored.
pub fn parse_mailto(link: &str) -> Option<DraftFields> {
    let rest = strip_prefix_ignore_case(link.trim(), "mailto:")?;
    let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut fields = DraftFields::default();
    let mut to = vec![decode(to)];
    let (mut cc, mut bcc) = (Vec::new(), Vec::new());
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        match decode(name).to_ascii_lowercase().as_str() {
            "to" => to.push(value),
            "cc" => cc.push(value),
            "bcc" => bcc.push(value),
            "subject" => fields.subject = value,
            "body" => fields.body = value.replace("\r\n", "\n"),
            _ => {}
        }
    }
    fields.to = join(&to);
    fields.cc = join(&cc);
    fields.bcc = join(&bcc);
    Some(fields)
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Address lists as the composer shows them: comma separated, no blanks.
fn join(lists: &[String]) -> String {
    lists
        .iter()
        .flat_map(|list| list.split(','))
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Percent-decodes as UTF-8. Unlike form data, `+` stays a plus sign.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_addresses_and_fields() {
        let f = parse_mailto(
            "mailto:ana@example.com,%20bo@example.com?cc=cy@example.com&Subject=Hi%20there%20%E2%9C%93\
             &body=Line%201%0D%0ALine%202&bcc=di@example.com&in-reply-to=%3Cx%40y%3E",
        )
        .unwrap();
        assert_eq!(f.to, "ana@example.com, bo@example.com");
        assert_eq!(f.cc, "cy@example.com");
        assert_eq!(f.bcc, "di@example.com");
        assert_eq!(f.subject, "Hi there ✓");
        assert_eq!(f.body, "Line 1\nLine 2");
    }

    #[test]
    fn handles_odd_links() {
        let f = parse_mailto("MAILTO:?to=a+b@example.com&to=c@example.com&subject=50%").unwrap();
        assert_eq!(f.to, "a+b@example.com, c@example.com");
        assert_eq!(f.subject, "50%");
        assert_eq!(parse_mailto("mailto:").unwrap(), DraftFields::default());
        assert_eq!(parse_mailto("https://example.com"), None);
        assert_eq!(parse_mailto("mail"), None);
    }
}
