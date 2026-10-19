//! Commands for writing and sending mail: drafts, attachments and the outbox.
//!
//! Drafts save locally as the user types; the account's push task copies them
//! to the Drafts folder and sends queued mail, so all of it works offline.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use mail_core::{AccountId, Draft, DraftAttachment, DraftFields, Error, FlagKind, Op, Result};
use mail_mime::{Prefill, ReplyMode, Sender};
use mail_store::{Contact, NewDraft, OutboxItem, OutboxState};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{change, raw_message};
use crate::state::{AppState, push_now, report_outbox};

/// Attachments of one message may add up to this much. Most providers refuse
/// larger messages, and base64 makes them a third bigger on the wire.
const MAX_ATTACHMENTS: u64 = 25 * 1024 * 1024;

/// An empty draft from `account`, with its signature.
#[tauri::command]
pub fn new_draft(state: State<'_, AppState>, account: AccountId) -> Result<Draft> {
    let account = state.account(account)?;
    let mut draft = NewDraft::new(account.id, mail_mime::new_message_id(&account.email));
    draft.fields.body = with_signature("", &account.signature);
    state.store.lock().unwrap().create_draft(draft)
}

/// A draft from `account` filled in from a `mailto:` link, as clicked on a
/// web page or in another app.
#[tauri::command]
pub fn mailto_draft(state: State<'_, AppState>, account: AccountId, link: String) -> Result<Draft> {
    let account = state.account(account)?;
    let mut fields = mail_mime::parse_mailto(&link)
        .ok_or_else(|| Error::Invalid("that isn't an email link".into()))?;
    fields
        .body
        .push_str(&with_signature("", &account.signature));
    let mut draft = NewDraft::new(account.id, mail_mime::new_message_id(&account.email));
    draft.fields = fields;
    state.store.lock().unwrap().create_draft(draft)
}

/// Puts the signature above `body` (a quote or forwarded text, which starts
/// with blank lines for the reply), after the usual "-- " separator.
pub fn with_signature(body: &str, signature: &str) -> String {
    if signature.trim().is_empty() {
        return body.to_owned();
    }
    format!("\n\n-- \n{signature}{body}")
}

/// A reply to a cached message, from the account it arrived in.
#[tauri::command]
pub async fn reply_draft(state: State<'_, AppState>, message: i64, all: bool) -> Result<Draft> {
    let (account, raw) = raw_message(&state, message).await?;
    let own: Vec<String> = state
        .store
        .lock()
        .unwrap()
        .list_accounts()?
        .into_iter()
        .map(|a| a.email)
        .collect();
    let own: Vec<&str> = own.iter().map(String::as_str).collect();
    let mode = if all {
        ReplyMode::All
    } else {
        ReplyMode::Sender
    };
    let mut prefill = mail_mime::reply(&raw, mode, &own)?;
    prefill.fields.body = with_signature(&prefill.fields.body, &account.signature);
    create(&state, account.id, &account.email, prefill, Some(message))
}

/// A forward of a cached message, attachments included.
#[tauri::command]
pub async fn forward_draft(state: State<'_, AppState>, message: i64) -> Result<Draft> {
    let (account, raw) = raw_message(&state, message).await?;
    let mut prefill = mail_mime::forward(&raw)?;
    prefill.fields.body = with_signature(&prefill.fields.body, &account.signature);
    create(&state, account.id, &account.email, prefill, None)
}

/// Opens a message from the Drafts folder for editing. If this device has a
/// newer version of it, that one opens instead.
#[tauri::command]
pub async fn open_draft(state: State<'_, AppState>, message: i64) -> Result<Draft> {
    let (account, raw) = raw_message(&state, message).await?;
    let message_id = mail_mime::message_id_of(&raw);
    if let Some(id) = &message_id
        && let Some(draft) = state
            .store
            .lock()
            .unwrap()
            .draft_by_message_id(account.id, id)?
    {
        return Ok(draft);
    }
    let prefill = mail_mime::reopen(&raw)?;
    let message_id = message_id.unwrap_or_else(|| mail_mime::new_message_id(&account.email));
    state.store.lock().unwrap().create_draft(NewDraft {
        fields: prefill.fields,
        in_reply_to: prefill.in_reply_to,
        references: prefill.references,
        attachments: prefill.attachments,
        // With a Message-ID the server copy is found and replaced on the next save.
        on_server: true,
        ..NewDraft::new(account.id, message_id)
    })
}

