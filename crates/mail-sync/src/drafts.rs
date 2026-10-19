//! Keeping the Drafts folder in step with drafts saved locally.
//!
//! A draft keeps its Message-ID across saves, so its server copy is found by
//! that header: each upload appends the new version, then deletes the old ones.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mail_core::{Account, FolderRole, Result};
use mail_mime::{Sender, build_message};
use mail_store::Store;

use crate::ImapSession;
use crate::imap::uid_set;

/// Writes or removes the server copy of every draft that changed. Returns the
/// Drafts folder's name when it changed, so it can be synced.
pub async fn upload_drafts(
    store: &Mutex<Store>,
    session: &mut ImapSession,
    account: &Account,
) -> Result<Option<String>> {
    let uploads = store.lock().unwrap().drafts_to_upload(account.id)?;
    if uploads.is_empty() {
        return Ok(None);
    }
    let Some(folder) = store
        .lock()
        .unwrap()
        .folder_with_role(account.id, FolderRole::Drafts)?
    else {
        // No Drafts folder: drafts stay on this device only.
        let store = store.lock().unwrap();
        for upload in uploads {
            if upload.discarded {
                store.forget_draft(upload.draft.id)?;
            } else {
                store.draft_uploaded(upload.draft.id, upload.version)?;
            }
        }
        return Ok(None);
    };

    session.select(&folder).await?;
    for upload in uploads {
        let draft = &upload.draft;
        // Searched even for a first upload: an earlier one may have reached
        // the server just before the connection dropped.
        let old = session
            .search_uids(&format!(
                "HEADER Message-ID {}",
                quoted(&format!("<{}>", draft.message_id))
            ))
            .await?;
        if !upload.discarded {
            let files = store.lock().unwrap().draft_files(draft.id)?;
            let sender = Sender {
                name: &account.display_name,
                email: &account.email,
            };
            let built = build_message(sender, draft, &files, now(), false)?;
            session
                .append(&folder, Some("(\\Draft \\Seen)"), &built.copy)
                .await?;
        }
        if !old.is_empty() {
            session.expunge_uids(&uid_set(&old)).await?;
        }
        let store = store.lock().unwrap();
        if upload.discarded {
            store.forget_draft(draft.id)?;
        } else {
            store.draft_uploaded(draft.id, upload.version)?;
        }
    }
    Ok(Some(folder))
}

/// An IMAP quoted string.
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn quoted_strings_escape_quotes_and_backslashes() {
        assert_eq!(super::quoted(r#"<a"b\c>"#), r#""<a\"b\\c>""#);
    }
}
