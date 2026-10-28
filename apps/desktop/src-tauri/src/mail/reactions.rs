//! Emoji reactions to the user's mail. After a sync, small replies in
//! conversations with the user's own mail are downloaded and looked at; a
//! reaction is marked read, left out of lists and shown on the message it
//! reacts to.

use mail_core::{AccountId, Error, FlagKind, Op};
use mail_sync::NewMail;
use tauri::{AppHandle, Manager};

use crate::commands::change;
use crate::mail::reader;
use crate::state::AppState;

/// How far back the user's own mail is looked at for reactions.
const LOOK_BACK: i64 = 90 * 24 * 60 * 60;
/// Messages looked at per sync; the rest wait for the next one.
const PER_SYNC: u32 = 25;

/// Finds reactions among recent mail. Returns `new_mail` without them, and
/// the new reactions as notices for the user ("👍 reacted to your message").
pub async fn find(
    app: &AppHandle,
    id: AccountId,
    new_mail: &[NewMail],
) -> (Vec<NewMail>, Vec<NewMail>) {
    let found = look(app, id).await;
    let (reactions, left): (Vec<NewMail>, Vec<NewMail>) = new_mail
        .iter()
        .cloned()
        .partition(|m| found.iter().any(|(r, _)| *r == m.id));
    let notices = reactions
        .into_iter()
        .filter_map(|m| {
            let (_, emoji) = found.iter().find(|(r, _)| *r == m.id)?;
            Some(NewMail {
                subject: Some(format!("{emoji} reacted to your message")),
                ..m
            })
        })
        .collect();
    (left, notices)
}

/// Looks at the messages that may be reactions; returns the reactions found
/// with their emoji. A message that can't be downloaded now waits for the
/// next sync.
async fn look(app: &AppHandle, id: AccountId) -> Vec<(i64, String)> {
    let state = app.state::<AppState>();
    let Ok(account) = state.account(id) else {
        return Vec::new();
    };
    let since = crate::mail::compose::now() - LOOK_BACK;
    let candidates = state
        .store
        .lock()
        .unwrap()
        .reaction_candidates(id, since, mail_mime::MAX_REACTION_SIZE as u32, PER_SYNC)
        .unwrap_or_default();
    let mut found = Vec::new();
    for message in candidates {
        let raw = match reader::raw_only(app, &account, message).await {
            Ok(raw) => raw,
            // Gone from the server, say: nothing to look at.
            Err(Error::Rejected(_) | Error::Protocol(_)) => Vec::new(),
            Err(_) => break,
        };
        let store = state.store.lock().unwrap();
        let reaction =
            mail_mime::reaction(&raw).filter(|r| store.has_message_id(id, &r.to).unwrap_or(false));
        let saved = store.set_reaction(
            message,
            reaction.as_ref().map(|r| (r.emoji.as_str(), r.to.as_str())),
        );
        if saved.is_ok()
            && let Some(r) = reaction
        {
            found.push((message, r.emoji));
        }
    }
    if !found.is_empty() {
        // Reactions are read where they are shown; their mail needn't wait unread.
        let ids: Vec<i64> = found.iter().map(|(m, _)| *m).collect();
        let _ = change(app, &state, &ids, |m| Op::SetFlag {
            folder: m.folder_name.clone(),
            uid_validity: m.uid_validity,
            uids: Vec::new(),
            flag: FlagKind::Seen,
            value: true,
        });
    }
    found
}
