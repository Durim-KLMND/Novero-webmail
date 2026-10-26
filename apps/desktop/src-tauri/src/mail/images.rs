//! Senders whose remote images always load, like Outlook's safe senders.
//! Each entry is an address (jane@example.com) or a domain (example.com),
//! which also covers its subdomains (mail.example.com).

use mail_core::{Error, Result};
use mail_store::Store;
use tauri::State;

use crate::state::AppState;

const KEY: &str = "imageSenders";

/// The saved senders and domains, sorted.
pub fn load(store: &Store) -> Vec<String> {
    store
        .setting(KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn save(store: &Store, list: &[String]) -> Result<()> {
    store.set_setting(KEY, &serde_json::json!(list))
}

/// An address or domain as it is kept: lower case, without a leading "@"
/// or "*.". Anything else is refused.
fn entry(text: &str) -> Result<String> {
    let text = text.trim().to_lowercase();
    let text = text
        .strip_prefix("*.")
        .or_else(|| text.strip_prefix('@'))
        .unwrap_or(&text);
    let (local, domain) = match text.rsplit_once('@') {
        Some((local, domain)) => (Some(local), domain),
        None => (None, text),
    };
    let domain_ok = domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-');
    let local_ok = local.is_none_or(|l| !l.is_empty() && !l.contains(char::is_whitespace));
    if domain_ok && local_ok {
        Ok(text.to_owned())
    } else {
        Err(Error::Invalid(
            "Enter an address, such as jane@example.com, or a domain, such as example.com.".into(),
        ))
    }
}

/// Whether `email` (lower case) is in the list, by address or domain.
pub fn trusted(list: &[String], email: &str) -> bool {
    let Some((_, domain)) = email.rsplit_once('@') else {
        return false;
    };
    list.iter().any(|e| {
        if e.contains('@') {
            e == email
        } else {
            domain == e || domain.ends_with(&format!(".{e}"))
        }
    })
}

#[tauri::command]
pub fn list_image_senders(state: State<'_, AppState>) -> Vec<String> {
    load(&state.store.lock().unwrap())
}

/// Always loads images from an address or domain; returns the new list.
#[tauri::command]
pub fn allow_images_from(state: State<'_, AppState>, sender: String) -> Result<Vec<String>> {
    let sender = entry(&sender)?;
    let store = state.store.lock().unwrap();
    let mut list = load(&store);
    if !list.contains(&sender) {
        list.push(sender);
        list.sort();
        save(&store, &list)?;
    }
    Ok(list)
}

/// Blocks images from an address or domain again; returns the new list.
#[tauri::command]
pub fn forget_image_sender(state: State<'_, AppState>, sender: String) -> Result<Vec<String>> {
    let store = state.store.lock().unwrap();
    let mut list = load(&store);
    list.retain(|e| *e != sender);
    save(&store, &list)?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_tidied_or_refused() {
        assert_eq!(entry(" Jane@Example.com ").unwrap(), "jane@example.com");
        assert_eq!(entry("@example.com").unwrap(), "example.com");
        assert_eq!(entry("*.example.com").unwrap(), "example.com");
        for bad in [
            "",
            "example",
            "jane@",
            "@",
            "a b.com",
            "jane@example",
            ".com",
        ] {
            assert!(entry(bad).is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn addresses_match_exactly_and_domains_cover_subdomains() {
        let list = vec!["jane@example.com".to_owned(), "shop.test".to_owned()];
        assert!(trusted(&list, "jane@example.com"));
        assert!(!trusted(&list, "john@example.com"));
        assert!(trusted(&list, "news@shop.test"));
        assert!(trusted(&list, "news@mail.shop.test"));
        assert!(!trusted(&list, "news@badshop.test"));
        assert!(!trusted(&list, ""));
    }
}
