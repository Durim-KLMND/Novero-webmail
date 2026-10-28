//! Emoji reactions sent as mail: a short message that replies to another and
//! carries one emoji, such as a thumbs up. Three forms are read:
//!
//! - RFC 9078: a part with `Content-Disposition: reaction` holding the emoji.
//! - Gmail: a `text/vnd.google.email-reaction+json` part, `{"version":1,"emoji":"👍"}`.
//! - Outlook: plain text starting "<emoji> <name> reacted to your message:".

use mail_parser::{Message, MessageParser, MimeHeaders, PartType};

use crate::parse::bracketed;

/// A reaction and the message it is to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: String,
    /// The Message-ID reacted to, in angle brackets.
    pub to: String,
}

/// Messages larger than this are not reactions, however they look.
pub const MAX_REACTION_SIZE: usize = 64 * 1024;

/// The reaction `raw` is, if it is one.
pub fn reaction(raw: &[u8]) -> Option<Reaction> {
    if raw.len() > MAX_REACTION_SIZE {
        return None;
    }
    let message = MessageParser::default().parse(raw)?;
    // A reaction answers exactly one message.
    let to = message.in_reply_to().as_text().or_else(|| {
        message
            .in_reply_to()
            .as_text_list()
            .and_then(|l| l.first().map(|s| s.as_ref()))
    })?;
    let emoji = tagged(&message).or_else(|| outlook(&message))?;
    Some(Reaction {
        emoji,
        to: bracketed(to),
    })
}

/// The emoji of a part marked as a reaction (RFC 9078 or Gmail's).
fn tagged(message: &Message<'_>) -> Option<String> {
    message.parts.iter().find_map(|part| {
        let PartType::Text(text) = &part.body else {
            return None;
        };
        let disposition = part
            .content_disposition()
            .map(|d| d.ctype().to_ascii_lowercase());
        if disposition.as_deref() == Some("reaction") {
            return emoji(text);
        }
        let kind = part.content_type()?;
        let gmail = kind.ctype().eq_ignore_ascii_case("text")
            && kind
                .subtype()
                .is_some_and(|s| s.eq_ignore_ascii_case("vnd.google.email-reaction+json"));
        if !gmail || disposition.as_deref() == Some("attachment") {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        if value.get("version")?.as_i64()? != 1 {
            return None;
        }
        emoji(value.get("emoji")?.as_str()?)
    })
}

/// Outlook's reaction mail: its first line is the reaction, as an emoji or
/// as the reaction's name in brackets.
fn outlook(message: &Message<'_>) -> Option<String> {
    let text = message.body_text(0)?;
    let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let rest = first.strip_suffix(':').unwrap_or(first);
    if !rest.ends_with(" reacted to your message") {
        return None;
    }
    let (sign, _) = rest.split_once(char::is_whitespace)?;
    let named = match sign.trim_matches(['[', ']']).to_ascii_lowercase().as_str() {
        "like" => Some("👍"),
        "heart" | "love" => Some("❤️"),
        "celebrate" => Some("🎉"),
        "laugh" => Some("😆"),
        "surprised" => Some("😮"),
        "sad" => Some("😢"),
        _ => None,
    };
    match named {
        Some(e) => Some(e.to_owned()),
        None => emoji(sign),
    }
}

/// `text` when it is a short run of emoji, and nothing else.
fn emoji(text: &str) -> Option<String> {
    let text = text.trim();
    let count = text.chars().count();
    let only_emoji = text.chars().all(|c| {
        !c.is_alphanumeric() && !c.is_ascii() && !c.is_whitespace()
            // Joiners and keycap marks are part of emoji sequences.
            || c == '\u{200d}'
    });
    (count > 0 && count <= 16 && only_emoji).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(body: &str) -> Vec<u8> {
        format!(
            "From: Ann <ann@example.test>\r\nTo: me@example.test\r\nSubject: Re: Lunch\r\n\
             Message-ID: <r1@example.test>\r\nIn-Reply-To: <m1@example.test>\r\n{body}"
        )
        .into_bytes()
    }

    #[test]
    fn reads_rfc_9078_reactions() {
        let raw = mail(
            "Content-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\n\
             Content-Type: text/plain; charset=utf-8\r\nContent-Disposition: reaction\r\n\r\n\
             👍\r\n--b\r\nContent-Type: text/plain\r\n\r\nAnn liked your message\r\n--b--\r\n",
        );
        assert_eq!(
            reaction(&raw),
            Some(Reaction {
                emoji: "👍".into(),
                to: "<m1@example.test>".into()
            })
        );
    }

    #[test]
    fn reads_gmail_reactions() {
        let raw = mail(
            "Content-Type: multipart/alternative; boundary=b\r\n\r\n--b\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\nAnn reacted via Gmail\r\n--b\r\n\
             Content-Type: text/vnd.google.email-reaction+json; charset=utf-8\r\n\r\n\
             {\"version\":1,\"emoji\":\"\u{2764}\u{fe0f}\"}\r\n--b\r\n\
             Content-Type: text/html; charset=utf-8\r\n\r\n<p>Ann reacted via Gmail</p>\r\n--b--\r\n",
        );
        assert_eq!(
            reaction(&raw).map(|r| r.emoji),
            Some("\u{2764}\u{fe0f}".into())
        );
        let wrong = mail(
            "Content-Type: text/vnd.google.email-reaction+json\r\n\r\n{\"version\":\"1\",\"emoji\":\"👍\"}",
        );
        assert_eq!(reaction(&wrong), None);
    }

    #[test]
    fn reads_outlook_reactions() {
        let raw = mail(
            "Content-Type: text/plain; charset=utf-8\r\n\r\n\
             [like] Ann Lee reacted to your message:\r\n______\r\nFrom: me\r\n",
        );
        assert_eq!(reaction(&raw).map(|r| r.emoji), Some("👍".into()));
        let emoji = mail(
            "Content-Type: text/plain; charset=utf-8\r\n\r\n😆 Ann reacted to your message:\r\n",
        );
        assert_eq!(reaction(&emoji).map(|r| r.emoji), Some("😆".into()));
    }

    #[test]
    fn ordinary_replies_are_not_reactions() {
        assert_eq!(
            reaction(&mail("Content-Type: text/plain\r\n\r\n👍\r\n")),
            None
        );
        assert_eq!(
            reaction(&mail(
                "Content-Type: text/plain\r\n\r\nI reacted to your message: no.\r\n"
            )),
            None
        );
        let no_parent = b"From: a@x\r\nContent-Disposition: reaction\r\n\r\n\xf0\x9f\x91\x8d\r\n";
        assert_eq!(reaction(no_parent), None);
    }
}
