//! Category folders: the user can pick one of their own folders for a
//! cleanup category, such as "Receipts" for Updates. Inbox mail of that
//! category is then filed there, when asked and as new mail arrives.
//! Nothing is set up by default and no folders are made.
//!
//! Mail is filed when the user confirmed its category or the guess is a
//! sure one. Spam has no folder of its own here: it goes to Junk.

use std::collections::{BTreeMap, HashMap};

use mail_core::categories::Category;
use mail_core::{AccountId, Error, FolderId, FolderRole, Result};
use mail_store::Store;
use mail_sync::NewMail;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::commands::move_to;
use crate::state::AppState;

const KEY: &str = "categoryFolders";

/// Each account's chosen folders, by folder id. Ids survive renames.
type Saved = HashMap<String, BTreeMap<Category, i64>>;

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

/// One category's folder in one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryFolder {
    pub account: AccountId,
    pub category: Category,
    pub folder: FolderId,
}

/// The chosen folders that still exist in their account.
fn chosen(store: &Store) -> Vec<CategoryFolder> {
    let mut out = Vec::new();
    for (account, folders) in load(store) {
        let Ok(account) = account.parse().map(AccountId) else {
            continue;
        };
        for (category, folder) in folders {
            let found = store.folder(FolderId(folder)).ok().flatten();
            if found.is_some_and(|f| f.account_id == account) {
                out.push(CategoryFolder {
                    account,
                    category,
                    folder: FolderId(folder),
                });
            }
        }
    }
    out.sort_by_key(|c| (c.account.0, c.category));
    out
}

#[tauri::command]
pub fn category_folders(state: State<'_, AppState>) -> Vec<CategoryFolder> {
    chosen(&state.store.lock().unwrap())
}

/// Files a category's inbox mail of an account into `folder`, one of the
/// user's own folders or Archive; `None` keeps it in the inbox.
#[tauri::command]
pub fn set_category_folder(
    state: State<'_, AppState>,
    account: AccountId,
    category: Category,
    folder: Option<FolderId>,
) -> Result<Vec<CategoryFolder>> {
    if category == Category::Spam {
        return Err(Error::Invalid("Spam always goes to Junk.".into()));
    }
    let store = state.store.lock().unwrap();
    if let Some(id) = folder {
        let found = store
            .folder(id)?
            .ok_or_else(|| Error::Invalid("that folder no longer exists".into()))?;
        if found.account_id != account {
            return Err(Error::Invalid(
                "Choose a folder of the same account.".into(),
            ));
        }
        if !matches!(found.role, FolderRole::Other | FolderRole::Archive) {
            return Err(Error::Invalid(
                "Choose one of your own folders or Archive.".into(),
            ));
        }
    }
    let mut saved = load(&store);
    let entry = saved.entry(account.0.to_string()).or_default();
    match folder {
        Some(id) => {
            entry.insert(category, id.0);
        }
        None => {
            entry.remove(&category);
        }
    }
    saved.retain(|_, folders| !folders.is_empty());
    save(&store, &saved)?;
    Ok(chosen(&store))
}

/// Files every inbox's sorted mail into the chosen folders. Returns how
/// many messages were moved.
#[tauri::command]
pub fn file_into_category_folders(app: AppHandle, state: State<'_, AppState>) -> Result<usize> {
    let accounts: Vec<AccountId> = {
        let store = state.store.lock().unwrap();
        let mut ids: Vec<AccountId> = chosen(&store).iter().map(|c| c.account).collect();
        ids.dedup();
        ids
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
            move_to(&app, &state, &ids, folder)?;
            filed += ids.len();
        }
    }
    Ok(filed)
}

/// Which of `ids` go to which chosen folder of `account`: those whose
/// category the user confirmed or the guess is sure of.
fn sort(store: &Store, account: AccountId, ids: &[i64]) -> Result<Vec<(FolderId, Vec<i64>)>> {
    let targets: HashMap<Category, FolderId> = chosen(store)
        .into_iter()
        .filter(|c| c.account == account)
        .map(|c| (c.category, c.folder))
        .collect();
    if targets.is_empty() || ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut moves: HashMap<FolderId, Vec<i64>> = HashMap::new();
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

/// Files new inbox mail into the account's chosen folders, and returns the
/// mail still to announce: what stayed in the inbox.
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
        saved.insert("3".into(), BTreeMap::from([(Category::Updates, 42)]));
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(value["3"]["updates"], 42);
        let back: Saved = serde_json::from_value(value).unwrap();
        assert_eq!(back, saved);
    }
}
