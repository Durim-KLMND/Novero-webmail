//! Emoji reactions to the user's mail. A reaction is a small reply, found
//! by downloading it (`mail_mime::reaction`); this keeps what was found and
//! which messages are still to be looked at.

use mail_core::{AccountId, Result};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;

use crate::storage;
use crate::store::Store;

/// Someone's reaction to a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReaction {
    pub emoji: String,
    pub from: Option<String>,
    pub date: Option<i64>,
}

fn reaction(r: &Row<'_>) -> rusqlite::Result<MessageReaction> {
    Ok(MessageReaction {
        emoji: r.get(0)?,
        from: r.get(1)?,
        date: r.get(2)?,
    })
}

impl Store {
    /// Messages of an account that may be reactions and haven't been looked
    /// at: small ones in conversations with the user's own mail from `since`
    /// on, that came after it. Newest first.
    pub fn reaction_candidates(
        &self,
        account: AccountId,
        since: i64,
        max_size: u32,
        limit: u32,
    ) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT m.id, max(m.date) FROM messages s \
                 JOIN folders sf ON sf.id = s.folder_id \
                 JOIN messages m ON m.thread_id = s.thread_id \
                 JOIN folders mf ON mf.id = m.folder_id \
                 WHERE sf.account_id = ?1 AND sf.role = 'sent' AND s.thread_id IS NOT NULL \
                 AND coalesce(s.date, 0) >= ?2 AND mf.account_id = ?1 \
                 AND m.reaction_checked = 0 AND coalesce(m.size, 0) <= ?3 \
                 AND coalesce(m.date, 0) >= coalesce(s.date, 0) \
                 AND coalesce(mf.role, '') NOT IN ('sent', 'drafts', 'trash') \
                 AND NOT EXISTS (SELECT 1 FROM messages o JOIN folders of ON of.id = o.folder_id \
                     WHERE o.message_id = m.message_id AND of.account_id = ?1 \
                     AND of.role IN ('sent', 'drafts')) \
                 GROUP BY m.id ORDER BY max(m.date) DESC LIMIT ?4",
            )
            .map_err(storage)?;
        stmt.query_map(params![account.0, since, max_size, limit], |r| r.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// Records what a message turned out to be: a reaction (its emoji and
    /// the Message-ID it is to) or not. Copies of it in other folders of the
    /// account (Gmail's All Mail) are the same mail and change with it.
    pub fn set_reaction(&self, message: i64, reaction: Option<(&str, &str)>) -> Result<()> {
        let (emoji, to) = reaction.unzip();
        self.conn
            .execute(
                "UPDATE messages SET reaction = ?2, reaction_to = ?3, reaction_checked = 1 \
                 WHERE id = ?1 OR id IN (SELECT c.id FROM messages m \
                     JOIN folders f ON f.id = m.folder_id \
                     JOIN messages c ON c.message_id = m.message_id \
                     JOIN folders cf ON cf.id = c.folder_id AND cf.account_id = f.account_id \
                     WHERE m.id = ?1)",
                params![message, emoji, to],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The reactions to the message with `message_id` in an account, oldest first.
    pub fn reactions_to(
        &self,
        account: AccountId,
        message_id: &str,
    ) -> Result<Vec<MessageReaction>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT m.reaction, m.from_addr, max(m.date) FROM messages m \
                 JOIN folders f ON f.id = m.folder_id \
                 WHERE f.account_id = ?1 AND m.reaction_to = ?2 AND m.reaction IS NOT NULL \
                 GROUP BY coalesce(m.message_id, m.id) ORDER BY 3, m.id",
            )
            .map_err(storage)?;
        stmt.query_map(params![account.0, message_id], reaction)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// The reactions anywhere in a conversation, oldest first.
    pub(crate) fn thread_reactions(&self, thread: i64) -> Result<Vec<MessageReaction>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT m.reaction, m.from_addr, max(m.date) FROM messages m \
                 WHERE m.thread_id = ?1 AND m.reaction IS NOT NULL \
                 GROUP BY coalesce(m.message_id, m.id) ORDER BY 3, m.id",
            )
            .map_err(storage)?;
        stmt.query_map([thread], reaction)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// Whether the account has the message with `message_id`.
    pub fn has_message_id(&self, account: AccountId, message_id: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id \
                 WHERE m.message_id = ?2 AND f.account_id = ?1)",
                params![account.0, message_id],
                |r| r.get(0),
            )
            .map_err(storage)
    }

    /// The reaction a message is, if it is one.
    pub fn reaction_of(&self, message: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT reaction FROM messages WHERE id = ?1",
                [message],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)
            .map(Option::flatten)
    }
}
