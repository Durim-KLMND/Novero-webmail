//! Snoozed messages: hidden from their folder until a set time, then back.

use mail_core::{AccountId, Result};
use rusqlite::{params, params_from_iter};

use crate::storage;
use crate::store::Store;

/// A snoozed message whose time has come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Woken {
    pub id: i64,
    pub account: AccountId,
    pub from: Option<String>,
    pub subject: Option<String>,
}

impl Store {
    /// Hides messages until `until` (UTC seconds). Snoozing again moves the time.
    pub fn snooze(&self, ids: &[i64], until: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "INSERT INTO snoozes (message, until) SELECT id, ?2 FROM messages WHERE id = ?1 \
                 ON CONFLICT (message) DO UPDATE SET until = excluded.until",
            )
            .map_err(storage)?;
        for id in ids {
            stmt.execute(params![id, until]).map_err(storage)?;
        }
        Ok(())
    }

    /// Brings snoozed messages back now.
    pub fn unsnooze(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.conn
            .execute(
                &format!(
                    "DELETE FROM snoozes WHERE message IN ({})",
                    vec!["?"; ids.len()].join(",")
                ),
                params_from_iter(ids),
            )
            .map_err(storage)?;
        Ok(())
    }

    /// When a message comes back from snooze, if it is snoozed.
    pub fn snoozed_until(&self, id: i64) -> Result<Option<i64>> {
        use rusqlite::OptionalExtension;
        self.conn
            .query_row("SELECT until FROM snoozes WHERE message = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)
    }

    /// How many messages are snoozed.
    pub fn snoozed_count(&self) -> Result<u32> {
        self.conn
            .query_row("SELECT count(*) FROM snoozes", [], |r| r.get(0))
            .map_err(storage)
    }

    /// Ends the snoozes due at `now` and returns those messages.
    pub fn wake_snoozed(&self, now: i64) -> Result<Vec<Woken>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT m.id, f.account_id, m.from_addr, m.subject FROM snoozes s \
                 JOIN messages m ON m.id = s.message JOIN folders f ON f.id = m.folder_id \
                 WHERE s.until <= ?1 ORDER BY s.until, m.id",
            )
            .map_err(storage)?;
        let woken: Vec<Woken> = stmt
            .query_map([now], |r| {
                Ok(Woken {
                    id: r.get(0)?,
                    account: AccountId(r.get(1)?),
                    from: r.get(2)?,
                    subject: r.get(3)?,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        self.conn
            .execute("DELETE FROM snoozes WHERE until <= ?1", [now])
            .map_err(storage)?;
        Ok(woken)
    }
}
