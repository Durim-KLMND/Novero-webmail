//! Credentials, OAuth2 and autodiscovery. Phases 1 and 4 of the roadmap fill this in.

pub use mail_core::{Error, Result};

/// Name of this crate, used in logs.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::CRATE, "mail-auth");
    }
}
