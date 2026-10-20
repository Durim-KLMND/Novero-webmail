//! Building outgoing mail, and pre-filling replies, forwards and reopened drafts.

use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address as BuilderAddress;
use mail_builder::headers::date::Date;
use mail_builder::headers::message_id::MessageId;
use mail_builder::mime::{BodyPart, MimePart};
pub use mail_core::address::Mailbox;
use mail_core::address::{parse_lenient, parse_mailbox, split, valid_email};
use mail_core::{Draft, DraftAttachment, DraftFields, Error, Result};
use mail_parser::{Address, DateTime, HeaderValue, Message, MessageParser, MimeHeaders};

/// Threading headers kept from the original message. Long threads keep only
/// the newest references, which is all mail clients use to thread.
const MAX_REFERENCES: usize = 20;

/// Splits a typed address list on commas and semicolons outside quotes and
/// angle brackets. Every entry must be a valid address.
pub fn parse_addresses(input: &str) -> Result<Vec<Mailbox>> {
    split(input)
        .map(|entry| {
            parse_mailbox(entry)
                .ok_or_else(|| Error::Invalid(format!("“{entry}” is not a valid email address")))
        })
        .collect()
}

/// Who a message is from.
#[derive(Debug, Clone, Copy)]
pub struct Sender<'a> {
    pub name: &'a str,
    pub email: &'a str,
}

/// An outgoing message in two forms, plus where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltMessage {
    /// What is sent over SMTP: no Bcc header, so Bcc recipients stay hidden.
    pub raw: Vec<u8>,
    /// The sender's own copy for Sent and Drafts, with the Bcc header kept.
    pub copy: Vec<u8>,
    /// Bare addresses of every recipient, Bcc included.
    pub recipients: Vec<String>,
}

/// Builds a draft into a message. When `sending`, every address must be valid
/// and there must be a recipient; otherwise addresses still being typed are
/// left out, so a half-written draft can always be saved.
/// `files` holds each attachment's contents, in the draft's order.
pub fn build_message(
    sender: Sender<'_>,
    draft: &Draft,
    files: &[Vec<u8>],
    date: i64,
    sending: bool,
) -> Result<BuiltMessage> {
    let parse = |input: &str| {
        if sending {
            parse_addresses(input)
        } else {
            Ok(parse_lenient(input))
        }
    };
    let to = parse(&draft.fields.to)?;
    let cc = parse(&draft.fields.cc)?;
    let bcc = parse(&draft.fields.bcc)?;
    let recipients: Vec<String> = to
        .iter()
        .chain(&cc)
        .chain(&bcc)
        .map(|m| m.email.clone())
        .collect();
    if sending && recipients.is_empty() {
        return Err(Error::Invalid("add at least one recipient".into()));
    }
    if files.len() != draft.attachments.len() {
        return Err(Error::Storage(
            "an attachment of this draft is missing".into(),
        ));
    }

    let build = |with_bcc: bool| -> Result<Vec<u8>> {
        let mut message = MessageBuilder::new()
            .from(address(&Mailbox {
                name: Some(sender.name.to_owned()).filter(|n| !n.trim().is_empty()),
                email: sender.email.to_owned(),
            }))
            .subject(draft.fields.subject.trim())
            .message_id(draft.message_id.as_str())
            .date(Date::new(date));
        // Plain text, or plain and formatted versions; formatted text with
        // pictures carries them next to it (multipart/related).
        let text = MimePart::new(
            "text/plain",
            BodyPart::Text(crlf(&draft.fields.body).into()),
        );
        let mut body = if draft.fields.html.trim().is_empty() {
            text
        } else {
            let id_base = draft.message_id.trim_matches(['<', '>']);
            let (html, images) =
                crate::inline_images(&crate::clean_compose_html(&draft.fields.html), id_base);
            let html = MimePart::new(
                "text/html",
                BodyPart::Text(format!("<!DOCTYPE html><html><body>{html}</body></html>").into()),
            );
            let html = if images.is_empty() {
                html
            } else {
                let mut parts = vec![html];
                parts.extend(images.into_iter().map(|image| {
                    MimePart::new(image.content_type, image.data)
                        .inline()
                        .cid(image.content_id)
                }));
                MimePart::new("multipart/related", parts)
            };
            MimePart::new("multipart/alternative", vec![text, html])
        };
        if !draft.attachments.is_empty() {
            let mut parts = vec![body];
            for (attachment, contents) in draft.attachments.iter().zip(files) {
                parts.push(
                    MimePart::new(attachment.mime_type.as_str(), contents.as_slice())
                        .attachment(attachment.name.as_str()),
                );
            }
            body = MimePart::new("multipart/mixed", parts);
        }
        message = message.body(body);
        if !to.is_empty() {
            message = message.to(address_list(&to));
        }
        if !cc.is_empty() {
            message = message.cc(address_list(&cc));
        }
        if with_bcc && !bcc.is_empty() {
            message = message.bcc(address_list(&bcc));
        }
        if let Some(id) = &draft.in_reply_to {
            message = message.in_reply_to(id.as_str());
        }
        if !draft.references.is_empty() {
            message = message.references(MessageId::new_list(draft.references.iter()));
        }
        message
            .write_to_vec()
            .map_err(|e| Error::Storage(e.to_string()))
    };

    Ok(BuiltMessage {
        raw: build(false)?,
        copy: build(true)?,
        recipients,
    })
}

