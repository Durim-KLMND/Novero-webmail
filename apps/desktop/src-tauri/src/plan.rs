//! Plans: Novero Mail Free and Novero Mail Pro.
//!
//! Pro is bought once through Lemon Squeezy, which sends a license key by
//! email. A key unlocks Pro for good in every version released within
//! [`UPDATES`] of the purchase; later versions need a new key.
//! The key is activated on this computer through Lemon Squeezy's license API
//! and checked again about once a day; without a connection, Pro keeps
//! working for [`GRACE`] after the last good check. New installs get a
//! [`TRIAL`] of Pro first. Everything else stays on the computer.
//!
//! Builds without a store (`NOVERO_PLANS_STORE_ID`, see docs/plans.md), such
//! as development builds, have everything unlocked, since there is no way to
//! buy Pro in them.

use std::time::Duration;

use mail_core::{AccountId, Error, Result};
use mail_store::Store;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::compose::now;
use crate::state::AppState;

const STORE_ID: Option<&str> = option_env!("NOVERO_PLANS_STORE_ID");
const PRODUCT_ID: Option<&str> = option_env!("NOVERO_PLANS_PRODUCT_ID");
const CHECKOUT_URL: Option<&str> = option_env!("NOVERO_PLANS_CHECKOUT_URL");

/// When this version was built (UTC seconds), set by build.rs.
const RELEASED: Option<&str> = option_env!("NOVERO_RELEASED");

const API: &str = "https://api.lemonsqueezy.com/v1/licenses";

const DAY: i64 = 24 * 60 * 60;
/// How long a new install has Pro before choosing.
pub const TRIAL: i64 = 14 * DAY;
/// How long Pro keeps working when the license can't be checked.
pub const GRACE: i64 = 14 * DAY;
/// How long after the purchase new versions are covered by a key.
pub const UPDATES: i64 = 365 * DAY;
/// How often the license is checked again.
const RECHECK: i64 = DAY;
/// How many mail accounts Free syncs.
pub const FREE_ACCOUNTS: usize = 2;

/// Sent to every window whenever the plan may have changed.
pub const PLAN_CHANGED: &str = "plan-changed";

const KEY: &str = "plan";

/// What only Pro can do. Its name is used in messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    MoreAccounts,
    CalendarSync,
    Rules,
    SendLater,
    Snooze,
    Templates,
    MoreSignatures,
}

impl Feature {
    fn name(self) -> &'static str {
        match self {
            Feature::MoreAccounts => "More than two accounts",
            Feature::CalendarSync => "Calendar sync",
            Feature::Rules => "Rules",
            Feature::SendLater => "Send later",
            Feature::Snooze => "Snooze",
            Feature::Templates => "Templates",
            Feature::MoreSignatures => "More than one signature per account",
        }
    }
}

/// What is kept in the database.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    /// When the trial began (UTC seconds); 0 until the app first looks.
    trial_started: i64,
    license: Option<License>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct License {
    key: String,
    /// This computer's activation, needed to check or release it.
    instance: String,
    /// The plan bought, such as "Yearly".
    #[serde(default)]
    variant: String,
    /// Lemon Squeezy's word for it: active, expired or disabled.
    status: String,
    /// When a subscription's key runs out, as Lemon Squeezy writes it.
    #[serde(default)]
    expires_at: Option<String>,
    /// When Lemon Squeezy last said the key is good (UTC seconds).
    checked: i64,
    /// When the key was bought (UTC seconds); 0 when unknown.
    #[serde(default)]
    purchased: i64,
}

impl License {
    /// The last release date this key covers, when known.
    fn updates_until(&self) -> Option<i64> {
        (self.purchased > 0).then(|| self.purchased + UPDATES)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Free,
    Pro,
}

/// Why the plan is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// A build that can't sell Pro, so has all of it.
    Unlocked,
    License,
    Trial,
    /// The license couldn't be checked for too long.
    Unchecked,
    /// The license was refunded or turned off.
    Ended,
    /// This version came out after the license's year of updates.
    TooNew,
    Free,
}

