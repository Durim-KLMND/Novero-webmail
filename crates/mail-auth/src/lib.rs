//! Credentials and server settings for accounts.
//!
//! Passwords are kept in the operating system's credential store
//! ([`platform_store`]); [`providers`] suggests server settings from an
//! email address. OAuth2 arrives in phase 4.

pub mod providers;
mod secrets;

pub use mail_core::{Error, Result};
pub use secrets::{MemoryStore, SecretStore, platform_store};