pub(crate) fn address(mailbox: &Mailbox) -> BuilderAddress<'static> {
    match &mailbox.name {
        Some(name) => BuilderAddress::new_address(Some(name.clone()), mailbox.email.clone()),
        None => BuilderAddress::new_address(None::<String>, mailbox.email.clone()),
    }
}

fn address_list(list: &[Mailbox]) -> BuilderAddress<'static> {
    BuilderAddress::new_list(list.iter().map(address).collect())
}

/// Mail lines end in CRLF; the editor gives bare LF.
pub(crate) fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Which addresses a reply goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyMode {
    /// The sender only (or their Reply-To).
    Sender,
    /// The sender and every other recipient, except the user.
    All,
}

/// What a new draft starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefill {
    pub fields: DraftFields,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Attachments with their contents.
    pub attachments: Vec<(DraftAttachment, Vec<u8>)>,
}

/// Starts a reply to `raw`, quoting its text. `own` lists the user's own
/// addresses, which a reply to all leaves out.
pub fn reply(raw: &[u8], mode: ReplyMode, own: &[&str]) -> Result<Prefill> {
    let message = parse(raw)?;
    let is_own = |m: &Mailbox| own.iter().any(|o| o.eq_ignore_ascii_case(&m.email));

    let target = message
        .reply_to()
        .or(message.from())
        .map(mailboxes)
        .unwrap_or_default();
    let mut to: Vec<Mailbox> = Vec::new();
    let mut cc: Vec<Mailbox> = Vec::new();
    let add = |list: &mut Vec<Mailbox>, m: Mailbox, seen: &[Mailbox]| {
        let duplicate = |x: &Mailbox| x.email.eq_ignore_ascii_case(&m.email);
        if !list.iter().any(duplicate) && !seen.iter().any(duplicate) {
            list.push(m);
        }
    };
    for m in target {
        add(&mut to, m, &[]);
    }
    if mode == ReplyMode::All {
        // Replying to one's own message goes to its recipients instead.
        if to.iter().all(is_own) {
            to.clear();
        }
        for m in message.to().map(mailboxes).unwrap_or_default() {
            if !is_own(&m) {
                add(&mut to, m, &[]);
            }
        }
        for m in message.cc().map(mailboxes).unwrap_or_default() {
            if !is_own(&m) {
                add(&mut cc, m, &to);
            }
        }
    }

    let mut references = ids(message.references());
    if references.is_empty()
        && let Some(parent) = ids(message.in_reply_to()).into_iter().next()
    {
        references.push(parent);
    }
    let message_id = message.message_id().map(str::to_owned);
    if let Some(id) = &message_id {
        references.retain(|r| r != id);
        references.push(id.clone());
    }
    let skip = references.len().saturating_sub(MAX_REFERENCES);
    references.drain(..skip);

    let from = message
        .from()
        .and_then(|a| mailboxes(a).into_iter().next())
        .map(|m| m.display())
        .unwrap_or_else(|| "someone".into());
    let intro = match message.date() {
        Some(date) => format!("On {}, {from} wrote:", human_date(date)),
        None => format!("{from} wrote:"),
    };
    let body = format!("\n\n{intro}\n{}", quote(&body_text(&message)));

    Ok(Prefill {
        fields: DraftFields {
            to: join(&to),
            cc: join(&cc),
            bcc: String::new(),
            subject: prefixed("Re:", message.subject()),
            body,
            html: String::new(),
        },
        in_reply_to: message_id,
        references,
        attachments: Vec::new(),
    })
}

