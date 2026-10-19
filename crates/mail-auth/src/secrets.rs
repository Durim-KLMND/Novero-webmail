use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mail_core::Result;

/// Service name passwords are stored under.
#[cfg(windows)]
const SERVICE: &str = "Novero Mail";

/// Where account passwords are kept.
pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, secret: &str) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// The credential store for this platform: Windows Credential Manager on
/// Windows. Other platforms keep secrets in memory for development only, so
/// passwords there are forgotten when the app quits.
pub fn platform_store() -> Arc<dyn SecretStore> {
    #[cfg(windows)]
    {
        match windows::CredentialManager::new() {
            Ok(store) => return Arc::new(store),
            Err(err) => eprintln!("credential manager unavailable, using memory: {err}"),
        }
    }
    Arc::new(MemoryStore::default())
}

/// Keeps secrets in memory. Used in tests and as a development fallback.
#[derive(Default)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn set(&self, key: &str, secret: &str) -> Result<()> {
        self.secrets
            .lock()
            .unwrap()
            .insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.secrets.lock().unwrap().get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.secrets.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use keyring_core::{Entry, Error as KeyringError};
    use mail_core::{Error, Result};

    use super::{SERVICE, SecretStore};

    pub struct CredentialManager;

    impl CredentialManager {
        pub fn new() -> Result<Self> {
            let store = windows_native_keyring_store::Store::new().map_err(auth)?;
            keyring_core::set_default_store(store);
            Ok(Self)
        }
    }

    fn entry(key: &str) -> Result<Entry> {
        Entry::new(SERVICE, key).map_err(auth)
    }

    fn auth(err: KeyringError) -> Error {
        Error::Auth(format!("credential store: {err}"))
    }

    impl SecretStore for CredentialManager {
        fn set(&self, key: &str, secret: &str) -> Result<()> {
            entry(key)?.set_password(secret).map_err(auth)
        }

        fn get(&self, key: &str) -> Result<Option<String>> {
            match entry(key)?.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(KeyringError::NoEntry) => Ok(None),
                Err(err) => Err(auth(err)),
            }
        }

        fn delete(&self, key: &str) -> Result<()> {
            match entry(key)?.delete_credential() {
                Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
                Err(err) => Err(auth(err)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_sets_reads_and_deletes() {
        let store = MemoryStore::default();
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "secret").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("secret"));
        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
    }
}
