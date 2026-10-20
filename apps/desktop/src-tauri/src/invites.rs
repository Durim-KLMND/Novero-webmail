//! Calendar invitations that arrive by mail: what the reader shows about
//! them, and answering them.

use mail_calendar::{Invitation, Method};
use mail_core::address::parse_mailbox;
use mail_core::calendar::{Attendee, Event, Response};
use mail_core::{Account, Error, Result};
use mail_mime::{InviteAnswer, Sender};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::calendar::changed;
use crate::commands::raw_message;
use crate::compose::now;
use crate::state::{AppState, push_now, report_outbox};

/// An invitation as the reader shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteView {
    /// `request`, `reply`, `cancel` or `publish`.
    pub method: &'static str,
    /// The event as the message describes it.
    pub event: Event,
    /// The event as the calendar has it, if it is there.
    pub saved: Option<Event>,
    /// For a reply: who answered, and how.
    pub answer: Option<Attendee>,
    /// Whether the user can accept or decline: an invitation from someone else.
    pub can_answer: bool,
    /// Titles of other events at the same time.
    pub conflicts: Vec<String>,
}

fn method_name(method: Method) -> &'static str {
    match method {
        Method::Request => "request",
        Method::Reply => "reply",
        Method::Cancel => "cancel",
        Method::Publish => "publish",
    }
}

async fn invitation(app: &AppHandle, id: i64) -> Result<Option<(Account, Vec<u8>, Invitation)>> {
    let (account, raw) = raw_message(app, id).await?;
    let found = mail_mime::calendar_text(&raw)
        .and_then(|text| mail_calendar::parse(&text))
        .filter(|i| !i.event.uid.is_empty());
    Ok(found.map(|i| (account, raw, i)))
}

/// Whether `organizer` (`Name <email>`) is this account.
fn is_me(account: &Account, organizer: Option<&str>) -> bool {
    organizer
        .and_then(parse_mailbox)
        .is_some_and(|m| m.email.eq_ignore_ascii_case(&account.email))
}

