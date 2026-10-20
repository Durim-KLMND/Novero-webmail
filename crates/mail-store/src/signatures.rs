//! An account's signatures, and message templates.

use mail_core::{AccountId, Error, Result, Signature, Template};
use rusqlite::{OptionalExtension, Row, params};

use crate::storage;
use crate::store::Store;

impl Store {
    /// Every account's signatures, by account and then name.
    pub fn signatures(&self) -> Result<Vec<Signature>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, account, name, text, html FROM signatures \
                 ORDER BY account, name COLLATE NOCASE, id",
            )
            .map_err(storage)?;
        let rows = stmt.query_map([], signature_row).map_err(storage)?;
        rows.collect::<rusqlite::Result<_>>().map_err(storage)
    }

    /// Adds the signature (id 0) or changes it, and returns it as stored.
    /// An account's first signature becomes its default for everything.
    pub fn save_signature(&self, signature: &Signature) -> Result<Signature> {
        let mut saved = signature.clone();
        if signature.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO signatures (account, name, text, html) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        signature.account.0,
                        signature.name,
                        signature.text,
                        signature.html
                    ],
                )
                .map_err(storage)?;
            saved.id = self.conn.last_insert_rowid();
            self.conn
                .execute(
                    "UPDATE accounts SET signature_new = ?2, signature_reply = ?2 WHERE id = ?1 \
                     AND (SELECT COUNT(*) FROM signatures WHERE account = ?1) = 1",
                    params![signature.account.0, saved.id],
                )
                .map_err(storage)?;
        } else {
            let changed = self
                .conn
                .execute(
                    "UPDATE signatures SET name = ?3, text = ?4, html = ?5 \
                     WHERE id = ?1 AND account = ?2",
                    params![
                        signature.id,
                        signature.account.0,
                        signature.name,
                        signature.text,
                        signature.html
                    ],
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err(Error::Invalid("that signature no longer exists".into()));
            }
        }
        Ok(saved)
    }

    /// Deletes a signature; messages that started with it start with none.
    pub fn delete_signature(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM signatures WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Picks the signatures the account's new messages and replies start
    /// with. Each must be one of the account's, or `None`.
    pub fn set_default_signatures(
        &self,
        account: AccountId,
        new: Option<i64>,
        reply: Option<i64>,
    ) -> Result<()> {
        for id in [new, reply].into_iter().flatten() {
            let owner: Option<i64> = self
                .conn
                .query_row("SELECT account FROM signatures WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(storage)?;
            if owner != Some(account.0) {
                return Err(Error::Invalid(
                    "that signature belongs to another account".into(),
                ));
            }
        }
        self.conn
            .execute(
                "UPDATE accounts SET signature_new = ?2, signature_reply = ?3 WHERE id = ?1",
                params![account.0, new, reply],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The saved templates, by name.
    pub fn templates(&self) -> Result<Vec<Template>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, subject, text, html FROM templates \
                 ORDER BY name COLLATE NOCASE, id",
            )
            .map_err(storage)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Template {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    subject: r.get(2)?,
                    text: r.get(3)?,
                    html: r.get(4)?,
                })
            })
            .map_err(storage)?;
        rows.collect::<rusqlite::Result<_>>().map_err(storage)
    }

    /// Adds the template (id 0) or changes it, and returns it as stored.
    pub fn save_template(&self, template: &Template) -> Result<Template> {
        let mut saved = template.clone();
        if template.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO templates (name, subject, text, html) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        template.name,
                        template.subject,
                        template.text,
                        template.html
                    ],
                )
                .map_err(storage)?;
            saved.id = self.conn.last_insert_rowid();
        } else {
            let changed = self
                .conn
                .execute(
                    "UPDATE templates SET name = ?2, subject = ?3, text = ?4, html = ?5 WHERE id = ?1",
                    params![
                        template.id,
                        template.name,
                        template.subject,
                        template.text,
                        template.html
                    ],
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err(Error::Invalid("that template no longer exists".into()));
            }
        }
        Ok(saved)
    }

    pub fn delete_template(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM templates WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }
}

fn signature_row(r: &Row<'_>) -> rusqlite::Result<Signature> {
    Ok(Signature {
        id: r.get(0)?,
        account: AccountId(r.get(1)?),
        name: r.get(2)?,
        text: r.get(3)?,
        html: r.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::{Endpoint, NewAccount, Security};

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

    fn signature(account: AccountId, name: &str, text: &str) -> Signature {
        Signature {
            id: 0,
            account,
            name: name.into(),
            text: text.into(),
            html: String::new(),
        }
    }

    #[test]
    fn accounts_start_with_their_chosen_signatures() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory(dir.path()).unwrap();
        let a = account(&store, "a@example.test");
        let b = account(&store, "b@example.test");
        let first = store
            .save_signature(&signature(a, "Work", "Alice\nNovero"))
            .unwrap();
        // The first one is used for everything.
        let acc = store.account(a).unwrap().unwrap();
        assert_eq!(
            (acc.signature_new, acc.signature_reply),
            (Some(first.id), Some(first.id))
        );
        assert_eq!(acc.signature, "Alice\nNovero");
        assert_eq!(acc.reply_signature, "Alice\nNovero");

        let short = store.save_signature(&signature(a, "Short", "A.")).unwrap();
        store
            .set_default_signatures(a, Some(first.id), Some(short.id))
            .unwrap();
        let acc = store.account(a).unwrap().unwrap();
        assert_eq!(
            (acc.signature.as_str(), acc.reply_signature.as_str()),
            ("Alice\nNovero", "A.")
        );
        // Another account's signature can't be picked.
        assert!(
            store
                .set_default_signatures(b, Some(short.id), None)
                .is_err()
        );

        let mut changed = first.clone();
        changed.html = "<p><b>Alice</b></p>".into();
        store.save_signature(&changed).unwrap();
        assert_eq!(
            store.account(a).unwrap().unwrap().signature_html,
            "<p><b>Alice</b></p>"
        );

        // Deleting one leaves its messages with none.
        store.delete_signature(short.id).unwrap();
        let acc = store.account(a).unwrap().unwrap();
        assert_eq!(
            (acc.signature_reply, acc.reply_signature.as_str()),
            (None, "")
        );
        let names: Vec<_> = store
            .signatures()
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["Work"]);
    }

    #[test]
    fn templates_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory(dir.path()).unwrap();
        let mut t = store
            .save_template(&Template {
                id: 0,
                name: "Thanks".into(),
                subject: "Thank you".into(),
                text: "Thanks for your order.".into(),
                html: String::new(),
            })
            .unwrap();
        t.name = "Order thanks".into();
        store.save_template(&t).unwrap();
        assert_eq!(store.templates().unwrap(), [t.clone()]);
        store.delete_template(t.id).unwrap();
        assert!(store.templates().unwrap().is_empty());
        t.id = 99;
        assert!(store.save_template(&t).is_err());
    }
}