fn configured() -> bool {
    STORE_ID.is_some_and(|s| !s.trim().is_empty())
}

fn released() -> i64 {
    RELEASED.and_then(|r| r.trim().parse().ok()).unwrap_or(0)
}

fn checkout_url() -> Option<&'static str> {
    CHECKOUT_URL.map(str::trim).filter(|u| !u.is_empty())
}

impl Saved {
    fn load(store: &Store) -> Saved {
        let mut saved: Saved = store
            .setting(KEY)
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if saved.trial_started == 0 {
            saved.trial_started = now();
            let _ = saved.save(store);
        }
        saved
    }

    fn save(&self, store: &Store) -> Result<()> {
        store.set_setting(KEY, &serde_json::to_value(self).expect("plan serializes"))
    }

    fn tier(&self, now: i64, released: i64, configured: bool) -> (Tier, Reason) {
        if !configured {
            return (Tier::Pro, Reason::Unlocked);
        }
        let mut lapsed = None;
        if let Some(license) = &self.license {
            if license.status == "active" {
                if license
                    .updates_until()
                    .is_some_and(|until| released > until)
                {
                    lapsed = Some(Reason::TooNew);
                } else if now - license.checked <= GRACE {
                    return (Tier::Pro, Reason::License);
                } else {
                    lapsed = Some(Reason::Unchecked);
                }
            } else {
                lapsed = Some(Reason::Ended);
            }
        }
        if now < self.trial_started + TRIAL {
            return (Tier::Pro, Reason::Trial);
        }
        (Tier::Free, lapsed.unwrap_or(Reason::Free))
    }
}

/// Whether this copy has Pro right now.
pub fn is_pro(store: &Store) -> bool {
    Saved::load(store).tier(now(), released(), configured()).0 == Tier::Pro
}

/// An error naming the Pro feature, unless this copy has Pro.
pub fn require(store: &Store, feature: Feature) -> Result<()> {
    if is_pro(store) {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{} is part of Novero Mail Pro.",
            feature.name()
        )))
    }
}

/// The accounts Free doesn't sync: all but the first two added.
fn paused(store: &Store) -> Vec<AccountId> {
    if is_pro(store) {
        return Vec::new();
    }
    let mut ids: Vec<AccountId> = store
        .list_accounts()
        .map(|a| a.into_iter().map(|a| a.id).collect())
        .unwrap_or_default();
    ids.sort_by_key(|id| id.0);
    ids.into_iter().skip(FREE_ACCOUNTS).collect()
}

/// Whether an account's mail is synced on this plan.
pub fn syncs(store: &Store, account: AccountId) -> bool {
    !paused(store).contains(&account)
}

/// Adding another account, unless the plan allows it.
pub fn can_add_account(store: &Store) -> Result<()> {
    if store.list_accounts()?.len() < FREE_ACCOUNTS {
        return Ok(());
    }
    require(store, Feature::MoreAccounts)
}

/// The plan as the UI shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo {
    tier: Tier,
    reason: Reason,
    /// Whole days of the trial left, counting today; 0 once it's over.
    trial_days_left: i64,
    license: Option<LicenseInfo>,
    /// Whether Pro can be bought from this copy.
    can_buy: bool,
    free_accounts: usize,
    /// Accounts not synced on Free.
    paused_accounts: Vec<AccountId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseInfo {
    /// The key with all but its last part hidden.
    key: String,
    variant: String,
    status: String,
    expires_at: Option<String>,
    checked: i64,
    /// Versions released until then are covered (UTC seconds).
    updates_until: Option<i64>,
}

fn info(store: &Store) -> PlanInfo {
    let saved = Saved::load(store);
    let now = now();
    let (tier, reason) = saved.tier(now, released(), configured());
    let left = saved.trial_started + TRIAL - now;
    PlanInfo {
        tier,
        reason,
        trial_days_left: if left > 0 { (left + DAY - 1) / DAY } else { 0 },
        license: saved.license.map(|l| LicenseInfo {
            updates_until: l.updates_until(),
            key: masked(&l.key),
            variant: l.variant,
            status: l.status,
            expires_at: l.expires_at,
            checked: l.checked,
        }),
        can_buy: configured() && checkout_url().is_some(),
        free_accounts: FREE_ACCOUNTS,
        paused_accounts: paused(store),
    }
}