/// Starts a forward of `raw`: its text below a short header, and its attachments.
pub fn forward(raw: &[u8]) -> Result<Prefill> {
    let message = parse(raw)?;
    let mut header = String::from("\n\n---------- Forwarded message ----------\n");
    let mut line = |name: &str, value: Option<String>| {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            header.push_str(&format!("{name}: {value}\n"));
        }
    };
    line("From", message.from().map(|a| join(&mailboxes(a))));
    line("Date", message.date().map(human_date));
    line("Subject", message.subject().map(str::to_owned));
    line("To", message.to().map(|a| join(&mailboxes(a))));
    line("Cc", message.cc().map(|a| join(&mailboxes(a))));
    Ok(Prefill {
        fields: DraftFields {
            subject: prefixed("Fwd:", message.subject()),
            body: format!("{header}\n{}", body_text(&message)),
            ..DraftFields::default()
        },
        attachments: attachments(&message),
        ..Prefill::default()
    })
}

/// Reads a saved draft (e.g. from the Drafts folder) back into editable form.
pub fn reopen(raw: &[u8]) -> Result<Prefill> {
    let message = parse(raw)?;
    let list = |a: Option<&Address<'_>>| a.map(|a| join(&mailboxes(a))).unwrap_or_default();
    Ok(Prefill {
        fields: DraftFields {
            to: list(message.to()),
            cc: list(message.cc()),
            bcc: list(message.bcc()),
            subject: message.subject().unwrap_or_default().to_owned(),
            body: body_text(&message).replace("\r\n", "\n"),
            // Formatting is kept when the draft was written with it.
            html: message
                .html_part(0)
                .filter(|p| p.is_text_html())
                .and_then(|p| p.text_contents())
                .map(|html| {
                    let html = crate::restore_inline_images(body_of(html), &inline_parts(&message));
                    crate::clean_compose_html(&html)
                })
                .unwrap_or_default(),
        },
        in_reply_to: ids(message.in_reply_to()).into_iter().next(),
        references: ids(message.references()),
        attachments: attachments(&message),
    })
}

/// Pictures a message carries for its HTML: content id, type and bytes.
fn inline_parts(message: &Message<'_>) -> Vec<(String, String, Vec<u8>)> {
    message
        .parts
        .iter()
        .filter_map(|part| {
            let id = part.content_id()?.trim_matches(['<', '>']).to_owned();
            let ct = part.content_type()?;
            let kind = format!("{}/{}", ct.ctype(), ct.subtype()?).to_ascii_lowercase();
            kind.starts_with("image/")
                .then(|| (id, kind, part.contents().to_vec()))
        })
        .collect()
}

