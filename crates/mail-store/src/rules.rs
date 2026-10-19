//! Saved rules, and what they need to know about new messages.

use mail_core::rules::{Rule, RuleInput};
use mail_core::{AccountId, Error, Result};
use rusqlite::{OptionalExtension, Row, params};

use crate::storage;
use crate::store::Store;

/// What rules look at in a cached message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleMessage {
    pub id: i64,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub subject: Option<String>,
}

impl RuleMessage {
    pub fn input(&self) -> RuleInput<'_> {
        RuleInput {
            from: self.from.as_deref(),
            to: self.to.as_deref(),
            cc: self.cc.as_deref(),
            subject: self.subject.as_deref(),
        }
    }
}

const RULE_SELECT: &str =
    "SELECT id, account_id, name, enabled, match_all, conditions, actions FROM rules";

fn json<T: serde::de::DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn rule_row(row: &Row<'_>) -> rusqlite::Result<Rule> {
    Ok(Rule {
        id: row.get(0)?,
        account: AccountId(row.get(1)?),
        name: row.get(2)?,
        enabled: row.get(3)?,
        match_all: row.get(4)?,
        conditions: json(row, 5)?,
        actions: json(row, 6)?,
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Storage(e.to_string()))
}

impl Store {
    /// An account's rules, in the order they run.
    pub fn list_rules(&self, account: AccountId) -> Result<Vec<Rule>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "{RULE_SELECT} WHERE account_id = ?1 ORDER BY position, id"
            ))
            .map_err(storage)?;
        stmt.query_map([account.0], rule_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// Saves a rule: a new one (id 0) runs after the account's others.
    pub fn save_rule(&self, rule: &Rule) -> Result<Rule> {
        if let Some(problem) = rule.problem() {
            return Err(Error::Invalid(problem.into()));
        }
        let conditions = to_json(&rule.conditions)?;
        let actions = to_json(&rule.actions)?;
        let id = if rule.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO rules (account_id, name, enabled, match_all, conditions, actions, position) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, \
                       (SELECT COALESCE(MAX(position), 0) + 1 FROM rules WHERE account_id = ?1))",
                    params![rule.account.0, rule.name.trim(), rule.enabled, rule.match_all, conditions, actions],
                )
                .map_err(storage)?;
            self.conn.last_insert_rowid()
        } else {
            let changed = self
                .conn
                .execute(
                    "UPDATE rules SET name = ?2, enabled = ?3, match_all = ?4, conditions = ?5, actions = ?6 \
                     WHERE id = ?1 AND account_id = ?7",
                    params![rule.id, rule.name.trim(), rule.enabled, rule.match_all, conditions, actions, rule.account.0],
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err(Error::Invalid("That rule no longer exists.".into()));
            }
            rule.id
        };
        self.conn
            .query_row(&format!("{RULE_SELECT} WHERE id = ?1"), [id], rule_row)
            .map_err(storage)
    }

    pub fn delete_rule(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM rules WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Puts an account's rules in the order of `ids`; rules not listed keep
    /// their place after them.
    pub fn order_rules(&self, account: AccountId, ids: &[i64]) -> Result<()> {
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        for (position, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE rules SET position = ?1 WHERE id = ?2 AND account_id = ?3",
                params![position as i64 - ids.len() as i64, id, account.0],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }

    /// The cached messages among `ids`, as rules see them.
    pub fn rule_messages(&self, ids: &[i64]) -> Result<Vec<RuleMessage>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, from_addr, to_addrs, cc_addrs, subject FROM messages WHERE id = ?1",
            )
            .map_err(storage)?;
        let mut found = Vec::new();
        for id in ids {
            let row = stmt
                .query_row([id], |r| {
                    Ok(RuleMessage {
                        id: r.get(0)?,
                        from: r.get(1)?,
                        to: r.get(2)?,
                        cc: r.get(3)?,
                        subject: r.get(4)?,
                    })
                })
                .optional()
                .map_err(storage)?;
            found.extend(row);
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use mail_core::rules::{Condition, RuleAction, RuleField};
    use mail_core::{Endpoint, FolderId, NewAccount, Security};

    use super::*;

    fn account(store: &Store, email: &str) -> AccountId {
        let endpoint = Endpoint {
            host: "mail.example.test".into(),
            port: 993,
            security: Security::ImplicitTls,
        };
        store
            .add_account(&NewAccount {
                display_name: "Me".into(),
                email: email.into(),
                username: email.into(),
                imap: endpoint.clone(),
                smtp: endpoint,
                auth: Default::default(),
            })
            .unwrap()
            .id
    }

    fn rule(account: AccountId, text: &str) -> Rule {
        Rule {
            id: 0,
            account,
            name: format!("  {text}  "),
            enabled: true,
            match_all: true,
            conditions: vec![Condition {
                field: RuleField::From,
                text: text.into(),
            }],
            actions: vec![
                RuleAction::MarkRead,
                RuleAction::Move {
                    folder: FolderId(7),
                },
            ],
        }
    }

    fn names(store: &Store, account: AccountId) -> Vec<String> {
        store
            .list_rules(account)
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect()
    }

    #[test]
    fn rules_are_saved_ordered_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let me = account(&store, "me@example.test");
        let other = account(&store, "other@example.test");

        let a = store.save_rule(&rule(me, "a")).unwrap();
        let b = store.save_rule(&rule(me, "b")).unwrap();
        store.save_rule(&rule(other, "x")).unwrap();
        assert_eq!(a.name, "a");
        assert_eq!(a.actions, rule(me, "a").actions);
        assert_eq!(names(&store, me), ["a", "b"]);

        let mut changed = b.clone();
        changed.enabled = false;
        changed.name = "bee".into();
        assert!(!store.save_rule(&changed).unwrap().enabled);
        store.order_rules(me, &[b.id]).unwrap();
        assert_eq!(names(&store, me), ["bee", "a"]);

        // Saving a rule into another account's id doesn't touch it.
        let mut stolen = a.clone();
        stolen.account = other;
        assert!(store.save_rule(&stolen).is_err());

        let mut empty = rule(me, " ");
        assert!(store.save_rule(&empty).is_err());
        empty.conditions.clear();
        assert!(store.save_rule(&empty).is_err());

        store.delete_rule(a.id).unwrap();
        assert_eq!(names(&store, me), ["bee"]);
        store.remove_account(me).unwrap();
        assert_eq!(names(&store, other), ["x"]);
        assert!(names(&store, me).is_empty());
    }
}
