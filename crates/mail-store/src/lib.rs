//! Local SQLite cache and message files. Phase 1 of the roadmap fills this in.

pub use mail_core::{Error, Result};

/// Name of this crate, used in logs.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::CRATE, "mail-store");
    }
}
