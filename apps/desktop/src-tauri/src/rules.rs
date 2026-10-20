//! Rules on new mail: the commands that edit them, and running them as mail
//! arrives in the inbox or, when asked, on the mail already in a folder.

use std::collections::{BTreeMap, BTreeSet};

use mail_core::rules::{Rule, RuleAction};
use mail_core::{AccountId, Error, FlagKind, FolderId, FolderRole, Op, Result};
use mail_sync::NewMail;
use tauri::{AppHandle, Manager, State};

use crate::commands::{change, move_to, to_role};
use crate::state::AppState;

#[tauri::command]
pub fn list_rules(state: State<'_, AppState>, account: AccountId) -> Result<Vec<Rule>> {
    state.store.lock().unwrap().list_rules(account)
}

/// Saves a rule and returns it as stored (with its id when new).
#[tauri::command]
pub fn save_rule(state: State<'_, AppState>, rule: Rule) -> Result<Rule> {
    let store = state.store.lock().unwrap();
    for action in &rule.actions {
        if let RuleAction::Move { folder } = action {
            let found = store.folder(*folder)?;
            if found.is_none_or(|f| f.account_id != rule.account) {
                return Err(Error::Invalid(
                    "Choose a folder of this account to move mail to.".into(),
                ));
            }
        }
    }
    store.save_rule(&rule)
}

#[tauri::command]
pub fn delete_rule(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().delete_rule(id)
}

/// Puts an account's rules in the order of `ids`.
#[tauri::command]
pub fn order_rules(state: State<'_, AppState>, account: AccountId, ids: Vec<i64>) -> Result<()> {
    state.store.lock().unwrap().order_rules(account, &ids)
}

/// What the rules decided for a batch of messages.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    read: Vec<i64>,
    flag: Vec<i64>,
    /// Where each moved message goes: the first moving action that matched.
    moves: BTreeMap<i64, RuleAction>,
}

fn plan(rules: &[Rule], messages: &[mail_store::RuleMessage]) -> Plan {
    let mut plan = Plan::default();
    for message in messages {
        let input = message.input();
        for rule in rules.iter().filter(|r| r.matches(&input)) {
            for action in &rule.actions {
                match action {
                    RuleAction::MarkRead if !plan.read.contains(&message.id) => {
                        plan.read.push(message.id)
                    }
                    RuleAction::Flag if !plan.flag.contains(&message.id) => {
                        plan.flag.push(message.id)
                    }
                    a if a.moves() => {
                        plan.moves.entry(message.id).or_insert_with(|| a.clone());
                    }
                    _ => {}
                }
            }
        }
    }
    plan
}

/// Runs the account's rules on new inbox mail and returns the mail still to
/// announce: what they left unread in the inbox.
pub fn apply(app: &AppHandle, account: AccountId, new_mail: &[NewMail]) -> Vec<NewMail> {
    let state = app.state::<AppState>();
    let found = {
        let store = state.store.lock().unwrap();
        let ids: Vec<i64> = new_mail.iter().map(|m| m.id).collect();
        store
            .list_rules(account)
            .and_then(|rules| Ok((store.rule_messages(&ids)?, rules)))
    };
    let (messages, rules) = match found {
        Ok(found) if !found.1.is_empty() => found,
        _ => return new_mail.to_vec(),
    };
    let plan = plan(&rules, &messages);
    let failed = act(app, &state, &plan);
    new_mail
        .iter()
        .filter(|m| {
            failed.contains(&m.id) || !(plan.read.contains(&m.id) || plan.moves.contains_key(&m.id))
        })
        .cloned()
        .collect()
}

/// Runs rules on the mail already in a folder: one rule (on or off), or
/// every rule of the account that is on. Returns how many messages matched.
#[tauri::command]
pub async fn run_rules(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: FolderId,
    rule: Option<i64>,
) -> Result<usize> {
    let (source, messages, rules) = {
        let store = state.store.lock().unwrap();
        let source = store
            .folder(folder)?
            .ok_or_else(|| Error::Invalid("that folder no longer exists".into()))?;
        let rules: Vec<Rule> = store
            .list_rules(source.account_id)?
            .into_iter()
            .filter(|r| match rule {
                Some(id) => r.id == id,
                None => r.enabled,
            })
            .collect();
        (source, store.folder_rule_messages(folder)?, rules)
    };
    let mut plan = plan(&rules, &messages);
    // Mail that is already where a rule would put it stays.
    plan.moves
        .retain(|_, action| !already_there(action, source.id, source.role));
    let failed = act(&app, &state, &plan);
    let acted: BTreeSet<i64> = plan
        .read
        .iter()
        .chain(&plan.flag)
        .chain(plan.moves.keys())
        .filter(|id| !failed.contains(id))
        .copied()
        .collect();
    Ok(acted.len())
}

