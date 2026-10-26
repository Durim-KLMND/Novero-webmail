//! Category folders: a Categories folder on each account's server with one
//! folder inside for Updates, Social, Forums, Newsletters and Promotions,
//! so mail sorted by the cleanup view is filed where webmail and phones see
//! it too. Personal mail stays in the inbox, and Spam goes to Junk as before.
//!
//! Inbox mail is filed when the user confirmed its category, or when the
//! guess is a sure one: once when the folders are made or when asked, and
//! as new mail arrives.

use std::collections::{BTreeMap, HashMap};

use mail_core::categories::Category;
use mail_core::{Account, AccountId, Error, Event, FolderRole, Result, Secret};
use mail_store::{Folder, Store};
use mail_sync::NewMail;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::move_to;
use crate::mail::folders::server_name;
use crate::state::{AppState, emit, request_sync};

const KEY: &str = "categoryFolders";

/// The folder that holds the category folders.
const PARENT: &str = "Categories";

/// The categories that get a folder of their own.
pub const FILED: [Category; 5] = [
    Category::Updates,
    Category::Social,
    Category::Forums,
    Category::Newsletters,
    Category::Promotions,
];

fn label(category: Category) -> &'static str {
    match category {
        Category::Personal => "Personal",
        Category::Updates => "Updates",
        Category::Social => "Social",
        Category::Forums => "Forums",
        Category::Newsletters => "Newsletters",
        Category::Promotions => "Promotions",
        Category::Spam => "Spam",
    }
}

/// Each account's category folders, by server name.
type Saved = HashMap<String, BTreeMap<Category, String>>;

fn load(store: &Store) -> Saved {
    store
        .setting(KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn save(store: &Store, saved: &Saved) -> Result<()> {
    let value = serde_json::to_value(saved).map_err(|e| Error::Storage(e.to_string()))?;
    store.set_setting(KEY, &value)
}

/// One account's category folders that still exist, by id.
fn targets(store: &Store, account: AccountId) -> BTreeMap<Category, mail_core::FolderId> {
    load(store)
        .remove(&account.0.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(c, name)| Some((c, store.folder_id(account, &name).ok()??)))
        .collect()
}

/// What making the folders did.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Made {
    /// Inbox messages filed into the folders.
    pub filed: usize,
    /// Accounts whose folders couldn't be made, with why.
    pub failed: Vec<String>,
}

/// The accounts that have category folders.
#[tauri::command]
pub fn category_folder_accounts(state: State<'_, AppState>) -> Vec<AccountId> {
    let store = state.store.lock().unwrap();
    let mut ids: Vec<AccountId> = load(&store)
        .keys()
        .filter_map(|k| k.parse().ok().map(AccountId))
        .collect();
    ids.sort_by_key(|a| a.0);
    ids
}

/// Makes the category folders on each of `accounts` (those that already
/// have them keep them), then files their inbox mail into them.
#[tauri::command]
pub async fn make_category_folders(
    app: AppHandle,
    state: State<'_, AppState>,
    accounts: Vec<AccountId>,
) -> Result<Made> {
    let mut made = Made::default();
    for id in accounts {
        let account = state.account(id)?;
        let result = async {
            let secret = state.secret(&account).await?;
            make(&state, &account, &secret).await
        }
        .await;
        emit(&app, Event::MailboxChanged { account: id });
        match result {
            Ok(folders) => {
                let store = state.store.lock().unwrap();
                let mut saved = load(&store);
                saved.insert(id.0.to_string(), folders);
                save(&store, &saved)?;
            }
            Err(e) => made.failed.push(format!("{}: {e}", account.email)),
        }
    }
    made.filed = file_all(&app, &state)?;
    Ok(made)
}

/// Files every inbox's sorted mail into its account's category folders.
/// Returns how many messages were moved.
#[tauri::command]
pub fn file_into_category_folders(app: AppHandle, state: State<'_, AppState>) -> Result<usize> {
    file_all(&app, &state)
}

/// Stops filing mail into category folders. The folders and the mail in
/// them stay on the server.
#[tauri::command]
pub fn stop_category_folders(state: State<'_, AppState>) -> Result<()> {
    save(&state.store.lock().unwrap(), &Saved::new())
}

/// Makes whichever category folders an account lacks; returns them all.
async fn make(
    state: &AppState,
    account: &Account,
    secret: &Secret,
) -> Result<BTreeMap<Category, String>> {
    let list = || state.store.lock().unwrap().list_folders(account.id);
    // Servers without folders inside folders get them at the top.
    let nested = list()?.iter().any(|f| f.delimiter.is_some());
    let parent = if nested {
        Some(ensure(state, account, secret, None, PARENT).await?)
    } else {
        None
    };
    let mut made = BTreeMap::new();
    for category in FILED {
        let folder = ensure(state, account, secret, parent.as_ref(), label(category)).await?;
        made.insert(category, folder.name);
    }
    Ok(made)
}

/// The folder `name` inside `parent`, made when it isn't there yet.
async fn ensure(
    state: &AppState,
    account: &Account,
    secret: &Secret,
    parent: Option<&Folder>,
    name: &str,
) -> Result<Folder> {
    let folders = state.store.lock().unwrap().list_folders(account.id)?;
    let full = server_name(&folders, parent, name)?;
    let existing = |folders: &[Folder]| {
        folders
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(&full))
            .cloned()
    };
    if let Some(folder) = existing(&folders) {
        return Ok(folder);
    }
    mail_sync::create_folder(&state.store, account, secret, &full).await?;
    let folders = state.store.lock().unwrap().list_folders(account.id)?;
    existing(&folders)
        .ok_or_else(|| Error::Protocol(format!("the server made {name} but does not list it")))
}

