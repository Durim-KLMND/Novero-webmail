//! Folder commands: making your own folders, renaming and deleting them,
//! marking a folder read and emptying Trash or Junk.

use mail_core::{AccountId, Error, Event, FlagKind, FolderId, FolderRole, Op, Result};
use mail_store::Folder;
use mail_sync::names;
use tauri::{AppHandle, State};

use crate::commands::change;
use crate::state::{AppState, emit};

/// Makes a folder named `name`, inside `parent` or at the top. Returns it.
#[tauri::command]
pub async fn create_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    account: AccountId,
    parent: Option<FolderId>,
    name: String,
) -> Result<Folder> {
    let folders = state.store.lock().unwrap().list_folders(account)?;
    let parent = match parent {
        Some(id) => Some(
            folders
                .iter()
                .find(|f| f.id == id)
                .ok_or_else(|| Error::Invalid("that folder no longer exists".into()))?,
        ),
        None => None,
    };
    let full = full_name(&folders, parent, &name)?;
    let account = state.account(account)?;
    let secret = state.secret(&account).await?;
    mail_sync::create_folder(&state.store, &account, &secret, &full).await?;
    emit(
        &app,
        Event::MailboxChanged {
            account: account.id,
        },
    );
    let store = state.store.lock().unwrap();
    let id = store
        .folder_id(account.id, &full)?
        .ok_or_else(|| Error::Protocol("the server made the folder but does not list it".into()))?;
    store
        .folder(id)?
        .ok_or_else(|| Error::Storage("the new folder could not be read".into()))
}

/// Gives one of your own folders a new name, keeping it where it is.
#[tauri::command]
pub async fn rename_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: FolderId,
    name: String,
) -> Result<()> {
    let (current, folders) = own_folder(&state, folder)?;
    let parent = parent_of(&folders, &current).cloned();
    let others: Vec<Folder> = folders.into_iter().filter(|f| f.id != current.id).collect();
    let full = full_name(&others, parent.as_ref(), &name)?;
    if full == current.name {
        return Ok(());
    }
    let account = state.account(current.account_id)?;
    let secret = state.secret(&account).await?;
    mail_sync::rename_folder(&state.store, &account, &secret, &current.name, &full).await?;
    emit(
        &app,
        Event::MailboxChanged {
            account: account.id,
        },
    );
    Ok(())
}

/// Deletes one of your own folders with its subfolders and messages.
#[tauri::command]
pub async fn delete_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: FolderId,
) -> Result<()> {
    let (current, _) = own_folder(&state, folder)?;
    let account = state.account(current.account_id)?;
    let secret = state.secret(&account).await?;
    mail_sync::delete_folder(&state.store, &account, &secret, &current.name).await?;
    emit(
        &app,
        Event::MailboxChanged {
            account: account.id,
        },
    );
    Ok(())
}

/// Marks every message in a folder read.
#[tauri::command]
pub fn mark_folder_read(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: FolderId,
) -> Result<()> {
    let ids = state.store.lock().unwrap().folder_messages(folder, true)?;
    change(&app, &state, &ids, |m| Op::SetFlag {
        folder: m.folder_name.clone(),
        uid_validity: m.uid_validity,
        uids: Vec::new(),
        flag: FlagKind::Seen,
        value: true,
    })
}

/// Deletes everything in Trash or Junk for good.
#[tauri::command]
pub fn empty_folder(app: AppHandle, state: State<'_, AppState>, folder: FolderId) -> Result<()> {
    let role = state.store.lock().unwrap().folder_role(folder)?;
    if !matches!(role, Some(FolderRole::Trash | FolderRole::Junk)) {
        return Err(Error::Invalid("only Trash and Junk can be emptied".into()));
    }
    let ids = state.store.lock().unwrap().folder_messages(folder, false)?;
    change(&app, &state, &ids, |m| Op::Expunge {
        folder: m.folder_name.clone(),
        uid_validity: m.uid_validity,
        uids: Vec::new(),
    })
}

/// A folder you made, with the rest of its account's folders. The server's
/// own folders stay as they are, and so does a folder with changes still
/// on their way to the server, which name it.
fn own_folder(state: &AppState, id: FolderId) -> Result<(Folder, Vec<Folder>)> {
    let store = state.store.lock().unwrap();
    let folder = store
        .folder(id)?
        .ok_or_else(|| Error::Invalid("that folder no longer exists".into()))?;
    if folder.role != FolderRole::Other {
        return Err(Error::Invalid(
            "this folder belongs to the mail server and can't be changed".into(),
        ));
    }
    if store.folder_has_pending(folder.account_id, &folder.name, folder.delimiter.as_deref())? {
        return Err(Error::Invalid(
            "changes to messages in this folder are still on their way to the server. Try again in a moment.".into(),
        ));
    }
    let folders = store.list_folders(folder.account_id)?;
    Ok((folder, folders))
}

/// The folder that holds `folder`, if it is in one.
fn parent_of<'a>(folders: &'a [Folder], folder: &Folder) -> Option<&'a Folder> {
    let delimiter = folder.delimiter.as_deref().filter(|d| !d.is_empty())?;
    let (parent, _) = folder.name.rsplit_once(delimiter)?;
    folders.iter().find(|f| f.name == parent)
}

