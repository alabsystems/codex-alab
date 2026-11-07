// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Redeem earned resets for the original in-flight request, never purchased credits.
use codex_backend_client::Client;
use codex_backend_client::ConsumeRateLimitResetCreditCode;
use codex_backend_client::RateLimitsWithResetCredits;
use codex_http_client::HttpClientFactory;
use codex_login::AuthManager;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

const PREFLIGHT_USED_PERCENT: f64 = 95.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Recover,
    Preflight,
}

fn preflight_cycle(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
) -> Option<i64> {
    let threshold = if read.ordinary_usage_allowed == Some(false) {
        100.0
    } else {
        PREFLIGHT_USED_PERCENT
    };
    pressure_cycle(read, owner, now, threshold)
}

fn pressure_cycle(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
    threshold: f64,
) -> Option<i64> {
    if !matches(read, owner) || explicit_denial(read) || read.ordinary_usage_allowed.is_none() {
        return None;
    }
    read.rate_limits
        .iter()
        .filter(|s| s.limit_id.as_deref().is_none_or(|id| id == "codex"))
        .flat_map(|s| [s.primary.as_ref(), s.secondary.as_ref()])
        .flatten()
        .filter(|w| w.used_percent.is_finite() && w.used_percent >= threshold)
        .filter_map(|w| {
            if !matches!(w.window_minutes, Some(300 | 10080)) {
                return None;
            }
            let reset = w.resets_at?;
            (reset.saturating_sub(now) >= 5 * 60).then_some(reset)
        })
        .max()
}

fn permitted_after_preflight(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
) -> bool {
    usage_below(read, owner, PREFLIGHT_USED_PERCENT)
}

fn usage_below(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    threshold: f64,
) -> bool {
    if !matches(read, owner) || explicit_denial(read) || read.ordinary_usage_allowed != Some(true) {
        return false;
    }
    let mut windows = read
        .rate_limits
        .iter()
        .filter(|s| s.limit_id.as_deref().is_none_or(|id| id == "codex"))
        .flat_map(|s| [s.primary.as_ref(), s.secondary.as_ref()])
        .flatten()
        .filter(|w| matches!(w.window_minutes, Some(300 | 10080)))
        .peekable();
    windows.peek().is_some()
        && windows.all(|w| {
            w.used_percent.is_finite() && w.used_percent >= 0.0 && w.used_percent < threshold
        })
}

fn identity(manager: &AuthManager) -> Option<(String, String, u64)> {
    let auth = manager.auth_cached()?;
    if !auth.uses_codex_backend() {
        return None;
    }
    Some((
        auth.get_account_id()?,
        auth.get_chatgpt_user_id()?,
        manager.auth_change_state_for(&auth)?.owner_generation,
    ))
}

fn matches(read: &RateLimitsWithResetCredits, owner: &(String, String, u64)) -> bool {
    read.account_id.as_ref() == Some(&owner.0) && read.user_id.as_ref() == Some(&owner.1)
}

fn explicit_denial(read: &RateLimitsWithResetCredits) -> bool {
    use codex_protocol::protocol::RateLimitReachedType;
    read.rate_limits
        .iter()
        .filter(|snapshot| snapshot.limit_id.as_deref().is_none_or(|id| id == "codex"))
        .any(|snapshot| {
            snapshot.spend_control_reached == Some(true)
                || snapshot
                    .rate_limit_reached_type
                    .is_some_and(|reason| reason != RateLimitReachedType::RateLimitReached)
        })
}

fn eligible(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
) -> Option<i64> {
    if read.ordinary_usage_allowed != Some(false) {
        return None;
    }
    pressure_cycle(read, owner, now, 100.0)
}