/// Whether a moving action would leave mail in `folder` where it is.
fn already_there(action: &RuleAction, folder: FolderId, role: FolderRole) -> bool {
    match action {
        RuleAction::Move { folder: to } => *to == folder,
        RuleAction::Archive => role == FolderRole::Archive,
        RuleAction::Delete => role == FolderRole::Trash,
        _ => false,
    }
}

/// Carries out a plan; returns the messages it couldn't act on.
fn act(app: &AppHandle, state: &AppState, plan: &Plan) -> Vec<i64> {
    let flag = |ids: &[i64], flag| {
        change(app, state, ids, |m| Op::SetFlag {
            folder: m.folder_name.clone(),
            uid_validity: m.uid_validity,
            uids: Vec::new(),
            flag,
            value: true,
        })
    };
    // A rule that can't act (its folder was removed, say) leaves the mail be.
    let mut failed = Vec::new();
    if !plan.read.is_empty() && flag(&plan.read, FlagKind::Seen).is_err() {
        failed.extend(&plan.read);
    }
    if !plan.flag.is_empty() {
        let _ = flag(&plan.flag, FlagKind::Flagged);
    }
    let mut groups: BTreeMap<String, (RuleAction, Vec<i64>)> = BTreeMap::new();
    for (id, action) in &plan.moves {
        let key = format!("{action:?}");
        groups
            .entry(key)
            .or_insert_with(|| (action.clone(), Vec::new()))
            .1
            .push(*id);
    }
    for (action, ids) in groups.into_values() {
        let done = match action {
            RuleAction::Move { folder } => move_to(app, state, &ids, folder),
            RuleAction::Archive => to_role(app, state, &ids, FolderRole::Archive),
            _ => to_role(app, state, &ids, FolderRole::Trash),
        };
        if done.is_err() {
            failed.extend(ids);
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use mail_core::FolderId;
    use mail_core::rules::{Condition, RuleField};
    use mail_store::RuleMessage;

    use super::*;

    fn rule(from: &str, actions: Vec<RuleAction>) -> Rule {
        Rule {
            id: 0,
            account: AccountId(1),
            name: String::new(),
            enabled: true,
            match_all: true,
            conditions: vec![Condition {
                field: RuleField::From,
                text: from.into(),
            }],
            actions,
        }
    }

    fn message(id: i64, from: &str) -> RuleMessage {
        RuleMessage {
            id,
            from: Some(from.into()),
            ..RuleMessage::default()
        }
    }

    #[test]
    fn every_matching_rule_acts_and_the_first_move_wins() {
        let rules = [
            rule(
                "shop",
                vec![
                    RuleAction::MarkRead,
                    RuleAction::Move {
                        folder: FolderId(5),
                    },
                ],
            ),
            rule("news", vec![RuleAction::Flag, RuleAction::Delete]),
            rule("@", vec![RuleAction::MarkRead]),
        ];
        let messages = [
            message(1, "news@shop.test"),
            message(2, "news@paper.test"),
            message(3, "friend@home.test"),
        ];
        let plan = plan(&rules, &messages);
        assert_eq!(plan.read, [1, 2, 3]);
        assert_eq!(plan.flag, [1, 2]);
        assert_eq!(
            plan.moves,
            BTreeMap::from([
                (
                    1,
                    RuleAction::Move {
                        folder: FolderId(5)
                    }
                ),
                (2, RuleAction::Delete)
            ])
        );
        assert_eq!(super::plan(&rules[..1], &messages[2..]), Plan::default());
    }

    #[test]
    fn mail_already_in_place_is_not_moved_again() {
        let here = FolderId(4);
        let to_here = RuleAction::Move { folder: here };
        assert!(already_there(&to_here, here, FolderRole::Other));
        assert!(!already_there(&to_here, FolderId(5), FolderRole::Inbox));
        assert!(already_there(
            &RuleAction::Archive,
            here,
            FolderRole::Archive
        ));
        assert!(!already_there(
            &RuleAction::Archive,
            here,
            FolderRole::Inbox
        ));
        assert!(already_there(&RuleAction::Delete, here, FolderRole::Trash));
        assert!(!already_there(
            &RuleAction::MarkRead,
            here,
            FolderRole::Inbox
        ));
    }
}
