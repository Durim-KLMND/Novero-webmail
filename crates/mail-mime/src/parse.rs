//! Decoding raw messages into what the UI shows.

use std::collections::HashMap;

use base64::Engine;
use mail_core::categories::signals;
use mail_core::{Error, MessageHeader, Result};
use mail_parser::{Address, HeaderValue, Message, MessageParser, MessagePart, MimeHeaders};
use serde::Serialize;

use crate::html::sanitize_html;

/// A message ready to display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedMessage {
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub date: Option<i64>,
    /// Plain-text body, when the message has one.
    pub text: Option<String>,
    /// Sanitized HTML body, when the message has one.
    pub html: Option<String>,
    /// Remote images blocked in `html`.
    pub blocked_remote: usize,
    pub attachments: Vec<AttachmentInfo>,
    /// Carries a calendar invitation (see `calendar_text`).
    pub invite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub name: String,
    pub mime_type: String,
    pub size: usize,
}

/// Inline images larger than this are not embedded as data URIs.
const MAX_INLINE_IMAGE: usize = 5 * 1024 * 1024;

/// Parses a full RFC 5322 message and sanitizes its HTML.
pub fn parse_message(raw: &[u8], allow_remote: bool) -> Result<ParsedMessage> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| Error::Protocol("message could not be parsed".into()))?;

    let mut inline_images = HashMap::new();
    let mut attachments = Vec::new();
    for part in message.attachments() {
        let mime_type = mime_type_of(part);
        let contents = part.contents();
        if let Some(cid) = inline_cid(part, &mime_type) {
            let data = base64::engine::general_purpose::STANDARD.encode(contents);
            inline_images.insert(cid, format!("data:{mime_type};base64,{data}"));
        }
        if listed(part, &mime_type) {
            attachments.push(AttachmentInfo {
                name: part.attachment_name().unwrap_or("attachment").to_owned(),
                mime_type,
                size: contents.len(),
            });
        }
    }

    let text = message
        .text_bodies()
        .find(|p| p.is_text() && !p.is_text_html())
        .and_then(|_| message.body_text(0))
        .map(|t| t.into_owned());
    let has_html = message.html_bodies().any(|p| p.is_text_html());
    let sanitized = has_html
        .then(|| message.body_html(0))
        .flatten()
        .map(|html| sanitize_html(&html, inline_images, allow_remote));

    Ok(ParsedMessage {
        subject: message.subject().map(str::to_owned),
        from: message.from().and_then(first_address),
        to: message.to().map(all_addresses),
        cc: message.cc().map(all_addresses),
        date: message.date().map(|d| d.to_timestamp()),
        text,
        blocked_remote: sanitized.as_ref().map_or(0, |s| s.blocked_remote),
        html: sanitized.map(|s| s.html),
        attachments,
        invite: crate::invite::calendar_part(&message).is_some(),
    })
}

/// One attachment of a message and its decoded contents. `index` counts the
/// attachments as [`ParsedMessage::attachments`] lists them.
pub fn attachment(raw: &[u8], index: usize) -> Option<(AttachmentInfo, Vec<u8>)> {
    let message = MessageParser::default().parse(raw)?;
    let part = message
        .attachments()
        .filter(|p| listed(p, &mime_type_of(p)))
        .nth(index)?;
    let contents = part.contents().to_vec();
    let info = AttachmentInfo {
        name: part.attachment_name().unwrap_or("attachment").to_owned(),
        mime_type: mime_type_of(part),
        size: contents.len(),
    };
    Some((info, contents))
}

