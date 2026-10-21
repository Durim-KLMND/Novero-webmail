//! Making, renaming and deleting folders on the server. These need the
//! server at once rather than queueing like message changes: the name the
//! server accepts is only known once it has answered.

use std::sync::Mutex;

use mail_core::{Account, Error, FolderRole, RemoteFolder, Result, Secret};
use mail_store::Store;

use crate::ImapSession;

/// Makes the folder `name` (a full server name) and adds it to the cache.
pub async fn create_folder(
    store: &Mutex<Store>,
    account: &Account,
    secret: &Secret,
    name: &str,
) -> Result<()> {
    let mut session = ImapSession::connect(&account.imap, &account.username, secret).await?;
    session.create_folder(name).await?;
    // Other apps may list only subscribed folders.
    let _ = session.subscribe(name).await;
    refresh(store, account, session).await
}

/// Renames the folder `from` to `to`, both full server names. Its messages
/// and subfolders keep their place in the cache.
pub async fn rename_folder(
    store: &Mutex<Store>,
    account: &Account,
    secret: &Secret,
    from: &str,
    to: &str,
) -> Result<()> {
    let mut session = ImapSession::connect(&account.imap, &account.username, secret).await?;
    let remote = session.list_folders().await?;
    protect(&remote, from)?;
    session.rename_folder(from, to).await?;
    let _ = session.unsubscribe(from).await;
    let _ = session.subscribe(to).await;
    store.lock().unwrap().rename_folder(account.id, from, to)?;
    refresh(store, account, session).await
}

/// Deletes the folder `name` and every folder inside it, with their messages.
pub async fn delete_folder(
    store: &Mutex<Store>,
    account: &Account,
    secret: &Secret,
    name: &str,
) -> Result<()> {
    let mut session = ImapSession::connect(&account.imap, &account.username, secret).await?;
    let remote = session.list_folders().await?;
    protect(&remote, name)?;
    // Innermost first: some servers keep a parent that still has children.
    let mut doomed: Vec<&RemoteFolder> = remote.iter().filter(|f| within(f, name)).collect();
    doomed.sort_by_key(|f| std::cmp::Reverse(f.name.len()));
    for folder in doomed {
        session.delete_folder(&folder.name).await?;
        let _ = session.unsubscribe(&folder.name).await;
    }
    refresh(store, account, session).await
}

/// Whether `folder` is `name` or inside it.
fn within(folder: &RemoteFolder, name: &str) -> bool {
    folder.name == name
        || folder.delimiter.as_deref().is_some_and(|d| {
            !d.is_empty()
                && folder
                    .name
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with(d))
        })
}

/// Refuses to change the server's own folders, or a folder that holds one,
/// such as Gmail's "[Gmail]".
fn protect(remote: &[RemoteFolder], name: &str) -> Result<()> {
    let folder = remote
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| Error::Rejected("that folder is no longer on the server".into()))?;
    if folder.role != FolderRole::Other || !folder.selectable {
        return Err(Error::Invalid(
            "this folder belongs to the mail server and can't be changed".into(),
        ));
    }
    if remote
        .iter()
        .any(|f| f.role != FolderRole::Other && within(f, name))
    {
        return Err(Error::Invalid(
            "this folder holds one of the server's own folders, so it can't be changed".into(),
        ));
    }
    Ok(())
}

/// Brings the cached folder list in step with the server and logs out.
async fn refresh(store: &Mutex<Store>, account: &Account, mut session: ImapSession) -> Result<()> {
    let remote = session.list_folders().await?;
    store.lock().unwrap().sync_folders(account.id, &remote)?;
    let _ = session.logout().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, role: FolderRole) -> RemoteFolder {
        RemoteFolder {
            name: name.into(),
            delimiter: Some("/".into()),
            role,
            selectable: true,
        }
    }

    #[test]
    fn subfolders_are_within_their_parent_only() {
        assert!(within(&folder("Work", FolderRole::Other), "Work"));
        assert!(within(&folder("Work/2026", FolderRole::Other), "Work"));
        assert!(!within(&folder("Workshop", FolderRole::Other), "Work"));
    }

    #[test]
    fn the_servers_own_folders_are_left_alone() {
        let mut gmail = folder("[Gmail]", FolderRole::Other);
        gmail.selectable = false;
        let remote = [
            folder("INBOX", FolderRole::Inbox),
            gmail,
            folder("[Gmail]/Sent Mail", FolderRole::Sent),
            folder("Work", FolderRole::Other),
            folder("Work/2026", FolderRole::Other),
            folder("Holder", FolderRole::Other),
            folder("Holder/Trash", FolderRole::Trash),
        ];
        assert!(protect(&remote, "Work").is_ok());
        assert!(protect(&remote, "Work/2026").is_ok());
        assert!(protect(&remote, "INBOX").is_err());
        assert!(protect(&remote, "[Gmail]").is_err());
        assert!(protect(&remote, "Holder").is_err());
        assert!(matches!(protect(&remote, "Gone"), Err(Error::Rejected(_))));
    }
}