/// The part of an HTML document inside `<body>`, or all of it.
fn body_of(html: &str) -> &str {
    let lower = html.to_ascii_lowercase();
    let start = lower
        .find("<body")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .unwrap_or(0);
    let end = lower
        .rfind("</body>")
        .filter(|&e| e >= start)
        .unwrap_or(html.len());
    &html[start..end]
}

/// A new Message-ID (without angle brackets) in the sender's domain. The
/// random part comes from the standard library's per-process hash keys.
pub fn new_message_id(email: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let domain = email
        .rsplit_once('@')
        .map(|(_, d)| d)
        .filter(|d| valid_email(&format!("x@{d}")))
        .unwrap_or("novero.invalid");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(nanos);
    format!("{nanos:x}.{:016x}@{domain}", hasher.finish())
}

/// The Message-ID of a raw message, without angle brackets.
pub fn message_id_of(raw: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse_headers(raw)
        .and_then(|m| m.message_id().map(str::to_owned))
}

fn parse(raw: &[u8]) -> Result<Message<'_>> {
    MessageParser::default()
        .parse(raw)
        .ok_or_else(|| Error::Protocol("message could not be parsed".into()))
}

fn mailboxes(address: &Address<'_>) -> Vec<Mailbox> {
    address
        .iter()
        .filter_map(|a| {
            Some(Mailbox {
                name: a
                    .name()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned),
                email: a.address()?.to_owned(),
            })
        })
        .collect()
}

fn join(list: &[Mailbox]) -> String {
    list.iter()
        .map(Mailbox::display)
        .collect::<Vec<_>>()
        .join(", ")
}

fn ids(value: &HeaderValue<'_>) -> Vec<String> {
    match (value.as_text_list(), value.as_text()) {
        (Some(list), _) => list.iter().map(|s| s.to_string()).collect(),
        (None, Some(one)) => vec![one.to_owned()],
        _ => Vec::new(),
    }
}

/// The text of a message, converted from HTML when it has no plain part.
fn body_text(message: &Message<'_>) -> String {
    message
        .body_text(0)
        .map(|t| t.trim_end().replace("\r\n", "\n"))
        .unwrap_or_default()
}

fn attachments(message: &Message<'_>) -> Vec<(DraftAttachment, Vec<u8>)> {
    message
        .attachments()
        // Pictures inside the formatted text come back with it, not as files.
        .filter(|part| {
            let image = part
                .content_type()
                .is_some_and(|ct| ct.ctype().eq_ignore_ascii_case("image"));
            !(image && part.content_id().is_some())
        })
        .filter_map(|part| {
            let name = part.attachment_name()?.to_owned();
            let mime_type = part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(sub) => format!("{}/{}", ct.ctype(), sub),
                    None => ct.ctype().to_owned(),
                })
                .unwrap_or_else(|| "application/octet-stream".into());
            let contents = part.contents().to_vec();
            Some((
                DraftAttachment {
                    name,
                    mime_type,
                    size: contents.len() as u64,
                },
                contents,
            ))
        })
        .collect()
}

fn quote(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() || line.starts_with('>') {
                format!(">{line}")
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `Re: Subject`, without stacking another prefix on `Re: Re: …`.
fn prefixed(prefix: &str, subject: Option<&str>) -> String {
    let subject = subject.unwrap_or_default().trim();
    let already = subject
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix));
    if already {
        subject.to_owned()
    } else {
        format!("{prefix} {subject}").trim_end().to_owned()
    }
}