fn mime_type_of(part: &MessagePart) -> String {
    part.content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_owned(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
        .to_ascii_lowercase()
}

/// The Content-ID a picture is shown by inside the HTML, when it is one.
fn inline_cid(part: &MessagePart, mime_type: &str) -> Option<String> {
    let cid = part.content_id()?;
    (mime_type.starts_with("image/") && part.contents().len() <= MAX_INLINE_IMAGE).then(|| {
        cid.trim_matches(|c| c == '<' || c == '>')
            .to_ascii_lowercase()
    })
}

/// Whether a part shows in the attachment list: everything except unnamed
/// pictures that are part of the HTML.
fn listed(part: &MessagePart, mime_type: &str) -> bool {
    inline_cid(part, mime_type).is_none() || part.attachment_name().is_some()
}

/// Decodes the header block fetched during sync (RFC 2047 words included)
/// into the fields the message list needs. `uid`, `size` and `flags` come
/// from the IMAP response and are left at their defaults.
pub fn parse_header_fields(raw: &[u8]) -> MessageHeader {
    let Some(message) = MessageParser::default().parse_headers(raw) else {
        return MessageHeader::default();
    };
    let has_attachments = message.content_type().is_some_and(|ct| {
        ct.ctype().eq_ignore_ascii_case("multipart")
            && ct
                .subtype()
                .is_some_and(|s| s.eq_ignore_ascii_case("mixed"))
    });
    let mut references: Vec<String> = Vec::new();
    for id in id_list(message.references())
        .into_iter()
        .chain(id_list(message.in_reply_to()))
    {
        if !references.contains(&id) {
            references.push(id);
        }
    }
    let (signals, unsubscribe) = list_signals(&message);
    MessageHeader {
        message_id: message.message_id().map(bracketed),
        references,
        signals,
        unsubscribe,
        from: message.from().and_then(first_address),
        to: message.to().map(all_addresses),
        cc: message.cc().map(all_addresses),
        subject: message.subject().map(str::to_owned),
        date: message.date().map(|d| d.to_timestamp()),
        has_attachments,
        ..MessageHeader::default()
    }
}

/// Headers bulk mailing services add; any of them sets `signals::CAMPAIGN`.
const CAMPAIGN_HEADERS: &[&str] = &[
    "Feedback-ID",
    "X-Campaign",
    "X-CampaignID",
    "X-MC-User",
    "X-SG-EID",
    "X-Mailgun-Tag",
];

/// What the list and bulk-mail headers say about a message, and its
/// unsubscribe link (https preferred over mailto).
fn list_signals(message: &Message<'_>) -> (u32, Option<String>) {
    let raw = |name: &'static str| message.header_raw(name).map(str::trim);
    let starts = |value: Option<&str>, word: &str| {
        value.is_some_and(|v| v.len() >= word.len() && v[..word.len()].eq_ignore_ascii_case(word))
    };
    let mut bits = 0;
    let mut unsubscribe = None;
    if let Some(value) = raw("List-Unsubscribe") {
        bits |= signals::UNSUBSCRIBE;
        let links: Vec<&str> = value
            .split('<')
            .skip(1)
            .filter_map(|part| part.split_once('>').map(|(link, _)| link.trim()))
            .collect();
        unsubscribe = links
            .iter()
            .find(|l| starts(Some(l), "https://"))
            .or_else(|| links.iter().find(|l| starts(Some(l), "mailto:")))
            .map(|l| l.chars().filter(|c| !c.is_whitespace()).collect());
    }
    if raw("List-Unsubscribe-Post").is_some() {
        bits |= signals::ONE_CLICK;
    }
    if raw("List-Id").is_some() {
        bits |= signals::LIST_ID;
    }
    let precedence = raw("Precedence");
    if ["bulk", "junk", "list"]
        .iter()
        .any(|w| starts(precedence, w))
    {
        bits |= signals::BULK;
    }
    let auto = raw("Auto-Submitted");
    if auto.is_some() && !starts(auto, "no") {
        bits |= signals::AUTOMATED;
    }
    if starts(raw("X-Spam-Flag"), "yes") || starts(raw("X-Spam-Status"), "yes") {
        bits |= signals::SPAM_FLAG;
    }
    if CAMPAIGN_HEADERS.iter().any(|h| raw(h).is_some()) {
        bits |= signals::CAMPAIGN;
    }
    (bits, unsubscribe)
}

/// The words of a message for the search index: its text (converted from
/// HTML when it has no plain part) and attachment names. Long messages are
/// cut, since a search rarely needs more than their start.
pub fn search_text(raw: &[u8]) -> Option<String> {
    const LIMIT: usize = 64 * 1024;
    let message = MessageParser::default().parse(raw)?;
    let mut text = message
        .body_text(0)
        .map(|t| t.into_owned())
        .unwrap_or_default();
    for part in message.attachments() {
        if let Some(name) = part.attachment_name() {
            text.push(' ');
            text.push_str(name);
        }
    }
    if text.len() > LIMIT {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Some(text)
}

/// Message-IDs in a References or In-Reply-To header, as `<id>`.
fn id_list(value: &HeaderValue<'_>) -> Vec<String> {
    match (value.as_text_list(), value.as_text()) {
        (Some(list), _) => list.iter().map(|s| bracketed(s)).collect(),
        (None, Some(one)) => vec![bracketed(one)],
        _ => Vec::new(),
    }
}

/// Message-IDs are compared in their `<id>` form, the way headers write them.
pub fn bracketed(id: &str) -> String {
    format!(
        "<{}>",
        id.trim().trim_start_matches('<').trim_end_matches('>')
    )
}

fn format_addr(name: Option<&str>, address: Option<&str>) -> Option<String> {
    match (name.map(str::trim).filter(|n| !n.is_empty()), address) {
        (Some(name), Some(address)) => Some(format!("{name} <{address}>")),
        (Some(name), None) => Some(name.to_owned()),
        (None, Some(address)) => Some(address.to_owned()),
        (None, None) => None,
    }
}

fn first_address(address: &Address<'_>) -> Option<String> {
    address
        .first()
        .and_then(|a| format_addr(a.name(), a.address()))
}

fn all_addresses(address: &Address<'_>) -> String {
    address
        .iter()
        .filter_map(|a| format_addr(a.name(), a.address()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &[u8] = b"From: =?UTF-8?Q?J=C3=BCrgen?= <j@example.test>\r\n\
To: Alice <alice@example.test>, bob@example.test\r\n\
Subject: =?UTF-8?B?w5xiZXJ3ZWlzdW5n?=\r\n\
Date: Mon, 19 Oct 2026 09:00:00 +0000\r\n\
Message-ID: <abc@example.test>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello Alice.\r\n";

    #[test]
    fn plain_text_message_decodes_headers_and_body() {
        let m = parse_message(PLAIN, false).unwrap();
        assert_eq!(m.subject.as_deref(), Some("Überweisung"));
        assert_eq!(m.from.as_deref(), Some("Jürgen <j@example.test>"));
        assert_eq!(
            m.to.as_deref(),
            Some("Alice <alice@example.test>, bob@example.test")
        );
        assert_eq!(m.date, Some(1_792_400_400));
        assert_eq!(m.text.as_deref().map(str::trim), Some("Hello Alice."));
        assert_eq!(m.html, None);
    }

    #[test]
    fn html_message_is_sanitized_and_inline_images_are_embedded() {
        let raw = b"From: a@example.test\r\n\
Subject: News\r\n\
Content-Type: multipart/related; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>Hi<script>x()</script></p><img src=\"cid:logo\"><img src=\"https://t.test/p.gif\">\r\n\
--b\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--b--\r\n";
        let m = parse_message(raw, false).unwrap();
        let html = m.html.unwrap();
        assert!(!html.contains("script"));
        assert!(html.contains("data:image/png;base64,"));
        assert_eq!(m.blocked_remote, 1);
        assert!(
            m.attachments.is_empty(),
            "inline image is not listed as an attachment"
        );
    }

    #[test]
    fn attachments_are_listed() {
        let raw = b"From: a@example.test\r\n\
Subject: Invoice\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
See attached.\r\n\
--b\r\n\
Content-Type: application/pdf; name=\"invoice.pdf\"\r\n\
Content-Disposition: attachment; filename=\"invoice.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQ=\r\n\
--b--\r\n";
        let m = parse_message(raw, false).unwrap();
        assert_eq!(m.attachments.len(), 1);
        assert_eq!(m.attachments[0].name, "invoice.pdf");
        assert_eq!(m.attachments[0].mime_type, "application/pdf");
    }

    #[test]
    fn attachment_contents_follow_the_list() {
        let raw = b"From: a@example.test\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/html\r\n\
\r\n\
<img src=\"cid:logo\">\r\n\
--b\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--b\r\n\
Content-Type: application/PDF; name=\"invoice.pdf\"\r\n\
Content-Disposition: attachment; filename=\"invoice.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQ=\r\n\
--b--\r\n";
        assert_eq!(parse_message(raw, false).unwrap().attachments.len(), 1);
        let (info, contents) = attachment(raw, 0).unwrap();
        assert_eq!(info.name, "invoice.pdf");
        assert_eq!(info.mime_type, "application/pdf");
        assert_eq!(contents, b"%PDF-1.4");
        assert!(attachment(raw, 1).is_none());
    }

    #[test]
    fn header_fields_decode_for_the_message_list() {
        let h = parse_header_fields(PLAIN);
        assert_eq!(h.subject.as_deref(), Some("Überweisung"));
        assert_eq!(h.from.as_deref(), Some("Jürgen <j@example.test>"));
        assert_eq!(h.message_id.as_deref(), Some("<abc@example.test>"));
        assert!(!h.has_attachments);
    }

    #[test]
    fn header_fields_list_what_a_message_replies_to() {
        let h = parse_header_fields(
            b"Message-ID: <c@x>\r\nReferences: <a@x>\r\n <b@x>\r\nIn-Reply-To: <b@x>\r\n\r\n",
        );
        assert_eq!(h.message_id.as_deref(), Some("<c@x>"));
        assert_eq!(h.references, ["<a@x>", "<b@x>"]);
        let h = parse_header_fields(b"In-Reply-To: <only@x>\r\n\r\n");
        assert_eq!(h.references, ["<only@x>"]);
    }

    #[test]
    fn search_text_covers_html_bodies_and_attachment_names() {
        let raw = b"Subject: x\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/html\r\n\r\n<p>Quarterly <b>numbers</b></p>\r\n\
--b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=report.pdf\r\n\r\nxx\r\n--b--\r\n";
        let text = search_text(raw).unwrap();
        assert!(text.contains("Quarterly numbers"), "{text}");
        assert!(text.contains("report.pdf"));
    }

    #[test]
    fn garbage_header_bytes_give_empty_fields() {
        let h = parse_header_fields(b"");
        assert_eq!(h.subject, None);
    }

    #[test]
    fn list_headers_become_category_hints() {
        use mail_core::categories::signals;
        let raw = b"From: Shop <deals@shop.test>\r\n\
List-Unsubscribe: <mailto:leave@shop.test>,\r\n <https://shop.test/leave?u=1>\r\n\
List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\
Precedence: Bulk\r\n\
Feedback-ID: 1:2:shop\r\n\
Auto-Submitted: no\r\n\
X-Spam-Status: No, score=0.1\r\n\r\n";
        let h = parse_header_fields(raw);
        assert_eq!(
            h.signals,
            signals::UNSUBSCRIBE | signals::ONE_CLICK | signals::BULK | signals::CAMPAIGN
        );
        assert_eq!(
            h.unsubscribe.as_deref(),
            Some("https://shop.test/leave?u=1")
        );

        let raw = b"From: x@y.test\r\nList-Unsubscribe: <mailto:off@y.test>\r\n\
X-Spam-Flag: YES\r\nAuto-Submitted: auto-generated\r\n\r\n";
        let h = parse_header_fields(raw);
        assert_eq!(
            h.signals,
            signals::UNSUBSCRIBE | signals::SPAM_FLAG | signals::AUTOMATED
        );
        assert_eq!(h.unsubscribe.as_deref(), Some("mailto:off@y.test"));
        assert_eq!(parse_header_fields(b"From: a@b.test\r\n\r\n").signals, 0);
    }
}