/// The invitation in a message, if it has one. Opening an answer to one of
/// the user's own invitations, or a cancellation, updates the calendar.
#[tauri::command]
pub async fn message_invite(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<InviteView>> {
    let Some((account, _, invite)) = invitation(&app, id).await? else {
        return Ok(None);
    };
    let event = invite.event;
    let store = state.store.lock().unwrap();
    let mut answer = None;
    let saved = match invite.method {
        Method::Reply => {
            answer = event.attendees.first().cloned();
            match &answer {
                Some(a) => {
                    store.record_answer(&event.uid, &a.email, a.name.as_deref(), a.response)?
                }
                None => store.event_by_uid(&event.uid)?,
            }
        }
        Method::Cancel => store.cancel_invitation(&event.uid, event.sequence)?,
        _ => store.event_by_uid(&event.uid)?,
    };
    let conflicts = store
        .occurrences(event.starts, event.ends.max(event.starts + 1))?
        .into_iter()
        .filter(|o| !o.all_day && !o.cancelled && Some(o.event) != saved.as_ref().map(|s| s.id))
        .map(|o| o.title)
        .collect();
    drop(store);
    if matches!(invite.method, Method::Reply | Method::Cancel) && saved.is_some() {
        changed(&app);
    }
    Ok(Some(InviteView {
        method: method_name(invite.method),
        can_answer: invite.method == Method::Request
            && !is_me(&account, event.organizer.as_deref()),
        conflicts: if event.all_day { Vec::new() } else { conflicts },
        event,
        saved,
        answer,
    }))
}

/// Answers an invitation: puts it in the calendar with the answer and, when
/// `send` is set, mails the answer to the organizer. A published event (no
/// answer expected) is just added.
#[tauri::command]
pub async fn answer_invite(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    response: Response,
    send: bool,
) -> Result<Event> {
    let (account, raw, invite) = invitation(&app, id)
        .await?
        .ok_or_else(|| Error::Invalid("This message has no invitation.".into()))?;
    if !matches!(invite.method, Method::Request | Method::Publish) {
        return Err(Error::Invalid(
            "There is nothing to answer in this message.".into(),
        ));
    }
    let answering = invite.method == Method::Request;
    let saved = state.store.lock().unwrap().take_invitation(
        &invite.event,
        answering.then_some(response),
        account.id,
    )?;
    let organizer = invite.event.organizer.as_deref().and_then(parse_mailbox);
    if let Some(organizer) = organizer.filter(|_| answering && send) {
        let me = if account.display_name.trim().is_empty() {
            account.email.clone()
        } else {
            format!("{} <{}>", account.display_name.trim(), account.email)
        };
        let when = now();
        let calendar = mail_calendar::reply(&invite.event, &me, response, when);
        let (verb, prefix) = match response {
            Response::Accepted => ("accepted", "Accepted"),
            Response::Tentative => ("tentatively accepted", "Tentative"),
            Response::Declined => ("declined", "Declined"),
            Response::NeedsAction => {
                return Err(Error::Invalid("Choose Accept, Maybe or Decline.".into()));
            }
        };
        let who = if account.display_name.trim().is_empty() {
            account.email.as_str()
        } else {
            account.display_name.trim()
        };
        let title = if invite.event.title.trim().is_empty() {
            "(no title)"
        } else {
            invite.event.title.trim()
        };
        let subject = format!("{prefix}: {title}");
        let note = format!("{who} has {verb} this invitation.");
        let message_id = mail_mime::new_message_id(&account.email);
        let original = mail_mime::message_id_of(&raw);
        let built = mail_mime::build_invite_answer(
            Sender {
                name: &account.display_name,
                email: &account.email,
            },
            &InviteAnswer {
                to: &organizer,
                subject: &subject,
                note: &note,
                calendar: &calendar,
                message_id: &message_id,
                in_reply_to: original.as_deref(),
            },
            when,
        )?;
        state.store.lock().unwrap().queue_message(
            account.id,
            &message_id,
            &subject,
            &built.raw,
            &built.copy,
            &built.recipients,
        )?;
        report_outbox(&app, account.id, None);
        push_now(&app, account.id);
    }
    changed(&app);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVITE: &str = "From: Ann Lee <ann@work.test>\r\nTo: durim@example.test\r\nSubject: Planning\r\n\
Message-ID: <inv-1@work.test>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nYou are invited.\r\n--b\r\n\
Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\n\
BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:plan-1\r\nSEQUENCE:3\r\n\
ORGANIZER;CN=Ann Lee:mailto:ann@work.test\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:durim@example.test\r\n\
SUMMARY:Planning\r\nDTSTART:20261021T120000Z\r\nDTEND:20261021T130000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n--b--\r\n";

    /// From an incoming invitation to the answer an organizer's calendar reads.
    #[test]
    fn an_invitation_is_answered_with_a_reply_the_organizer_can_read() {
        let text = mail_mime::calendar_text(INVITE.as_bytes()).unwrap();
        let invite = mail_calendar::parse(&text).unwrap();
        assert_eq!(invite.method, Method::Request);
        let organizer = parse_mailbox(invite.event.organizer.as_deref().unwrap()).unwrap();
        assert_eq!(organizer.email, "ann@work.test");

        let calendar = mail_calendar::reply(
            &invite.event,
            "Durim K <durim@example.test>",
            Response::Accepted,
            1,
        );
        let built = mail_mime::build_invite_answer(
            Sender {
                name: "Durim K",
                email: "durim@example.test",
            },
            &InviteAnswer {
                to: &organizer,
                subject: "Accepted: Planning",
                note: "Durim K has accepted this invitation.",
                calendar: &calendar,
                message_id: "a@example.test",
                in_reply_to: Some("inv-1@work.test"),
            },
            1,
        )
        .unwrap();

        let back = mail_calendar::parse(&mail_mime::calendar_text(&built.raw).unwrap()).unwrap();
        assert_eq!(back.method, Method::Reply);
        assert_eq!(back.event.uid, "plan-1");
        assert_eq!(back.event.sequence, 3);
        assert_eq!(back.event.attendees.len(), 1);
        assert_eq!(back.event.attendees[0].email, "durim@example.test");
        assert_eq!(back.event.attendees[0].response, Response::Accepted);
        assert_eq!(built.recipients, ["ann@work.test"]);
    }
}