fn restored(read: &RateLimitsWithResetCredits, owner: &(String, String, u64)) -> bool {
    usage_below(read, owner, 100.0)
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[derive(Default)]
pub struct State {
    owner: Option<(String, String, u64)>,
}
struct Settings {
    home: PathBuf,
    base: String,
    factory: HttpClientFactory,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Pending,
    Consumed,
    NothingToReset,
    NoCredit,
}
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Operation {
    credit: String,
    cycle: i64,
    key: String,
    generation: u64,
    outcome: Outcome,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Ledger {
    version: u8,
    next_generation: u64,
    current: Option<Operation>,
}
impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            next_generation: 1,
            current: None,
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Credit {
    id: String,
    consumed: bool,
    retry_released: bool,
}

pub async fn recover_weekly(
    manager: &AuthManager,
    home: &Path,
    base: &str,
    factory: HttpClientFactory,
    turn_id: &str,
    state: &State,
) -> bool {
    if !codex_alab_core::active() {
        return false;
    }
    let Some(owner) = state
        .owner
        .clone()
        .filter(|owner| identity(manager).as_ref() == Some(owner))
    else {
        return false;
    };
    let config = Settings {
        home: home.to_owned(),
        base: base.to_owned(),
        factory,
    };
    tokio::time::timeout(
        Duration::from_secs(45),
        recover(manager, &config, &owner, turn_id, Purpose::Recover),
    )
    .await
    .unwrap_or(false)
}

/// Check the authenticated allowance before sampling, independently of model text.
/// The existing account-bound ledger owns every redemption and uncertain retry.
pub async fn preflight(
    manager: &AuthManager,
    home: &Path,
    base: &str,
    factory: HttpClientFactory,
    turn_id: &str,
    state: &State,
) {
    if !codex_alab_core::active() {
        return;
    }
    let Some(owner) = state
        .owner
        .clone()
        .filter(|owner| identity(manager).as_ref() == Some(owner))
    else {
        return;
    };
    let config = Settings {
        home: home.to_owned(),
        base: base.to_owned(),
        factory,
    };
    let _ = tokio::time::timeout(
        Duration::from_secs(45),
        recover(manager, &config, &owner, turn_id, Purpose::Preflight),
    )
    .await;
}

async fn recover(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    purpose: Purpose,
) -> bool {
    let current = || codex_alab_core::active() && identity(manager).as_ref() == Some(owner);
    if !current() {
        return false;
    }
    let Some(auth) = manager.auth().await else {
        return false;
    };
    let client = Client::from_auth(config.base.clone(), &auth, config.factory.clone());
    let account = digest(&format!("{}\0{}\0{}", config.base, owner.0, owner.1));
    let lock = config.home.join(format!("weekly-reset-{account}"));
    let Ok(Some(_guard)) =
        codex_alab_core::auth_lock::AuthLock::take(&lock, Duration::from_secs(5)).await
    else {
        return false;
    };
    let dir = config.home.join(format!("weekly-reset-{account}.d"));
    if private_dir(&dir).is_err() {
        return false;
    }
    let state_path = dir.join("state.json");
    let mut ledger = match load::<Ledger>(&state_path) {
        Ok(Some(state)) if state.version == 1 => state,
        Ok(None) => Ledger::default(),
        _ => return false,
    };
    if let Some(op) = ledger.current.as_ref()
        && (!valid_id(&op.credit)
            || op.generation == 0
            || op.generation >= ledger.next_generation
            || op.key != operation_key(&account, op.cycle, &op.credit, op.generation))
    {
        return false;
    }
    if let Some(op) = ledger.current.as_ref() {
        let Some(record) = credit_at(&dir, &op.credit) else {
            return false;
        };
        if op.outcome == Outcome::Consumed && !record.consumed {
            return false;
        }
    }
    let Ok(mut read) = client.get_rate_limits_with_reset_credits().await else {
        return false;
    };
    if !current() {
        return false;
    }
    if purpose == Purpose::Preflight && permitted_after_preflight(&read, owner) {
        if ledger
            .current
            .as_ref()
            .is_some_and(|op| op.outcome == Outcome::Consumed)
        {
            let _ = release(
                &dir,
                ledger.current.as_ref().map(|op| op.credit.as_str()),
                turn_id,
            );
        }
        return false;
    }
    if purpose == Purpose::Recover && restored(&read, owner) {
        return release(
            &dir,
            ledger.current.as_ref().map(|op| op.credit.as_str()),
            turn_id,
        );
    }
    let mut tried = std::collections::BTreeSet::new();
    loop {
        if purpose == Purpose::Preflight
            && ledger.current.as_ref().is_some_and(|op| {
                op.outcome == Outcome::Consumed
                    && credit_at(&dir, &op.credit).is_none_or(|credit| !credit.retry_released)
            })
        {
            return false;
        }
        let now = chrono::Utc::now().timestamp();
        let cycle = match purpose {
            Purpose::Recover => eligible(&read, owner, now),
            Purpose::Preflight => preflight_cycle(&read, owner, now),
        };
        let Some(cycle) = cycle else {
            return false;
        };
        let pending = ledger
            .current
            .as_ref()
            .filter(|op| op.outcome == Outcome::Pending)
            .cloned();
        let op = if let Some(op) = pending {
            // A committed per-credit proof can survive a crash before state.json.
            let Some(credit) = credit_at(&dir, &op.credit) else {
                return false;
            };
            if credit.consumed {
                let Some(current_op) = ledger.current.as_mut() else {
                    return false;
                };
                current_op.outcome = Outcome::Consumed;
                if save(&state_path, &ledger).is_none() {
                    return false;
                }
                continue;
            }
            op
        } else {
            let Some(id) = available(&client, &dir, &tried, now).await else {
                return false;
            };
            let Some(op) = begin(&dir, &account, &mut ledger, cycle, &id) else {
                return false;
            };
            op
        };
        if !current() {
            return false;
        }
        tried.insert(op.credit.clone());
        let reply = client
            .consume_rate_limit_reset_credit_by_id(&op.key, &op.credit)
            .await;
        let code = reply.ok().map(|reply| reply.code);
        let outcome = match code {
            Some(
                ConsumeRateLimitResetCreditCode::Reset
                | ConsumeRateLimitResetCreditCode::AlreadyRedeemed,
            ) => Outcome::Consumed,
            Some(ConsumeRateLimitResetCreditCode::NothingToReset) => Outcome::NothingToReset,
            Some(ConsumeRateLimitResetCreditCode::NoCredit) => Outcome::NoCredit,
            None => Outcome::Pending,
        };
        if !current() {
            return false;
        }
        if outcome == Outcome::Consumed {
            let Some(mut credit) = credit_at(&dir, &op.credit) else {
                return false;
            };
            credit.consumed = true;
            if save(&credit_path(&dir, &op.credit), &credit).is_none() {
                return false;
            }
        }
        let Some(current_op) = ledger.current.as_mut() else {
            return false;
        };
        current_op.outcome = outcome;
        if save(&state_path, &ledger).is_none() {
            return false;
        }
        let Ok(fresh) = client.get_rate_limits_with_reset_credits().await else {
            return false;
        };
        if !current() {
            return false;
        }
        let restored = match purpose {
            Purpose::Recover => restored(&fresh, owner),
            Purpose::Preflight => permitted_after_preflight(&fresh, owner),
        };
        if restored {
            return release(&dir, Some(&op.credit), turn_id);
        }
        // Unknown POST custody must be reconciled with this same immutable key.
        // A newly applied reset that has not restored usage cannot burn another.
        if outcome == Outcome::Pending
            || code == Some(ConsumeRateLimitResetCreditCode::Reset)
            || (purpose == Purpose::Preflight && outcome == Outcome::Consumed)
        {
            return false;
        }
        read = fresh;
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
}
fn operation_key(account: &str, cycle: i64, credit: &str, generation: u64) -> String {
    digest(&format!(
        "alab/weekly-reset/v3/{account}/{cycle}/{credit}/{generation}"
    ))
}
fn credit_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("credit-{}.json", digest(id)))
}
fn credit_at(dir: &Path, id: &str) -> Option<Credit> {
    let credit = load::<Credit>(&credit_path(dir, id)).ok()??;
    (credit.id == id && (!credit.retry_released || credit.consumed)).then_some(credit)
}
fn begin(
    dir: &Path,
    account: &str,
    ledger: &mut Ledger,
    cycle: i64,
    id: &str,
) -> Option<Operation> {
    if !valid_id(id) {
        return None;
    }
    let path = credit_path(dir, id);
    match load::<Credit>(&path).ok()? {
        Some(credit) if credit.id == id && !credit.consumed && !credit.retry_released => {}
        Some(_) => return None,
        None => save(
            &path,
            &Credit {
                id: id.into(),
                consumed: false,
                retry_released: false,
            },
        )?,
    }
    let generation = ledger.next_generation;
    let next = generation.checked_add(1)?;
    let op = Operation {
        credit: id.into(),
        cycle,
        key: operation_key(account, cycle, id, generation),
        generation,
        outcome: Outcome::Pending,
    };
    ledger.next_generation = next;
    ledger.current = Some(op.clone());
    save(&dir.join("state.json"), ledger)?;
    Some(op)
}

async fn available(
    client: &Client,
    dir: &Path,
    tried: &std::collections::BTreeSet<String>,
    now: i64,
) -> Option<String> {
    let details = client.list_rate_limit_reset_credits().await.ok()?;
    if details.available_count <= 0 {
        return None;
    }
    details
        .credits
        .iter()
        .filter(|c| {
            valid_id(&c.id)
                && c.status == "available"
                && c.reset_type == "codex_rate_limits"
                && !tried.contains(&c.id)
        })
        .filter(|c| {
            c.expires_at.as_deref().is_none_or(|s| {
                chrono::DateTime::parse_from_rfc3339(s).is_ok_and(|t| t.timestamp() > now)
            })
        })
        .filter(|c| {
            matches!(load::<Credit>(&credit_path(dir, &c.id)), Ok(None))
                || credit_at(dir, &c.id).is_some_and(|record| !record.consumed)
        })
        .min_by_key(|c| {
            c.expires_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map_or(i64::MAX, |t| t.timestamp())
        })
        .map(|c| c.id.clone())
}

fn release(dir: &Path, id: Option<&str>, turn: &str) -> bool {
    let mut record = if let Some(id) = id {
        let Some(record) = credit_at(dir, id) else {
            return false;
        };
        Some(record)
    } else {
        None
    };
    if let Some(credit) = record.as_mut()
        && credit.consumed
        && !credit.retry_released
    {
        // Record external custody first. A later write failure must leave the
        // positive consumed-credit retry available for reconciliation.
        if external_marker(dir, turn, false).is_none() {
            return false;
        }
        credit.retry_released = true;
        return save(&credit_path(dir, &credit.id), credit).is_some();
    }
    external_marker(dir, turn, true).is_some()
}
fn external_marker(dir: &Path, turn: &str, only_new: bool) -> Option<()> {
    let path = dir.join(format!("turn-{}.external", digest(turn)));
    use std::os::unix::fs::OpenOptionsExt;
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path);
    match result {
        Ok(file) => {
            file.sync_all().ok()?;
            std::fs::File::open(dir).ok()?.sync_all().ok()?;
            Some(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && !only_new => {
            std::fs::symlink_metadata(path)
                .ok()?
                .file_type()
                .is_file()
                .then_some(())
        }
        Err(_) => None,
    }
}
fn private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::fs::PermissionsExt;
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_dir() || meta.permissions().mode() & 0o077 != 0 {
        return Err(std::io::Error::other("reset directory is not private"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("reset directory has no parent"))?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> std::io::Result<Option<T>> {
    read_receipt(path)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(std::io::Error::other))
        .transpose()
}
fn save<T: serde::Serialize>(path: &Path, value: &T) -> Option<()> {
    write_bytes(path, &serde_json::to_vec(value).ok()?)
}

fn read_receipt(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = match options.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("reset receipt is not regular"));
    }
    let mut bytes = Vec::new();
    file.by_ref().take(8193).read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(std::io::Error::other("reset receipt too large"));
    }
    Ok(Some(bytes))
}

