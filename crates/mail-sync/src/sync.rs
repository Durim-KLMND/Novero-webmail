//! Bringing the local cache in step with the server.
//!
//! Phase 1 sync fetches folder lists and the headers of messages the cache
//! does not have yet: new ones first, then older ones until the folder is
//! complete. Flag changes and server-side deletions are phase 2.

use std::sync::Mutex;

use mail_core::{Account, Event, FolderRole, Result};
use mail_store::Store;

use crate::ImapSession;

/// Headers fetched per round trip.
const BATCH: usize = 500;

/// Syncs every folder of `account`, inbox first, reporting progress through `on_event`.
pub async fn sync_account(
    store: &Mutex<Store>,
    account: &Account,
    password: &str,
    on_event: impl Fn(Event),
) -> Result<()> {
    let mut session = ImapSession::connect(&account.imap, &account.username, password).await?;
    let remote = session.list_folders().await?;
    let mut folders = store.lock().unwrap().sync_folders(account.id, &remote)?;
    folders.retain(|(_, f)| f.selectable);
    folders.sort_by_key(|(_, f)| (f.role != FolderRole::Inbox, f.role.sort_rank()));

    for (folder_id, folder) in folders {
        let status = session.select(&folder.name).await?;
        let mut state = store.lock().unwrap().folder_state(folder_id)?;
        if state.uid_validity.is_some() && state.uid_validity != status.uid_validity {
            store.lock().unwrap().reset_folder(account.id, folder_id)?;
            state = store.lock().unwrap().folder_state(folder_id)?;
        }

        // New messages above what we have.
        let mut wanted = Vec::new();
        if state.max_uid > 0 {
            wanted = session
                .search_uids(&format!("UID {}:*", state.max_uid + 1))
                .await?
                .into_iter()
                .filter(|&uid| uid > state.max_uid)
                .collect();
        }
        // Older messages missing from the cache: everything on a first sync,
        // or the rest of an interrupted one.
        if (state.count as usize + wanted.len()) < status.exists as usize {
            let query = match state.min_uid {
                0 => "ALL".to_owned(),
                1 => String::new(),
                min => format!("UID 1:{}", min - 1),
            };
            if !query.is_empty() {
                let older = session.search_uids(&query).await?;
                wanted.extend(
                    older
                        .into_iter()
                        .filter(|&u| u < state.min_uid || state.min_uid == 0),
                );
            }
        }
        wanted.sort_unstable_by(|a, b| b.cmp(a));
        wanted.dedup();

        let total = wanted.len() as u32;
        let mut done = 0;
        on_event(Event::SyncProgress {
            account: account.id,
            folder: folder.name.clone(),
            done,
            total,
        });
        for chunk in wanted.chunks(BATCH) {
            let set = chunk
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let headers = session.fetch_headers(&set).await?;
            store.lock().unwrap().insert_headers(folder_id, &headers)?;
            done += chunk.len() as u32;
            on_event(Event::SyncProgress {
                account: account.id,
                folder: folder.name.clone(),
                done,
                total,
            });
        }
        store
            .lock()
            .unwrap()
            .set_folder_synced(folder_id, status.uid_validity, status.uid_next)?;
    }

    session.logout().await
}

/// Fetches a message body the cache does not have yet and saves it.
pub async fn fetch_message(
    store: &Mutex<Store>,
    account: &Account,
    password: &str,
    message: i64,
) -> Result<Vec<u8>> {
    let source = store.lock().unwrap().message_source(message)?;
    if let Some(path) = &source.raw_path
        && let Ok(raw) = std::fs::read(path)
    {
        return Ok(raw);
    }
    let mut session = ImapSession::connect(&account.imap, &account.username, password).await?;
    let raw = session.fetch_raw(&source.folder_name, source.uid).await?;
    store.lock().unwrap().save_raw(message, &raw)?;
    let _ = session.logout().await;
    Ok(raw)
}
