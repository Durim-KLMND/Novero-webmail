//! Rules: simple filters run on new mail as it arrives in the inbox, such as
//! "from X, move to folder Y" or "subject has Z, mark as read".

use serde::{Deserialize, Serialize};

use crate::{AccountId, FolderId};

/// A rule of one account. Rules run in `position` order, and every rule that
/// matches a message acts on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// 0 for a rule not saved yet.
    #[serde(default)]
    pub id: i64,
    pub account: AccountId,
    #[serde(default)]
    pub name: String,
    pub enabled: bool,
    /// Whether every condition must hold, rather than any one of them.
    pub match_all: bool,
    pub conditions: Vec<Condition>,
    pub actions: Vec<RuleAction>,
}

/// Holds when `field` contains `text`, ignoring case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub field: RuleField,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleField {
    From,
    /// To or Cc.
    To,
    Subject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleAction {
    MarkRead,
    Flag,
    Move { folder: FolderId },
    Archive,
    Delete,
}

impl RuleAction {
    /// Whether it takes the message out of the inbox.
    pub fn moves(&self) -> bool {
        matches!(
            self,
            RuleAction::Move { .. } | RuleAction::Archive | RuleAction::Delete
        )
    }
}

/// The parts of a message rules look at.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleInput<'a> {
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub cc: Option<&'a str>,
    pub subject: Option<&'a str>,
}

impl Condition {
    fn holds(&self, mail: &RuleInput<'_>) -> bool {
        let text = self.text.trim().to_lowercase();
        let has = |value: Option<&str>| value.is_some_and(|v| v.to_lowercase().contains(&text));
        match self.field {
            RuleField::From => has(mail.from),
            RuleField::To => has(mail.to) || has(mail.cc),
            RuleField::Subject => has(mail.subject),
        }
    }
}

impl Rule {
    /// Whether the rule acts on `mail`. A rule that is off, or has no
    /// condition, never does.
    pub fn matches(&self, mail: &RuleInput<'_>) -> bool {
        if !self.enabled {
            return false;
        }
        let conditions: Vec<&Condition> = self
            .conditions
            .iter()
            .filter(|c| !c.text.trim().is_empty())
            .collect();
        if conditions.is_empty() {
            return false;
        }
        if self.match_all {
            conditions.iter().all(|c| c.holds(mail))
        } else {
            conditions.iter().any(|c| c.holds(mail))
        }
    }

    /// Why the rule can't be saved, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        if !self.conditions.iter().any(|c| !c.text.trim().is_empty()) {
            return Some("Add a condition, such as who the mail is from.");
        }
        if self.actions.is_empty() {
            return Some("Choose what the rule does.");
        }
        if self.actions.iter().filter(|a| a.moves()).count() > 1 {
            return Some("A rule can move mail to one place only.");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(match_all: bool, conditions: &[(RuleField, &str)]) -> Rule {
        Rule {
            id: 0,
            account: AccountId(1),
            name: String::new(),
            enabled: true,
            match_all,
            conditions: conditions
                .iter()
                .map(|(field, text)| Condition {
                    field: *field,
                    text: (*text).into(),
                })
                .collect(),
            actions: vec![RuleAction::MarkRead],
        }
    }

    const MAIL: RuleInput<'static> = RuleInput {
        from: Some("Shop News <news@shop.test>"),
        to: Some("me@example.test"),
        cc: Some("Team <team@work.test>"),
        subject: Some("Weekly DEALS"),
    };

    #[test]
    fn conditions_match_any_or_all_ignoring_case() {
        assert!(rule(false, &[(RuleField::From, "SHOP.test")]).matches(&MAIL));
        assert!(rule(false, &[(RuleField::To, "team@")]).matches(&MAIL));
        assert!(
            rule(
                false,
                &[(RuleField::Subject, "nope"), (RuleField::Subject, "deals")]
            )
            .matches(&MAIL)
        );
        assert!(
            !rule(
                true,
                &[(RuleField::Subject, "nope"), (RuleField::Subject, "deals")]
            )
            .matches(&MAIL)
        );
        assert!(
            rule(
                true,
                &[(RuleField::From, "news"), (RuleField::Subject, " weekly ")]
            )
            .matches(&MAIL)
        );
        assert!(!rule(false, &[(RuleField::From, "team")]).matches(&RuleInput::default()));
    }

    #[test]
    fn empty_or_disabled_rules_never_match() {
        assert!(!rule(true, &[]).matches(&MAIL));
        assert!(!rule(true, &[(RuleField::From, "  ")]).matches(&MAIL));
        let mut off = rule(false, &[(RuleField::From, "shop")]);
        off.enabled = false;
        assert!(!off.matches(&MAIL));
    }

    #[test]
    fn rules_that_cannot_work_are_refused() {
        assert!(rule(true, &[(RuleField::From, " ")]).problem().is_some());
        let mut r = rule(true, &[(RuleField::From, "a")]);
        assert_eq!(r.problem(), None);
        r.actions.clear();
        assert!(r.problem().is_some());
        r.actions = vec![RuleAction::Archive, RuleAction::Delete];
        assert!(r.problem().is_some());
    }
}