/// Premium and spend-control walls cannot consume an ordinary allowance reset.
pub fn ordinary_weekly_wall(details: &codex_protocol::error::UsageLimitReachedError) -> bool {
    use codex_protocol::protocol::RateLimitReachedType;
    details
        .rate_limit_reached_type
        .is_none_or(|kind| kind == RateLimitReachedType::RateLimitReached)
        && details.rate_limits.as_ref().is_none_or(|snapshot| {
            snapshot.limit_id.as_deref().is_none_or(|id| id == "codex")
                && snapshot.spend_control_reached != Some(true)
                && snapshot
                    .rate_limit_reached_type
                    .is_none_or(|kind| kind == RateLimitReachedType::RateLimitReached)
        })
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Option<()> {
    if bytes.len() > 8192 {
        return None;
    }
    let temp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    use std::io::Write;
    let mut file = options.open(&temp).ok()?;
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        let _ = std::fs::remove_file(&temp);
        return None;
    }
    if std::fs::rename(&temp, path).is_err() {
        let _ = std::fs::remove_file(&temp);
        return None;
    }
    std::fs::File::open(path.parent()?).ok()?.sync_all().ok()?;
    Some(())
}

impl State {
    pub async fn bind(manager: &AuthManager) -> Self {
        let _ = manager.auth().await;
        Self {
            owner: identity(manager),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reading(used: f64, minutes: i64, reset: i64) -> RateLimitsWithResetCredits {
        let snapshot = serde_json::from_value(serde_json::json!({
            "limit_id":"codex", "primary":null,
            "secondary":{"used_percent":used,"window_minutes":minutes,"resets_at":reset}
        }))
        .unwrap();
        RateLimitsWithResetCredits {
            rate_limits: vec![snapshot],
            ordinary_usage_allowed: Some(false),
            rate_limit_reset_credits: None,
            account_id: Some("account".into()),
            user_id: Some("user".into()),
            rate_limit_upsell: None,
        }
    }
    #[test]
    fn exact_five_minute_boundary_and_fail_closed_inputs() {
        let owner = ("account".into(), "user".into(), 1);
        assert_eq!(
            eligible(&reading(100.0, 10080, 1300), &owner, 1000),
            Some(1300)
        );
        for read in [reading(100.0, 10080, 1299), reading(99.9, 10080, 1300)] {
            assert_eq!(eligible(&read, &owner, 1000), None);
        }
        let mut read = reading(100.0, 10080, 1300);
        read.user_id = Some("other".into());
        assert_eq!(eligible(&read, &owner, 1000), None);
        read.user_id = Some("user".into());
        read.ordinary_usage_allowed = None;
        assert_eq!(eligible(&read, &owner, 1000), None);
        read.ordinary_usage_allowed = Some(true);
        assert_eq!(eligible(&read, &owner, 1000), None);
    }
    #[test]
    fn fresh_explicit_spend_denial_blocks_redemption_and_restoration() {
        use codex_protocol::protocol::RateLimitReachedType;
        let owner = ("account".into(), "user".into(), 1);
        for reason in [
            None,
            Some(RateLimitReachedType::WorkspaceOwnerCreditsDepleted),
            Some(RateLimitReachedType::WorkspaceMemberUsageLimitReached),
        ] {
            let mut read = reading(100.0, 10080, 44200);
            read.rate_limits[0].rate_limit_reached_type = reason;
            read.rate_limits[0].spend_control_reached = reason.is_none().then_some(true);
            assert_eq!(eligible(&read, &owner, 1000), None);
            read.ordinary_usage_allowed = Some(true);
            read.rate_limits[0].secondary.as_mut().unwrap().used_percent = 0.0;
            assert!(!restored(&read, &owner));
        }
    }

    #[test]
    fn preflight_preserves_headroom_in_both_subscription_windows() {
        let owner = ("account".into(), "user".into(), 1);
        for (minutes, horizon) in [(300, 300), (10080, 300)] {
            let mut read = reading(95.0, minutes, 1000 + horizon);
            read.ordinary_usage_allowed = Some(true);
            assert_eq!(preflight_cycle(&read, &owner, 1000), Some(1000 + horizon));
            assert!(!permitted_after_preflight(&read, &owner));
            read.rate_limits[0].secondary.as_mut().unwrap().used_percent = 94.9;
            assert_eq!(preflight_cycle(&read, &owner, 1000), None);
            assert!(permitted_after_preflight(&read, &owner));
            read.rate_limits[0].secondary.as_mut().unwrap().used_percent = 100.0;
            read.ordinary_usage_allowed = Some(false);
            assert_eq!(eligible(&read, &owner, 1000), Some(1000 + horizon));
            read.rate_limits[0].secondary.as_mut().unwrap().resets_at = Some(999 + horizon);
            assert_eq!(preflight_cycle(&read, &owner, 1000), None);
            read.ordinary_usage_allowed = None;
            assert_eq!(preflight_cycle(&read, &owner, 1000), None);
        }
    }

    #[tokio::test]
    async fn preflight_redeems_before_denial_and_does_not_repeat_the_request() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;
        use wiremock::Mock;
        use wiremock::MockServer;
        use wiremock::ResponseTemplate;
        use wiremock::matchers::method;
        use wiremock::matchers::path;
        codex_alab_core::activate();
        let server = MockServer::start().await;
        let redeemed = Arc::new(AtomicBool::new(false));
        let read_redeemed = Arc::clone(&redeemed);
        let reset = chrono::Utc::now().timestamp() + 3600;
        Mock::given(method("GET")).and(path("/api/codex/usage"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "account_id":"account", "user_id":"user", "plan_type":"plus",
                    "rate_limit":{"allowed":true, "limit_reached":false,
                        "primary_window":{"used_percent":if read_redeemed.load(Ordering::SeqCst) {0}else{95},
                        "limit_window_seconds":18000,"reset_at":reset,"reset_after_seconds":3600}}
                }))
            }).expect(3).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/codex/rate-limit-reset-credits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "available_count":1,"credits":[{"id":"A","reset_type":"codex_rate_limits",
                "status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-01-01T00:00:00Z"}]
            }))).expect(1).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/codex/rate-limit-reset-credits/consume"))
            .respond_with(move |_: &wiremock::Request| {
                redeemed.store(true, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let home = tempfile::tempdir().unwrap();
        let auth = codex_login::CodexAuth::from_external_chatgpt_tokens("e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture", "account", Some("plus")).unwrap();
        let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
        let state = State::bind(&manager).await;
        for _ in 0..2 {
            preflight(
                &manager,
                home.path(),
                &server.uri(),
                HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
                "turn",
                &state,
            )
            .await;
        }
        let account = digest(&format!("{}\0account\0user", server.uri()));
        let dir = home.path().join(format!("weekly-reset-{account}.d"));
        let credit = credit_at(&dir, "A").unwrap();
        assert!(credit.consumed && credit.retry_released);
    }
    #[test]
    fn external_restoration_does_not_spend_a_pending_credit_or_its_positive_retry() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::default();
        let op = begin(dir.path(), "account", &mut ledger, 44200, "A").unwrap();
        assert!(release(dir.path(), Some("A"), "original-turn"));
        assert!(!release(dir.path(), Some("A"), "original-turn"));
        let mut credit = credit_at(dir.path(), "A").unwrap();
        assert!(!credit.consumed);
        let saved = load::<Ledger>(&dir.path().join("state.json"))
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.current.unwrap().key,
            op.key,
            "pending POST key survives healthy external recovery"
        );
        credit.consumed = true;
        save(&credit_path(dir.path(), "A"), &credit).unwrap();
        assert!(
            release(dir.path(), Some("A"), "original-turn"),
            "actual consumption grants its independent retry"
        );
        assert!(!release(dir.path(), Some("A"), "original-turn"));
    }
    #[test]
    fn definitive_nonconsumption_can_start_another_operation_without_releasing_more_external_retries()
     {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::default();
        let first = begin(dir.path(), "account", &mut ledger, 44200, "A").unwrap();
        assert!(release(dir.path(), Some("A"), "original-turn"));
        ledger.current.as_mut().unwrap().outcome = Outcome::NothingToReset;
        save(&dir.path().join("state.json"), &ledger).unwrap();
        let second = begin(dir.path(), "account", &mut ledger, 44200, "A").unwrap();
        assert_ne!(first.key, second.key);
        assert!(!credit_at(dir.path(), "A").unwrap().consumed);
        assert!(!release(dir.path(), Some("A"), "original-turn"));
    }
    #[cfg(unix)]
    #[test]
    fn linked_record_and_nonprivate_directory_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let path = dir.path().join("record");
        std::fs::write(&target, "not a record").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(read_receipt(&path).is_err());
        assert!(private_dir(&path).is_err());
    }
    #[tokio::test]
    async fn definite_no_credit_does_not_block_another_earned_credit() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;
        use wiremock::Mock;
        use wiremock::MockServer;
        use wiremock::ResponseTemplate;
        use wiremock::matchers::body_partial_json;
        use wiremock::matchers::method;
        use wiremock::matchers::path;
        codex_alab_core::activate();
        let server = MockServer::start().await;
        let restored_flag = Arc::new(AtomicBool::new(false));
        let cycle = chrono::Utc::now().timestamp() + 13 * 3600;
        let observed = Arc::clone(&restored_flag);
        Mock::given(method("GET")).and(path("/api/codex/usage"))
            .respond_with(move |_:&wiremock::Request| {
                let restored=observed.load(Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "account_id":"account","user_id":"user","plan_type":"plus",
                    "rate_limit":{"allowed":restored,"limit_reached":!restored,
                        "secondary_window":{"used_percent":if restored {0}else{100},"limit_window_seconds":604800,"reset_at":cycle,"reset_after_seconds":46800}}
                }))
            }).expect(3).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/codex/rate-limit-reset-credits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"available_count":2,"credits":[
                {"id":"A","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-01-01T00:00:00Z"},
                {"id":"B","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-02-01T00:00:00Z"}
            ]}))).expect(2).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/codex/rate-limit-reset-credits/consume"))
            .and(body_partial_json(serde_json::json!({"credit_id":"A"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":"no_credit","windows_reset":0})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let redeemed = Arc::clone(&restored_flag);
        Mock::given(method("POST"))
            .and(path("/api/codex/rate-limit-reset-credits/consume"))
            .and(body_partial_json(serde_json::json!({"credit_id":"B"})))
            .respond_with(move |_: &wiremock::Request| {
                redeemed.store(true, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let home = tempfile::tempdir().unwrap();
        let auth=codex_login::CodexAuth::from_external_chatgpt_tokens("e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture","account",Some("plus")).unwrap();
        let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
        let state = State::bind(&manager).await;
        assert!(
            state.owner.is_some(),
            "fixture supplies actual account/user identity"
        );
        assert!(
            recover_weekly(
                &manager,
                home.path(),
                &server.uri(),
                HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
                "original-turn",
                &state
            )
            .await
        );
        let account = digest(&format!("{}\0account\0user", server.uri()));
        let dir = home.path().join(format!("weekly-reset-{account}.d"));
        assert!(!credit_at(&dir, "A").unwrap().consumed);
        assert!(credit_at(&dir, "B").unwrap().consumed);
    }
    #[tokio::test]
    async fn backend_selection_retains_all_consumed_ids_and_distinct_operation_keys() {
        use wiremock::Mock;
        use wiremock::MockServer;
        use wiremock::ResponseTemplate;
        use wiremock::matchers::method;
        use wiremock::matchers::path;
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/codex/rate-limit-reset-credits"))
          .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"available_count":3,"credits":[
            {"id":"B","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-01-01T01:00:00+00:00"},
            {"id":"A","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-01-01T02:00:00+02:00"},
            {"id":"unknown","reset_type":"unknown","status":"available","granted_at":"2026-01-01T00:00:00Z"}
          ]}))).expect(3).mount(&server).await;
        let client = Client::new(
            server.uri(),
            HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
        );
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::default();
        let tried = std::collections::BTreeSet::new();
        let first = available(&client, dir.path(), &tried, 1000).await.unwrap();
        assert_eq!(first, "A", "parsed expiry orders offsets correctly");
        let a = begin(dir.path(), "account", &mut ledger, 44200, &first).unwrap();
        assert_eq!(a.key, operation_key("account", 44200, "A", 1));
        let mut record = credit_at(dir.path(), "A").unwrap();
        record.consumed = true;
        save(&credit_path(dir.path(), "A"), &record).unwrap();
        let second = available(&client, dir.path(), &tried, 1000).await.unwrap();
        assert_eq!(second, "B");
        let b = begin(dir.path(), "account", &mut ledger, 44200, &second).unwrap();
        assert_ne!(a.key, b.key);
        let mut record = credit_at(dir.path(), "B").unwrap();
        record.consumed = true;
        save(&credit_path(dir.path(), "B"), &record).unwrap();
        assert!(
            available(&client, dir.path(), &tried, 1000).await.is_none(),
            "stale A/B lists never forget either consumed credit"
        );
    }
}
