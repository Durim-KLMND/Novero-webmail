//! Calendar invitations in mail: finding the `text/calendar` part of an
//! incoming message, and building the answer that goes back.

use mail_builder::MessageBuilder;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::date::Date;
use mail_builder::mime::{BodyPart, MimePart};
use mail_core::address::Mailbox;
use mail_core::{Error, Result};
use mail_parser::{Message, MessageParser, MimeHeaders};

use crate::compose::{BuiltMessage, Sender, address, crlf};

/// The iCalendar text of a message's invitation, if it carries one: a
/// `text/calendar` part, or an attached `.ics` file.
pub fn calendar_text(raw: &[u8]) -> Option<String> {
    calendar_part(&MessageParser::default().parse(raw)?)
}

pub(crate) fn calendar_part(message: &Message<'_>) -> Option<String> {
    message.parts.iter().find_map(|part| {
        let calendar = part.content_type().is_some_and(|ct| {
            let sub = ct.subtype().unwrap_or_default();
            (ct.ctype().eq_ignore_ascii_case("text") && sub.eq_ignore_ascii_case("calendar"))
                || (ct.ctype().eq_ignore_ascii_case("application")
                    && sub.eq_ignore_ascii_case("ics"))
        }) || part
            .attachment_name()
            .is_some_and(|n| n.to_ascii_lowercase().ends_with(".ics"));
        if !calendar {
            return None;
        }
        let text = match part.text_contents() {
            Some(text) => text.to_owned(),
            None => String::from_utf8_lossy(part.contents()).into_owned(),
        };
        text.contains("BEGIN:VCALENDAR").then_some(text)
    })
}

/// An answer to an invitation: a short note for people, and the iCalendar
/// reply (`METHOD:REPLY`) that calendars read.
pub struct InviteAnswer<'a> {
    pub to: &'a Mailbox,
    pub subject: &'a str,
    pub note: &'a str,
    pub calendar: &'a str,
    pub message_id: &'a str,
    /// The invitation's Message-ID, to thread the answer under it.
    pub in_reply_to: Option<&'a str>,
}

pub fn build_invite_answer(
    sender: Sender<'_>,
    answer: &InviteAnswer<'_>,
    date: i64,
) -> Result<BuiltMessage> {
    let from = Mailbox {
        name: Some(sender.name.to_owned()).filter(|n| !n.trim().is_empty()),
        email: sender.email.to_owned(),
    };
    let body = MimePart::new(
        "multipart/alternative",
        vec![
            MimePart::new("text/plain", BodyPart::Text(crlf(answer.note).into())),
            MimePart::new(
                ContentType::new("text/calendar")
                    .attribute("method", "REPLY")
                    .attribute("charset", "utf-8"),
                BodyPart::Text(answer.calendar.to_owned().into()),
            ),
        ],
    );
    let mut message = MessageBuilder::new()
        .from(address(&from))
        .to(address(answer.to))
        .subject(answer.subject)
        .message_id(answer.message_id)
        .date(Date::new(date))
        .body(body);
    if let Some(id) = answer.in_reply_to {
        message = message.in_reply_to(id);
    }
    let raw = message
        .write_to_vec()
        .map_err(|e| Error::Storage(e.to_string()))?;
    Ok(BuiltMessage {
        copy: raw.clone(),
        raw,
        recipients: vec![answer.to.email.clone()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVITE: &str = "From: Ann <ann@work.test>\r\nTo: durim@example.test\r\nSubject: Invitation: Planning\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\n\
You are invited.\r\n--b\r\nContent-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\n\
BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:1\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n--b--\r\n";

    #[test]
    fn finds_the_calendar_part() {
        let text = calendar_text(INVITE.as_bytes()).unwrap();
        assert!(text.starts_with("BEGIN:VCALENDAR"));
        assert!(text.contains("UID:1"));
        let plain = "From: a@b.test\r\nSubject: hi\r\n\r\nNo invitation here.\r\n";
        assert_eq!(calendar_text(plain.as_bytes()), None);
    }

    #[test]
    fn answers_carry_a_calendar_reply() {
        let to = Mailbox {
            name: Some("Ann".into()),
            email: "ann@work.test".into(),
        };
        let built = build_invite_answer(
            Sender {
                name: "Durim K",
                email: "durim@example.test",
            },
            &InviteAnswer {
                to: &to,
                subject: "Accepted: Planning",
                note: "Durim K accepted.",
                calendar: "BEGIN:VCALENDAR\r\nMETHOD:REPLY\r\nEND:VCALENDAR\r\n",
                message_id: "x@example.test",
                in_reply_to: Some("inv@work.test"),
            },
            1_800_000_000,
        )
        .unwrap();
        let text = String::from_utf8(built.raw).unwrap();
        assert!(text.contains("text/calendar; method=\"REPLY\""), "{text}");
        assert!(text.contains("METHOD:REPLY"));
        assert!(text.contains("In-Reply-To: <inv@work.test>"));
        assert_eq!(built.recipients, ["ann@work.test"]);
    }
}