/// `7 Jan 2026 at 09:05`, in the time zone the message was written in.
fn human_date(date: &DateTime) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS
        .get(usize::from(date.month).saturating_sub(1))
        .unwrap_or(&"");
    format!(
        "{} {month} {} at {:02}:{:02}",
        date.day, date.year, date.hour, date.minute
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::AccountId;

    fn draft(fields: DraftFields) -> Draft {
        Draft {
            id: 1,
            account: AccountId(1),
            message_id: "draft-1@novero.test".into(),
            fields,
            attachments: Vec::new(),
            in_reply_to: None,
            references: Vec::new(),
            replying_to: None,
        }
    }

    const ME: Sender<'static> = Sender {
        name: "Alice Example",
        email: "alice@example.test",
    };

    #[test]
    fn message_ids_are_unique_and_use_the_sender_domain() {
        let a = new_message_id("alice@example.test");
        let b = new_message_id("alice@example.test");
        assert_ne!(a, b);
        assert!(a.ends_with("@example.test"));
        assert!(new_message_id("broken").ends_with("@novero.invalid"));
    }

    #[test]
    fn address_lists_split_outside_quotes_and_brackets() {
        let list =
            parse_addresses("\"Doe, Jane\" <jane@example.test>; bob@example.test, Carl <c@x.test>")
                .unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name.as_deref(), Some("Doe, Jane"));
        assert_eq!(list[0].email, "jane@example.test");
        assert_eq!(list[1].name, None);
        assert_eq!(list[2].display(), "Carl <c@x.test>");
        assert_eq!(list[0].display(), "\"Doe, Jane\" <jane@example.test>");
        assert!(parse_addresses(" , ").unwrap().is_empty());
    }

    #[test]
    fn bad_addresses_are_named_in_the_error() {
        for bad in [
            "bob",
            "bob@",
            "@x.test",
            "a b@x.test",
            "a@x..test",
            "<a@x.test",
        ] {
            let err = parse_addresses(&format!("ok@x.test, {bad}")).unwrap_err();
            assert!(err.to_string().contains(bad), "{bad}: {err}");
        }
    }

    #[test]
    fn sent_mail_hides_bcc_and_the_copy_keeps_it() {
        let mut d = draft(DraftFields {
            to: "Bob <bob@example.test>".into(),
            cc: "carol@example.test".into(),
            bcc: "dave@example.test".into(),
            subject: "Grüße".into(),
            body: "Hi Bob,\nsee attached.".into(),
            ..DraftFields::default()
        });
        d.in_reply_to = Some("orig@example.test".into());
        d.references = vec!["root@example.test".into(), "orig@example.test".into()];
        d.attachments.push(DraftAttachment {
            name: "notes.txt".into(),
            mime_type: "text/plain".into(),
            size: 5,
        });
        let built = build_message(ME, &d, &[b"notes".to_vec()], 1_792_400_400, true).unwrap();
        assert_eq!(
            built.recipients,
            [
                "bob@example.test",
                "carol@example.test",
                "dave@example.test"
            ]
        );

        let sent = MessageParser::default().parse(&built.raw).unwrap();
        assert!(sent.bcc().is_none());
        assert_eq!(sent.subject(), Some("Grüße"));
        assert_eq!(sent.message_id(), Some("draft-1@novero.test"));
        assert_eq!(sent.in_reply_to().as_text(), Some("orig@example.test"));
        assert_eq!(ids(sent.references()).len(), 2);
        assert_eq!(
            sent.from().unwrap().first().unwrap().name(),
            Some("Alice Example")
        );
        assert_eq!(
            sent.body_text(0).unwrap().trim_end(),
            "Hi Bob,\r\nsee attached."
        );
        assert_eq!(sent.attachments().next().unwrap().contents(), b"notes");
        assert_eq!(sent.date().unwrap().to_timestamp(), 1_792_400_400);

        let copy = MessageParser::default().parse(&built.copy).unwrap();
        assert_eq!(
            copy.bcc().unwrap().first().unwrap().address(),
            Some("dave@example.test")
        );
    }

    #[test]
    fn formatted_mail_carries_both_versions_and_reopens_formatted() {
        let d = draft(DraftFields {
            to: "bob@example.test".into(),
            subject: "Plan".into(),
            body: "Hi Bob,\n- one\n- two".into(),
            html: r#"<div>Hi <b>Bob</b>,</div><ul><li>one</li><li><a href="https://example.test" onclick="x()">two</a></li></ul><script>alert(1)</script><img src="https://track.example/p.gif">"#.into(),
            ..DraftFields::default()
        });
        let built = build_message(ME, &d, &[], 1_792_400_400, true).unwrap();
        let sent = MessageParser::default().parse(&built.raw).unwrap();
        assert_eq!(
            sent.body_text(0).unwrap().trim_end(),
            "Hi Bob,\r\n- one\r\n- two"
        );
        let html = sent.body_html(0).unwrap();
        assert!(html.contains("Hi <b>Bob</b>,"), "{html}");
        assert!(
            html.contains(r#"<a href="https://example.test" rel="noopener noreferrer">two</a>"#),
            "{html}"
        );
        assert!(
            !html.contains("script") && !html.contains("onclick") && !html.contains("img"),
            "{html}"
        );

        let reopened = reopen(&built.copy).unwrap();
        assert_eq!(reopened.fields.body, "Hi Bob,\n- one\n- two");
        assert!(
            reopened
                .fields
                .html
                .starts_with("<div>Hi <b>Bob</b>,</div><ul>"),
            "{}",
            reopened.fields.html
        );

        // Pasted pictures travel as inline parts and come back on reopening.
        let pixel = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let pictured = draft(DraftFields {
            to: "bob@example.test".into(),
            body: "Look".into(),
            html: format!(
                r#"<div>Look</div><img src="data:image/png;base64,{pixel}" alt="dot"><img src="data:image/png;base64,{pixel}"><img src="data:text/html;base64,PGI+"><img src="https://track.example/p.gif">"#
            ),
            ..DraftFields::default()
        });
        let built = build_message(ME, &pictured, &[], 1_792_400_400, true).unwrap();
        let text = String::from_utf8_lossy(&built.raw);
        assert!(
            text.contains("multipart/alternative") && text.contains("multipart/related"),
            "{text}"
        );
        let sent = MessageParser::default().parse(&built.raw).unwrap();
        let html = sent.body_html(0).unwrap();
        assert_eq!(html.matches("src=\"cid:image1.").count(), 2, "{html}");
        assert!(
            !html.contains("data:") && !html.contains("track.example"),
            "{html}"
        );
        let images: Vec<_> = sent
            .parts
            .iter()
            .filter(|p| p.content_id().is_some())
            .collect();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].contents().len(), 70);
        let reopened = reopen(&built.copy).unwrap();
        assert_eq!(
            reopened
                .fields
                .html
                .matches(&format!("data:image/png;base64,{pixel}"))
                .count(),
            2
        );
        assert!(reopened.attachments.is_empty());

        // Plain-text drafts stay plain.
        let plain = draft(DraftFields {
            to: "bob@example.test".into(),
            body: "Just text".into(),
            ..DraftFields::default()
        });
        let built = build_message(ME, &plain, &[], 1_792_400_400, true).unwrap();
        let sent = MessageParser::default().parse(&built.raw).unwrap();
        assert!(sent.html_part(0).is_none_or(|p| !p.is_text_html()));
        assert_eq!(reopen(&built.raw).unwrap().fields.html, "");
    }

    #[test]
    fn sending_needs_valid_recipients_but_saving_does_not() {
        let d = draft(DraftFields {
            to: "bob@".into(),
            ..DraftFields::default()
        });
        assert!(matches!(
            build_message(ME, &d, &[], 0, true),
            Err(Error::Invalid(_))
        ));
        assert!(build_message(ME, &d, &[], 0, false).is_ok());
        let empty = draft(DraftFields::default());
        assert!(build_message(ME, &empty, &[], 0, true).is_err());
    }

    const ORIGINAL: &[u8] = b"From: Bob <bob@example.test>\r\n\
To: Alice <alice@example.test>, carol@example.test\r\n\
Cc: dave@example.test, ALICE@example.test\r\n\
Subject: Lunch\r\n\
Date: Mon, 19 Oct 2026 09:05:00 +0200\r\n\
Message-ID: <m2@example.test>\r\n\
References: <m0@example.test> <m1@example.test>\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Noon?\r\n\
\r\n\
> earlier\r\n\
--b\r\n\
Content-Type: application/pdf\r\n\
Content-Disposition: attachment; filename=\"menu.pdf\"\r\n\
\r\n\
%PDF\r\n\
--b--\r\n";

    #[test]
    fn reply_goes_to_the_sender_and_quotes_the_text() {
        let p = reply(ORIGINAL, ReplyMode::Sender, &["alice@example.test"]).unwrap();
        assert_eq!(p.fields.to, "Bob <bob@example.test>");
        assert_eq!(p.fields.cc, "");
        assert_eq!(p.fields.subject, "Re: Lunch");
        assert_eq!(p.in_reply_to.as_deref(), Some("m2@example.test"));
        assert_eq!(
            p.references,
            ["m0@example.test", "m1@example.test", "m2@example.test"]
        );
        assert!(
            p.fields.body.contains(
                "On 19 Oct 2026 at 09:05, Bob <bob@example.test> wrote:\n> Noon?\n>\n>> earlier"
            ),
            "{}",
            p.fields.body
        );
        assert!(p.attachments.is_empty());
    }

    #[test]
    fn reply_all_adds_everyone_but_the_user() {
        let p = reply(ORIGINAL, ReplyMode::All, &["alice@example.test"]).unwrap();
        assert_eq!(p.fields.to, "Bob <bob@example.test>, carol@example.test");
        assert_eq!(p.fields.cc, "dave@example.test");
        let again = reply(b"Subject: re: Lunch\r\n\r\nx", ReplyMode::Sender, &[]).unwrap();
        assert_eq!(again.fields.subject, "re: Lunch");
    }

    #[test]
    fn forward_keeps_the_text_and_attachments() {
        let p = forward(ORIGINAL).unwrap();
        assert_eq!(p.fields.subject, "Fwd: Lunch");
        assert_eq!(p.fields.to, "");
        assert!(p.fields.body.contains("From: Bob <bob@example.test>\n"));
        assert!(p.fields.body.contains("Subject: Lunch\n"));
        assert!(p.fields.body.contains("Noon?"));
        assert_eq!(p.attachments.len(), 1);
        assert_eq!(p.attachments[0].0.name, "menu.pdf");
        assert_eq!(p.attachments[0].0.mime_type, "application/pdf");
        assert_eq!(p.in_reply_to, None);
    }

    #[test]
    fn a_built_draft_reopens_with_its_fields() {
        let mut d = draft(DraftFields {
            to: "Bob <bob@example.test>".into(),
            bcc: "dave@example.test".into(),
            subject: "Plan".into(),
            body: "Line one\nLine two".into(),
            ..DraftFields::default()
        });
        d.in_reply_to = Some("m2@example.test".into());
        d.attachments.push(DraftAttachment {
            name: "a.bin".into(),
            mime_type: "application/octet-stream".into(),
            size: 3,
        });
        let built = build_message(ME, &d, &[vec![1, 2, 3]], 0, false).unwrap();
        let p = reopen(&built.copy).unwrap();
        assert_eq!(p.fields.to, d.fields.to);
        assert_eq!(p.fields.bcc, d.fields.bcc);
        assert_eq!(p.fields.subject, "Plan");
        assert_eq!(p.fields.body.trim_end(), "Line one\nLine two");
        assert_eq!(p.in_reply_to.as_deref(), Some("m2@example.test"));
        assert_eq!(p.attachments[0].1, vec![1, 2, 3]);
        assert_eq!(
            message_id_of(&built.copy).as_deref(),
            Some("draft-1@novero.test")
        );
    }
}