fn masked(key: &str) -> String {
    match key.rsplit_once('-') {
        Some((_, last)) => format!("••••-{last}"),
        None => "••••".into(),
    }
}

/// What Lemon Squeezy answers to activate, validate and deactivate.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Answer {
    activated: bool,
    valid: bool,
    error: Option<String>,
    license_key: Option<AnswerKey>,
    instance: Option<AnswerInstance>,
    meta: Option<Meta>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AnswerKey {
    status: String,
    expires_at: Option<String>,
    created_at: Option<String>,
}

impl AnswerKey {
    fn purchased(&self) -> i64 {
        self.created_at
            .as_deref()
            .and_then(|d| d.parse::<jiff::Timestamp>().ok())
            .map_or(0, |t| t.as_second())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AnswerInstance {
    id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Meta {
    store_id: u64,
    product_id: u64,
    variant_name: String,
}

impl Answer {
    /// Whether the key was sold by our store, for our product. Any Lemon
    /// Squeezy shop's keys would pass the license API otherwise.
    fn ours(&self) -> bool {
        let Some(meta) = &self.meta else {
            return false;
        };
        let matches = |want: Option<&str>, got: u64| {
            want.map(str::trim)
                .filter(|w| !w.is_empty())
                .is_none_or(|w| w == got.to_string())
        };
        configured() && matches(STORE_ID, meta.store_id) && matches(PRODUCT_ID, meta.product_id)
    }
}

/// Posts to the license API. Lemon Squeezy answers refusals with an error
/// status and the same JSON, so only an unreadable answer is an error here.
async fn call(action: &str, form: &[(&str, &str)]) -> Result<Answer> {
    let client = mail_auth::https_client(Duration::from_secs(15))?;
    let response = client
        .post(format!("{API}/{action}"))
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| Error::Network(format!("could not reach Lemon Squeezy: {e}")))?;
    response
        .json::<Answer>()
        .await
        .map_err(|e| Error::Network(format!("Lemon Squeezy's answer was unreadable: {e}")))
}

/// The name this computer's activation is listed under in the store.
fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Novero Mail".into())
}

/// Tells the windows, and starts or stops the accounts Free doesn't sync.
fn changed(app: &AppHandle) {
    let _ = app.emit(PLAN_CHANGED, ());
    let paused = paused(&app.state::<AppState>().store.lock().unwrap());
    for id in &paused {
        crate::state::stop_account(app, *id);
    }
    crate::state::start_all(app);
}

#[tauri::command]
pub fn get_plan(state: State<'_, AppState>) -> PlanInfo {
    info(&state.store.lock().unwrap())
}

/// Activates a license key on this computer.
#[tauri::command]
pub async fn activate_license(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<PlanInfo> {
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err(Error::Invalid("Enter your license key.".into()));
    }
    if !configured() {
        return Err(Error::Invalid(
            "This copy of Novero Mail already has every feature.".into(),
        ));
    }
    let answer = call(
        "activate",
        &[("license_key", &key), ("instance_name", &computer_name())],
    )
    .await?;
    if !answer.activated {
        let why = answer.error.unwrap_or_else(|| "it was refused".into());
        return Err(Error::Invalid(format!("That key didn't work: {why}")));
    }
    let instance = answer
        .instance
        .as_ref()
        .map(|i| i.id.clone())
        .unwrap_or_default();
    if !answer.ours() {
        // Give the activation back; the key belongs to something else.
        let _ = call(
            "deactivate",
            &[("license_key", &key), ("instance_id", &instance)],
        )
        .await;
        return Err(Error::Invalid(
            "That key isn't for Novero Mail. Check the email from your purchase.".into(),
        ));
    }
    let key_info = answer.license_key.unwrap_or_default();
    let license = License {
        key,
        instance,
        variant: answer.meta.map(|m| m.variant_name).unwrap_or_default(),
        purchased: key_info.purchased(),
        status: key_info.status,
        expires_at: key_info.expires_at,
        checked: now(),
    };
    {
        let store = state.store.lock().unwrap();
        let mut saved = Saved::load(&store);
        saved.license = Some(license);
        saved.save(&store)?;
    }
    changed(&app);
    Ok(get_plan(state))
}

/// Releases this computer's activation, so the key can be used on another.
#[tauri::command]
pub async fn deactivate_license(app: AppHandle, state: State<'_, AppState>) -> Result<PlanInfo> {
    let license = Saved::load(&state.store.lock().unwrap()).license;
    if let Some(license) = license {
        // Only an unreachable store keeps the key; a refusal means it isn't
        // activated here any more anyway.
        call(
            "deactivate",
            &[
                ("license_key", &license.key),
                ("instance_id", &license.instance),
            ],
        )
        .await?;
        let store = state.store.lock().unwrap();
        let mut saved = Saved::load(&store);
        saved.license = None;
        saved.save(&store)?;
    }
    changed(&app);
    Ok(get_plan(state))
}

/// Checks the license with Lemon Squeezy now.
#[tauri::command]
pub async fn refresh_plan(app: AppHandle, state: State<'_, AppState>) -> Result<PlanInfo> {
    check(&app).await?;
    Ok(get_plan(state))
}

/// Opens the store's checkout in the browser, with the first account's
/// address filled in.
#[tauri::command]
pub fn open_checkout(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    let base = checkout_url().filter(|_| configured()).ok_or_else(|| {
        Error::Invalid("Pro can't be bought from this copy of Novero Mail.".into())
    })?;
    let email = state
        .store
        .lock()
        .unwrap()
        .list_accounts()?
        .into_iter()
        .next()
        .map(|a| a.email);
    let mut url = tauri::Url::parse(base).map_err(|e| Error::Invalid(e.to_string()))?;
    if let Some(email) = email {
        url.query_pairs_mut().append_pair("checkout[email]", &email);
    }
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|e| Error::Invalid(format!("could not open the browser: {e}")))
}