/// The server name for a new folder typed as `name`, inside `parent` or at
/// the top. Refused when the account already has a folder of that name.
fn full_name(folders: &[Folder], parent: Option<&Folder>, name: &str) -> Result<String> {
    let full = server_name(folders, parent, name)?;
    if full.eq_ignore_ascii_case("INBOX")
        || folders
            .iter()
            .any(|f| f.name.to_lowercase() == full.to_lowercase())
    {
        return Err(Error::Invalid(format!(
            "there is already a folder called “{}”",
            name.trim()
        )));
    }
    Ok(full)
}

/// The server name for a folder typed as `name`, inside `parent` or at the
/// top, whether or not it exists yet.
pub(crate) fn server_name(
    folders: &[Folder],
    parent: Option<&Folder>,
    name: &str,
) -> Result<String> {
    let name = name.trim();
    let delimiter = parent
        .and_then(|p| p.delimiter.clone())
        .or_else(|| folders.iter().find_map(|f| f.delimiter.clone()))
        .filter(|d| !d.is_empty());
    if name.is_empty() {
        return Err(Error::Invalid("type a name for the folder".into()));
    }
    if name.chars().count() > 100 {
        return Err(Error::Invalid("that name is too long".into()));
    }
    if let Some(d) = delimiter.as_deref().filter(|d| name.contains(d)) {
        return Err(Error::Invalid(format!("a folder name can't contain “{d}”")));
    }
    if name.chars().any(|c| c.is_control() || c == '*' || c == '%') {
        return Err(Error::Invalid("a folder name can't contain * or %".into()));
    }
    let encoded = names::encode(name);
    let full = match (parent, delimiter.as_deref()) {
        (Some(p), Some(d)) => format!("{}{d}{encoded}", p.name),
        (Some(_), None) => {
            return Err(Error::Invalid(
                "this server doesn't allow folders inside folders".into(),
            ));
        }
        // Some servers keep every folder inside the inbox ("INBOX.Work").
        (None, Some(d)) if inside_inbox(folders, d) => format!("INBOX{d}{encoded}"),
        (None, _) => encoded,
    };
    Ok(full)
}

/// Whether every folder but the inbox is inside it, as on servers whose
/// own folders all start with "INBOX.".
fn inside_inbox(folders: &[Folder], delimiter: &str) -> bool {
    let prefix = format!("inbox{}", delimiter.to_lowercase());
    let mut others = folders
        .iter()
        .filter(|f| f.role != FolderRole::Inbox)
        .peekable();
    others.peek().is_some() && others.all(|f| f.name.to_lowercase().starts_with(&prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: i64, name: &str, role: FolderRole, delimiter: &str) -> Folder {
        Folder {
            id: FolderId(id),
            account_id: AccountId(1),
            name: name.into(),
            delimiter: Some(delimiter.into()),
            role,
            total: 0,
            unread: 0,
        }
    }

    #[test]
    fn new_folders_go_at_the_top_or_inside_their_parent() {
        let folders = [
            folder(1, "INBOX", FolderRole::Inbox, "/"),
            folder(2, "Sent", FolderRole::Sent, "/"),
            folder(3, "Work", FolderRole::Other, "/"),
        ];
        assert_eq!(full_name(&folders, None, " Receipts ").unwrap(), "Receipts");
        assert_eq!(
            full_name(&folders, Some(&folders[2]), "2026").unwrap(),
            "Work/2026"
        );
        assert_eq!(
            full_name(&folders, None, "Entwürfe").unwrap(),
            "Entw&APw-rfe"
        );
        assert!(full_name(&folders, None, "work").is_err());
        assert!(full_name(&folders, None, "inbox").is_err());
        assert!(full_name(&folders, None, "a/b").is_err());
        assert!(full_name(&folders, None, "  ").is_err());
    }

    #[test]
    fn servers_that_keep_folders_in_the_inbox_get_them_there() {
        let folders = [
            folder(1, "INBOX", FolderRole::Inbox, "."),
            folder(2, "INBOX.Sent", FolderRole::Sent, "."),
            folder(3, "INBOX.Trash", FolderRole::Trash, "."),
        ];
        assert_eq!(full_name(&folders, None, "Work").unwrap(), "INBOX.Work");
        assert!(full_name(&folders, None, "v1.2").is_err());
    }

    #[test]
    fn renamed_folders_stay_in_their_parent() {
        let folders = [
            folder(1, "INBOX", FolderRole::Inbox, "/"),
            folder(2, "Work", FolderRole::Other, "/"),
            folder(3, "Work/2026", FolderRole::Other, "/"),
        ];
        let parent = parent_of(&folders, &folders[2]);
        assert_eq!(parent.map(|p| p.id), Some(FolderId(2)));
        assert_eq!(full_name(&folders, parent, "2027").unwrap(), "Work/2027");
        assert!(parent_of(&folders, &folders[1]).is_none());
    }
}