fn create(
    state: &AppState,
    account: AccountId,
    email: &str,
    prefill: Prefill,
    replying_to: Option<i64>,
) -> Result<Draft> {
    state.store.lock().unwrap().create_draft(NewDraft {
        fields: prefill.fields,
        in_reply_to: prefill.in_reply_to,
        references: prefill.references,
        replying_to,
        attachments: prefill.attachments,
        ..NewDraft::new(account, mail_mime::new_message_id(email))
    })
}

/// Saves what the user typed. With `upload`, the Drafts folder copy is
/// updated too (when the composer closes, and now and then while typing).
#[tauri::command]
pub fn save_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    fields: DraftFields,
    upload: bool,
) -> Result<Draft> {
    let draft = state.store.lock().unwrap().save_draft_fields(id, &fields)?;
    if upload {
        push_now(&app, draft.account);
    }
    Ok(draft)
}

/// Asks for files with the system file picker and attaches them. The UI never
/// sees file paths, so it cannot attach a file the user did not pick.
#[tauri::command]
pub async fn attach_files(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<Draft> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Attach files")
        .pick_files(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.ok().flatten().unwrap_or_default();
    let paths: Vec<PathBuf> = picked
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect();
    attach_paths(&state, id, paths)
}

/// Attaches files the person picked or dropped.
pub fn attach_paths(state: &AppState, id: i64, paths: Vec<PathBuf>) -> Result<Draft> {
    let mut draft = state
        .store
        .lock()
        .unwrap()
        .draft(id)?
        .ok_or_else(|| Error::Storage("this draft was sent or deleted".into()))?;
    let mut total: u64 = draft.attachments.iter().map(|a| a.size).sum();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".into());
        let contents = std::fs::read(&path)
            .map_err(|e| Error::Invalid(format!("{name} could not be read: {e}")))?;
        total += contents.len() as u64;
        if total > MAX_ATTACHMENTS {
            return Err(Error::Invalid(format!(
                "{name} was not attached: attachments can add up to {} MB",
                MAX_ATTACHMENTS / (1024 * 1024)
            )));
        }
        let attachment = DraftAttachment {
            mime_type: mime_guess::from_path(&path)
                .first_or_octet_stream()
                .essence_str()
                .to_owned(),
            size: contents.len() as u64,
            name,
        };
        draft = state
            .store
            .lock()
            .unwrap()
            .add_draft_attachment(id, attachment, &contents)?;
    }
    Ok(draft)
}

#[tauri::command]
pub fn remove_attachment(state: State<'_, AppState>, id: i64, index: usize) -> Result<Draft> {
    state
        .store
        .lock()
        .unwrap()
        .remove_draft_attachment(id, index)
}

/// Throws a draft away, here and in the Drafts folder.
#[tauri::command]
pub fn discard_draft(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<()> {
    let account = state.store.lock().unwrap().draft(id)?.map(|d| d.account);
    state.store.lock().unwrap().discard_draft(id)?;
    if let Some(account) = account {
        push_now(&app, account);
    }
    Ok(())
}

/// Saves the draft one last time, checks it, and queues it to send. The
/// composer can close right away; sending happens in the background.
#[tauri::command]
pub fn send_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    fields: DraftFields,
) -> Result<()> {
    let draft = state.store.lock().unwrap().save_draft_fields(id, &fields)?;
    let account = state.account(draft.account)?;
    let files = state.store.lock().unwrap().draft_files(id)?;
    let sender = Sender {
        name: &account.display_name,
        email: &account.email,
    };
    let built = mail_mime::build_message(sender, &draft, &files, now(), true)?;
    state.store.lock().unwrap().queue_outgoing(
        &draft,
        &built.raw,
        &built.copy,
        &built.recipients,
    )?;
    let mut recipients = mail_core::address::parse_lenient(&draft.fields.to);
    recipients.extend(mail_core::address::parse_lenient(&draft.fields.cc));
    recipients.extend(mail_core::address::parse_lenient(&draft.fields.bcc));
    // Only for suggestions; sending doesn't depend on it.
    let _ = state.store.lock().unwrap().note_sent(&recipients, now());
    if let Some(original) = draft.replying_to {
        // The original may have been moved or deleted meanwhile; then there
        // is nothing to mark.
        let _ = change(&app, &state, &[original], |m| Op::SetFlag {
            folder: m.folder_name.clone(),
            uid_validity: m.uid_validity,
            uids: Vec::new(),
            flag: FlagKind::Answered,
            value: true,
        });
    }
    report_outbox(&app, account.id, None);
    push_now(&app, account.id);
    Ok(())
}