/// Asks Lemon Squeezy whether the saved key is still good.
async fn check(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let Some(license) = Saved::load(&state.store.lock().unwrap()).license else {
        return Ok(());
    };
    let answer = call(
        "validate",
        &[
            ("license_key", &license.key),
            ("instance_id", &license.instance),
        ],
    )
    .await?;
    let key = answer.license_key.as_ref();
    let updated = if answer.valid && answer.ours() {
        License {
            status: key.map(|k| k.status.clone()).unwrap_or_default(),
            expires_at: key.and_then(|k| k.expires_at.clone()),
            checked: now(),
            ..license
        }
    } else {
        // Expired, turned off, or no longer activated on this computer.
        License {
            status: key
                .map(|k| k.status.clone())
                .filter(|s| s != "active")
                .unwrap_or_else(|| "disabled".into()),
            expires_at: key.and_then(|k| k.expires_at.clone()),
            ..license
        }
    };
    {
        let store = state.store.lock().unwrap();
        let mut saved = Saved::load(&store);
        if saved.license.as_ref().is_some_and(|l| l.key == updated.key) {
            saved.license = Some(updated);
            saved.save(&store)?;
        }
    }
    changed(app);
    Ok(())
}

/// Checks the license shortly after start-up and then about once a day.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let due = {
                let state = app.state::<AppState>();
                let saved = Saved::load(&state.store.lock().unwrap());
                saved.license.is_some_and(|l| now() - l.checked >= RECHECK)
            };
            if due {
                // Offline: tried again next hour, within the grace period.
                let _ = check(&app).await;
            }
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn license(status: &str, checked: i64) -> Option<License> {
        Some(License {
            key: "ABC-DEF-123".into(),
            instance: "i".into(),
            variant: "Yearly".into(),
            status: status.into(),
            expires_at: None,
            checked,
            purchased: 0,
        })
    }

    #[test]
    fn unconfigured_builds_have_everything() {
        let saved = Saved {
            trial_started: 1,
            license: None,
        };
        assert_eq!(
            saved.tier(100 * DAY, 0, false),
            (Tier::Pro, Reason::Unlocked)
        );
    }

    #[test]
    fn the_trial_ends_after_two_weeks() {
        let start = 1_000 * DAY;
        let saved = Saved {
            trial_started: start,
            license: None,
        };
        assert_eq!(
            saved.tier(start + TRIAL - 1, start, true),
            (Tier::Pro, Reason::Trial)
        );
        assert_eq!(
            saved.tier(start + TRIAL, start, true),
            (Tier::Free, Reason::Free)
        );
    }

    #[test]
    fn a_license_works_offline_for_a_while() {
        let start = 1_000 * DAY;
        let checked = start + 30 * DAY;
        let saved = Saved {
            trial_started: start,
            license: license("active", checked),
        };
        assert_eq!(
            saved.tier(checked + GRACE, start, true),
            (Tier::Pro, Reason::License)
        );
        assert_eq!(
            saved.tier(checked + GRACE + 1, start, true),
            (Tier::Free, Reason::Unchecked)
        );
    }

    #[test]
    fn an_ended_license_falls_back_to_the_trial_or_free() {
        let start = 1_000 * DAY;
        let saved = Saved {
            trial_started: start,
            license: license("expired", start),
        };
        assert_eq!(
            saved.tier(start + DAY, start, true),
            (Tier::Pro, Reason::Trial)
        );
        assert_eq!(
            saved.tier(start + TRIAL, start, true),
            (Tier::Free, Reason::Ended)
        );
    }

    #[test]
    fn a_license_covers_a_year_of_releases() {
        let start = 1_000 * DAY;
        let mut l = license("active", start + 400 * DAY).unwrap();
        l.purchased = start;
        let saved = Saved {
            trial_started: start,
            license: Some(l),
        };
        let now = start + 400 * DAY;
        assert_eq!(
            saved.tier(now, start + UPDATES, true),
            (Tier::Pro, Reason::License)
        );
        assert_eq!(
            saved.tier(now, start + UPDATES + 1, true),
            (Tier::Free, Reason::TooNew)
        );
    }

    #[test]
    fn keys_are_shown_masked() {
        assert_eq!(
            masked("38b1460a-5104-4067-a91d-77b872934d51"),
            "••••-77b872934d51"
        );
        assert_eq!(masked("nodashes"), "••••");
    }

    #[test]
    fn lemon_squeezy_answers_are_read() {
        let answer: Answer = serde_json::from_str(
            r#"{"activated":true,"error":null,
                "license_key":{"id":1,"status":"active","key":"k","activation_limit":3,"expires_at":null,"created_at":"2026-10-22T08:00:00.000000Z"},
                "instance":{"id":"47596ad9-a811-4ebf-ac8a-03fc7b6d2a17","name":"PC"},
                "meta":{"store_id":12,"order_id":2,"product_id":34,"product_name":"Novero Mail Pro","variant_id":5,"variant_name":"Yearly","customer_email":"a@b.c"}}"#,
        )
        .unwrap();
        assert!(answer.activated);
        assert_eq!(
            answer.instance.unwrap().id,
            "47596ad9-a811-4ebf-ac8a-03fc7b6d2a17"
        );
        let key = answer.license_key.unwrap();
        assert_eq!(key.status, "active");
        assert_eq!(key.purchased(), 1_792_656_000);
        let meta = answer.meta.unwrap();
        assert_eq!((meta.store_id, meta.product_id), (12, 34));
        let refused: Answer =
            serde_json::from_str(r#"{"activated":false,"error":"license_key not found."}"#)
                .unwrap();
        assert!(!refused.activated && !refused.ours());
    }
}
