//! Signing in and finding server settings for accounts.
//!
//! Passwords and refresh tokens are kept in the operating system's
//! credential store ([`platform_store`]). [`discover`] finds server settings
//! from an email address, and [`oauth`] signs in through Google's and
//! Microsoft's own pages.

pub mod discover;
mod http;
pub mod oauth;
pub mod providers;
mod secrets;

pub use mail_core::{Error, Result};
pub use secrets::{MemoryStore, SecretStore, platform_store};