/// Which of `ids` go to which category folder of `account`: those whose
/// category the user confirmed or the guess is sure of.
fn sort(
    store: &Store,
    account: AccountId,
    ids: &[i64],
) -> Result<Vec<(mail_core::FolderId, Vec<i64>)>> {
    let targets = targets(store, account);
    if targets.is_empty() || ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut moves: HashMap<mail_core::FolderId, Vec<i64>> = HashMap::new();
    for item in store.cleanup_items(ids)? {
        if !(item.confirmed || item.sure) {
            continue;
        }
        if let Some(folder) = targets.get(&item.category) {
            moves.entry(*folder).or_default().push(item.id);
        }
    }
    Ok(moves.into_iter().collect())
}

fn file_all(app: &AppHandle, state: &AppState) -> Result<usize> {
    let accounts: Vec<AccountId> = {
        let store = state.store.lock().unwrap();
        load(&store)
            .keys()
            .filter_map(|k| k.parse().ok().map(AccountId))
            .collect()
    };
    let mut filed = 0;
    for account in accounts {
        let moves = {
            let store = state.store.lock().unwrap();
            let Some(inbox) = store
                .folder_with_role(account, FolderRole::Inbox)?
                .and_then(|name| store.folder_id(account, &name).ok().flatten())
            else {
                continue;
            };
            let ids = store.folder_messages(inbox, false)?;
            sort(&store, account, &ids)?
        };
        for (folder, ids) in moves {
            move_to(app, state, &ids, folder)?;
            filed += ids.len();
        }
        // The new folders fill in with the next sync of every folder.
        request_sync(app, account, true);
    }
    Ok(filed)
}

/// Files new inbox mail into the account's category folders, and returns
/// the mail still to announce: what stayed in the inbox.
pub fn apply(app: &AppHandle, account: AccountId, new_mail: &[NewMail]) -> Vec<NewMail> {
    let state = app.state::<AppState>();
    let ids: Vec<i64> = new_mail.iter().map(|m| m.id).collect();
    let moves = match sort(&state.store.lock().unwrap(), account, &ids) {
        Ok(moves) if !moves.is_empty() => moves,
        _ => return new_mail.to_vec(),
    };
    let mut filed = Vec::new();
    for (folder, ids) in moves {
        if move_to(app, &state, &ids, folder).is_ok() {
            filed.extend(ids);
        }
    }
    new_mail
        .iter()
        .filter(|m| !filed.contains(&m.id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_folders_read_back() {
        let mut saved = Saved::new();
        saved.insert(
            "3".into(),
            BTreeMap::from([(Category::Updates, "Categories/Updates".to_owned())]),
        );
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(value["3"]["updates"], "Categories/Updates");
        let back: Saved = serde_json::from_value(value).unwrap();
        assert_eq!(back, saved);
    }
}