/// Known addresses matching what is being typed, for the To, Cc and Bcc fields.
#[tauri::command]
pub fn suggest_contacts(state: State<'_, AppState>, query: String) -> Result<Vec<Contact>> {
    state.store.lock().unwrap().suggest_contacts(&query, 8)
}

/// Stops suggesting an address.
#[tauri::command]
pub fn hide_contact(state: State<'_, AppState>, email: String) -> Result<()> {
    state.store.lock().unwrap().hide_contact(&email)
}

/// Messages waiting to send or refused by the server.
#[tauri::command]
pub fn list_outbox(state: State<'_, AppState>, account: AccountId) -> Result<Vec<OutboxItem>> {
    Ok(state
        .store
        .lock()
        .unwrap()
        .outbox(account)?
        .into_iter()
        .filter(|i| i.state != OutboxState::Sent)
        .collect())
}

/// Counts for the sidebar: waiting to send, and refused.
#[tauri::command]
pub fn outbox_status(state: State<'_, AppState>, account: AccountId) -> Result<(u32, u32)> {
    state.store.lock().unwrap().outbox_counts(account)
}

/// Tries a refused message again.
#[tauri::command]
pub fn retry_outbox(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<()> {
    let item = outbox_item(&state, id)?;
    state.store.lock().unwrap().outbox_retry(id)?;
    report_outbox(&app, item.account_id, None);
    push_now(&app, item.account_id);
    Ok(())
}

/// Removes a message from the outbox without sending it.
#[tauri::command]
pub fn delete_outbox(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<()> {
    let item = unsent_item(&state, id)?;
    state.store.lock().unwrap().outbox_remove(id)?;
    report_outbox(&app, item.account_id, None);
    Ok(())
}

/// Takes a message out of the outbox and reopens it as a draft.
#[tauri::command]
pub fn edit_outbox(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<Draft> {
    let item = unsent_item(&state, id)?;
    let copy = state.store.lock().unwrap().outbox_file(&item, true)?;
    let prefill = mail_mime::reopen(&copy)?;
    let draft = state.store.lock().unwrap().create_draft(NewDraft {
        fields: prefill.fields,
        in_reply_to: prefill.in_reply_to,
        references: prefill.references,
        attachments: prefill.attachments,
        ..NewDraft::new(item.account_id, item.message_id.clone())
    })?;
    state.store.lock().unwrap().outbox_remove(id)?;
    report_outbox(&app, item.account_id, None);
    Ok(draft)
}

fn outbox_item(state: &AppState, id: i64) -> Result<OutboxItem> {
    state
        .store
        .lock()
        .unwrap()
        .outbox_item(id)?
        .ok_or_else(|| Error::Storage("that message is no longer in the outbox".into()))
}

/// A message that has not gone out yet. One the server has taken can no
/// longer be edited or held back.
fn unsent_item(state: &AppState, id: i64) -> Result<OutboxItem> {
    let item = outbox_item(state, id)?;
    if item.state == OutboxState::Sent {
        return Err(Error::Invalid("this message was already sent".into()));
    }
    Ok(item)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::with_signature;

    #[test]
    fn signatures_go_above_the_quote() {
        assert_eq!(with_signature("", "Ann"), "\n\n-- \nAnn");
        assert_eq!(
            with_signature("\n\nOn Monday, Bob wrote:\n> Hi", "Ann\nNovero"),
            "\n\n-- \nAnn\nNovero\n\nOn Monday, Bob wrote:\n> Hi"
        );
        assert_eq!(with_signature("\n\n> Hi", "  "), "\n\n> Hi");
    }
}
