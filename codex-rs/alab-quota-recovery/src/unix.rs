// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Redeem earned resets for the original in-flight request, never purchased credits.
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
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
/// How long one point of headroom below the redemption threshold excuses the
/// next authoritative read. A reused reading can miss the threshold only if
/// the whole account burns faster than `1 / PREFLIGHT_SECONDS_PER_POINT`
/// points a second after it was taken (alab/QUOTA-RECOVERY.md).
const PREFLIGHT_SECONDS_PER_POINT: f64 = 4.0;
/// No reading is reused for longer than this, whatever its headroom.
const PREFLIGHT_CLEAR_MAX_SECONDS: f64 = 60.0;

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
    pressure_cycle(read, owner, now, threshold, 5 * 60)
}

fn pressure_cycle(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
    threshold: f64,
    minimum_wait: i64,
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
            (reset.saturating_sub(now) >= minimum_wait).then_some(reset)
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

/// A preflight read that permitted the request, kept for the turn that made it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Clear {
    /// Wall-clock seconds, taken just before the usage request was sent.
    read_at: i64,
    /// The same moment on the monotonic clock, which no clock step moves.
    read: std::time::Instant,
    /// The highest ordinary window that reading reported.
    used: f64,
}

impl Clear {
    /// Whether this reading still excuses an authoritative read at `now`, the
    /// wall clock's second. `observed` is what a later model response
    /// reported: it can only raise the usage this is judged against, never
    /// lower it.
    fn holds(self, now: i64, observed: Option<f64>) -> bool {
        self.holds_after(now, self.read.elapsed(), observed)
    }

    /// The reading's age must be inside the window on both clocks. The wall
    /// clock counts host sleep, which the monotonic clock may not; `elapsed`,
    /// the monotonic age, is what a wall clock stepped back cannot shorten. A
    /// wall clock behind `read_at` proves nothing and forces a read.
    fn holds_after(self, now: i64, elapsed: Duration, observed: Option<f64>) -> bool {
        // `f64::max` would drop an unreadable value instead of honoring it.
        if !self.used.is_finite() || observed.is_some_and(|observed| !observed.is_finite()) {
            return false;
        }
        let used = observed.map_or(self.used, |observed| observed.max(self.used));
        let Some(age) = now.checked_sub(self.read_at).filter(|age| *age >= 0) else {
            return false;
        };
        let good_for = ((PREFLIGHT_USED_PERCENT - used) * PREFLIGHT_SECONDS_PER_POINT)
            .min(PREFLIGHT_CLEAR_MAX_SECONDS);
        (age as f64) < good_for && elapsed.as_secs_f64() < good_for
    }
}

/// The highest ordinary window of an authoritative reading, over the windows
/// `usage_below` judges.
fn ordinary_used(read: &RateLimitsWithResetCredits) -> Option<f64> {
    read.rate_limits
        .iter()
        .filter(|s| s.limit_id.as_deref().is_none_or(|id| id == "codex"))
        .flat_map(|s| [s.primary.as_ref(), s.secondary.as_ref()])
        .flatten()
        .filter(|w| matches!(w.window_minutes, Some(300 | 10080)))
        .map(|w| w.used_percent)
        .reduce(f64::max)
}

/// The highest window a model response reported for the ordinary bucket. It
/// carries no identity, permission or observation time, so it is evidence
/// against a clear reading only: an unreadable value counts as full, and a
/// window of unknown length counts as an ordinary one.
fn response_used(snapshot: &codex_protocol::protocol::RateLimitSnapshot) -> Option<f64> {
    if snapshot.limit_id.as_deref().is_some_and(|id| id != "codex") {
        return None;
    }
    [snapshot.primary.as_ref(), snapshot.secondary.as_ref()]
        .into_iter()
        .flatten()
        .filter(|w| {
            w.window_minutes
                .is_none_or(|minutes| matches!(minutes, 300 | 10080))
        })
        .map(|w| {
            if w.used_percent.is_finite() {
                w.used_percent
            } else {
                100.0
            }
        })
        .reduce(f64::max)
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
    pressure_cycle(read, owner, now, 100.0, 1)
}

/// A lagging reset timestamp can justify another read, never a redemption.
fn stale_exhaustion(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
) -> bool {
    matches(read, owner)
        && !explicit_denial(read)
        && read.ordinary_usage_allowed == Some(false)
        && read
            .rate_limits
            .iter()
            .filter(|snapshot| snapshot.limit_id.as_deref().is_none_or(|id| id == "codex"))
            .flat_map(|snapshot| [snapshot.primary.as_ref(), snapshot.secondary.as_ref()])
            .flatten()
            .any(|window| {
                matches!(window.window_minutes, Some(300 | 10080))
                    && window.used_percent.is_finite()
                    && window.used_percent >= 100.0
                    && window
                        .resets_at
                        .is_some_and(|reset| reset > 0 && reset <= now)
            })
}

/// The 429's named reset and the usage read's five-hour reset are two reports
/// of one moment (seconds since the Unix epoch). Further apart than this many
/// seconds they are not the same wall, and it is not offered to a host.
const RESET_AGREEMENT: u64 = 60;

/// The account's own allowance is spent and will not return by itself before
/// the reset this names: the wall another account could answer. The week is
/// judged first, as the host judges it, and a spent five-hour window is named
/// only where no weekly window is in the way: every weekly window the read
/// carries shows a finite figure under 100%. A read that carries no weekly
/// window satisfies that too, and names the five-hour wall. A weekly window
/// that is spent with no reset ahead, or whose figure is not a finite number,
/// names no wall at all. Among several spent windows of one length the wall is
/// the latest reset: it stands until the last of them. Whether a host would
/// leave the wall is the host's answer, not this one's.
fn spent_wall(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
) -> Option<SpentWall> {
    if !matches(read, owner) || explicit_denial(read) || read.ordinary_usage_allowed != Some(false)
    {
        return None;
    }
    let windows = |minutes: i64| {
        read.rate_limits
            .iter()
            .filter(|snapshot| snapshot.limit_id.as_deref().is_none_or(|id| id == "codex"))
            .flat_map(|snapshot| [snapshot.primary.as_ref(), snapshot.secondary.as_ref()])
            .flatten()
            .filter(move |window| window.window_minutes == Some(minutes))
    };
    // The latest reset ahead among the spent windows of that length.
    let resets_at = |minutes: i64| {
        windows(minutes)
            .filter(|window| window.used_percent.is_finite() && window.used_percent >= 100.0)
            .filter_map(|window| window.resets_at)
            .filter(|reset| *reset > now)
            .max()
    };
    if let Some(resets_at) = resets_at(10080) {
        return Some(SpentWall {
            window: SpentWindow::Week,
            resets_at,
        });
    }
    let week_usable =
        windows(10080).all(|window| window.used_percent.is_finite() && window.used_percent < 100.0);
    if !week_usable {
        return None;
    }
    Some(SpentWall {
        window: SpentWindow::FiveHour,
        resets_at: resets_at(300)?,
    })
}

/// Whether a host may be asked about this wall at all. A spent week always:
/// the turn that ends is resumed, or asked about, at whatever the 429 named
/// (which need not be the week's own reset: the Autopilot's bound on asking
/// says what that costs). A spent five-hour window only when the 429 named a
/// reset and it is the window's own: what follows the ended turn (the
/// Autopilot's limit on asking and its usage resume) runs on the 429's reset,
/// so with none named nothing would bound the ask, and one that disagrees
/// would resume the session at the wrong moment. Both are moments, not times
/// left, so the comparison does not depend on when it is made.
fn offerable(wall: SpentWall, reset_at: Option<i64>) -> bool {
    match wall.window {
        SpentWindow::Week => true,
        SpentWindow::FiveHour => {
            reset_at.is_some_and(|reset| reset.abs_diff(wall.resets_at) <= RESET_AGREEMENT)
        }
    }
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
    /// This turn's last permitting preflight read. A turn owns its `State`, so
    /// the first request of every turn reads, and nothing is shared between
    /// sessions or written to the custody ledger.
    clear: std::sync::Mutex<Option<Clear>>,
}

impl State {
    fn clear(&self) -> std::sync::MutexGuard<'_, Option<Clear>> {
        self.clear
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
struct Settings {
    home: PathBuf,
    base: String,
    factory: HttpClientFactory,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    // Unclassified uncertainty carried from a version-one receipt.
    Pending,
    // Version two persists this before POST, including uncertain retries.
    AwaitingAck,
    // Fresh usable allowance was observed while this ACK remained uncertain.
    RestoredPending,
    Consumed,
    NothingToReset,
    NoCredit,
}
impl Outcome {
    fn pending(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::AwaitingAck | Self::RestoredPending
        )
    }
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
            version: 2,
            next_generation: 1,
            current: None,
        }
    }
}
fn remember_restoration(path: &Path, ledger: &mut Ledger) -> Option<()> {
    let Some(op) = ledger
        .current
        .as_mut()
        .filter(|op| op.outcome.pending() && op.outcome != Outcome::RestoredPending)
    else {
        return Some(());
    };
    op.outcome = Outcome::RestoredPending;
    save(path, ledger)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Credit {
    id: String,
    consumed: bool,
    retry_released: bool,
}

/// Retry only an ordinary, authenticated usage wall. Each attempt releases the
/// cross-process lock before sleeping, so another session can use the account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recovery {
    Stop,
    Wait,
    /// One of the account's own windows is spent (`wall`), nothing is in
    /// redemption custody and no earned credit can be redeemed now. Polling is
    /// one answer; a host that would leave that wall for another account is
    /// the other. `conclusive` only when this attempt read the credit list and
    /// it offered nothing: an unread list, or one whose credit the backend
    /// just refused, may hide a credit.
    NoCredit {
        wall: SpentWall,
        conclusive: bool,
    },
    Spent,
    Restored,
}

async fn wait_for_reset(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    interval: Duration,
    reset_at: Option<i64>,
    handoff: Option<&codex_alab_core::quota::WallHandoff>,
) -> bool {
    let max_attempt = Duration::from_secs(45);
    // Consecutive creditless answers that did not rest on a clean list read.
    let mut inconclusive = 0_u32;
    loop {
        if !codex_alab_core::active() || identity(manager).as_ref() != Some(owner) {
            return false;
        }
        // Backend recovery must not postpone the native quota policy's reset,
        // including while an HTTP attempt is still waiting for its response.
        let attempt = reset_at.map_or(max_attempt, until_reset).min(max_attempt);
        if attempt.is_zero() {
            return false;
        }
        let outcome = tokio::time::timeout(
            attempt,
            recover(manager, config, owner, turn_id, Purpose::Recover),
        )
        .await;
        // The earned reset comes before another account, so one unread credit
        // list does not end the request: it is listed once more, one poll
        // later. A second such answer in a row does, so a list outage cannot
        // hold a spent allowance away from another account. The count is of
        // unread lists, not of readings of one wall: the two answers may name
        // different walls (a five-hour window, then the week), and the wall
        // offered is the second's.
        inconclusive = match outcome {
            Ok(Recovery::NoCredit {
                conclusive: false, ..
            }) => inconclusive.saturating_add(1),
            _ => 0,
        };
        match outcome {
            Ok(Recovery::Restored) => return true,
            Ok(Recovery::Stop) => return false,
            Ok(Recovery::Spent) if reset_at.is_none() => return false,
            Ok(Recovery::NoCredit { wall, conclusive })
                if (conclusive || inconclusive >= 2) && offerable(wall, reset_at) =>
            {
                // The earned reset was tried first and has nothing to redeem.
                // `recover` has returned, so neither the account resource nor
                // the custody lock is held while the host is asked. Ending
                // here returns the original usage error to the turn; a host
                // that is not ready leaves this request polling for a credit.
                if let Some(handoff) = handoff {
                    let ready = match reset_at {
                        Some(reset) => {
                            tokio::time::timeout(until_reset(reset), handoff.ready(wall))
                                .await
                                .unwrap_or(false)
                        }
                        None => handoff.ready(wall).await,
                    };
                    if ready {
                        return false;
                    }
                }
            }
            Ok(Recovery::Spent | Recovery::Wait | Recovery::NoCredit { .. }) | Err(_) => {}
        }
        let mut delay = interval;
        if let Some(reset) = reset_at {
            let remaining = until_reset(reset);
            if remaining.is_zero() {
                return false;
            }
            delay = delay.min(remaining);
        }
        // The caller owns cancellation of this original sampler/compaction.
        tokio::time::sleep(delay).await;
    }
}

fn until_reset(reset: i64) -> Duration {
    let seconds = reset.saturating_sub(chrono::Utc::now().timestamp());
    Duration::from_secs(u64::try_from(seconds).unwrap_or_default())
}

pub async fn recover_weekly(
    manager: &AuthManager,
    home: &Path,
    base: &str,
    factory: HttpClientFactory,
    turn_id: &str,
    state: &State,
    wall: crate::Wall<'_>,
) -> bool {
    // A wall contradicts a clear reading, and recovery changes the ledger.
    *state.clear() = None;
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
    wait_for_reset(
        manager,
        &config,
        &owner,
        turn_id,
        Duration::from_secs(30),
        wall.reset_at,
        wall.handoff,
    )
    .await
}

/// Check the authenticated allowance before sampling, independently of model text.
/// The existing account-bound ledger owns every redemption and uncertain retry.
///
/// A read that permitted an earlier request of this turn is reused while its
/// headroom proves the threshold out of reach ([`Clear::holds`]); `observed`,
/// the session's latest response rate limits, can only end that reuse sooner.
/// A reused reading takes no lock, no account lease and no HTTP request.
pub async fn preflight(
    manager: &AuthManager,
    home: &Path,
    base: &str,
    factory: HttpClientFactory,
    turn_id: &str,
    state: &State,
    observed: Option<&codex_protocol::protocol::RateLimitSnapshot>,
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
    {
        let mut clear = state.clear();
        if clear.is_some_and(|clear| {
            clear.holds(
                chrono::Utc::now().timestamp(),
                observed.and_then(response_used),
            )
        }) {
            return;
        }
        // Only the read below can clear this turn again.
        *clear = None;
    }
    let config = Settings {
        home: home.to_owned(),
        base: base.to_owned(),
        factory,
    };
    let _ = tokio::time::timeout(
        Duration::from_secs(45),
        recover_with(
            manager,
            &config,
            &owner,
            turn_id,
            Purpose::Preflight,
            Some(state),
        ),
    )
    .await;
}

async fn recover(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    purpose: Purpose,
) -> Recovery {
    recover_with(manager, config, owner, turn_id, purpose, None).await
}

/// `clearing` is the turn whose permitting preflight read may excuse its later
/// preflights; no other outcome, and no redemption, records one.
async fn recover_with(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    purpose: Purpose,
    clearing: Option<&State>,
) -> Recovery {
    let current = || codex_alab_core::active() && identity(manager).as_ref() == Some(owner);
    if !current() {
        return Recovery::Stop;
    }
    // Native reassignment drains this bounded attempt, including its receipts.
    // This lease ends before wait_for_reset sleeps between attempts.
    let Ok(_account_resource) = manager.alab_account_resource() else {
        return Recovery::Wait;
    };
    let Some(auth) = manager.auth().await else {
        return Recovery::Stop;
    };
    if !current() {
        return Recovery::Stop;
    }
    let client = Client::from_auth(config.base.clone(), &auth, config.factory.clone());
    let account = digest(&format!("{}\0{}\0{}", config.base, owner.0, owner.1));
    let lock = config.home.join(format!("weekly-reset-{account}"));
    let Ok(Some(_guard)) =
        codex_alab_core::auth_lock::AuthLock::take(&lock, Duration::from_secs(5)).await
    else {
        return Recovery::Wait;
    };
    let dir = config.home.join(format!("weekly-reset-{account}.d"));
    if private_dir(&dir).is_err() {
        return Recovery::Stop;
    }
    let state_path = dir.join("state.json");
    let mut ledger = match load::<Ledger>(&state_path) {
        Ok(Some(mut state)) if matches!(state.version, 1 | 2) => {
            if state.version == 1
                && state.current.as_ref().is_some_and(|op| {
                    matches!(op.outcome, Outcome::AwaitingAck | Outcome::RestoredPending)
                })
            {
                return Recovery::Stop;
            }
            state.version = 2;
            state
        }
        Ok(None) => Ledger::default(),
        _ => return Recovery::Stop,
    };
    if let Some(op) = ledger.current.as_ref()
        && (!valid_id(&op.credit)
            || op.generation == 0
            || op.generation >= ledger.next_generation
            || op.key != operation_key(&account, op.cycle, &op.credit, op.generation))
    {
        return Recovery::Stop;
    }
    let committed_credit = if let Some(op) = ledger.current.as_ref() {
        let Some(record) = credit_at(&dir, &op.credit) else {
            return Recovery::Stop;
        };
        if op.outcome == Outcome::Consumed && !record.consumed {
            return Recovery::Stop;
        }
        record.consumed
    } else {
        false
    };
    if !current() {
        return Recovery::Stop;
    }
    let read_at = chrono::Utc::now().timestamp();
    let read_started = std::time::Instant::now();
    let Ok(mut read) = client.get_rate_limits_with_reset_credits().await else {
        return Recovery::Wait;
    };
    if !current() {
        return Recovery::Stop;
    }
    let allowed = match purpose {
        Purpose::Preflight => permitted_after_preflight(&read, owner),
        Purpose::Recover => restored(&read, owner),
    };
    if allowed && remember_restoration(&state_path, &mut ledger).is_none() {
        return Recovery::Stop;
    }
    if purpose == Purpose::Preflight && allowed {
        // The credit receipt is committed before the ledger ACK. A crash in
        // between must not hide that proof from a successful preflight.
        if committed_credit {
            let _ = release(
                &dir,
                ledger.current.as_ref().map(|op| op.credit.as_str()),
                turn_id,
            );
        }
        // Recorded only here, after settlement: a reused reading is always
        // backed by a read that restored and released what the ledger held.
        if let Some(state) = clearing
            && let Some(used) = ordinary_used(&read)
        {
            *state.clear() = Some(Clear {
                read_at,
                read: read_started,
                used,
            });
        }
        return Recovery::Stop;
    }
    if purpose == Purpose::Recover && allowed {
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
            return Recovery::Stop;
        }
        let now = chrono::Utc::now().timestamp();
        let cycle = match purpose {
            Purpose::Recover => eligible(&read, owner, now),
            Purpose::Preflight => preflight_cycle(&read, owner, now),
        };
        let Some(cycle) = cycle else {
            return if purpose == Purpose::Recover && stale_exhaustion(&read, owner, now) {
                Recovery::Wait
            } else {
                Recovery::Stop
            };
        };
        // A successful POST whose new usage has not propagated must not burn
        // another credit on the next poll, even if its reset timestamp advanced.
        if ledger.current.as_ref().is_some_and(|op| {
            op.outcome == Outcome::Consumed
                && credit_at(&dir, &op.credit).is_some_and(|credit| !credit.retry_released)
        }) {
            return Recovery::Wait;
        }
        let pending = ledger
            .current
            .as_ref()
            .filter(|op| op.outcome.pending())
            .cloned();
        let op = if let Some(op) = pending {
            // A committed per-credit proof can survive a crash before state.json.
            let Some(credit) = credit_at(&dir, &op.credit) else {
                return Recovery::Stop;
            };
            if credit.consumed {
                let Some(current_op) = ledger.current.as_mut() else {
                    return Recovery::Stop;
                };
                current_op.outcome = Outcome::Consumed;
                if save(&state_path, &ledger).is_none() {
                    return Recovery::Stop;
                }
                continue;
            }
            op
        } else {
            let listed = available(&client, &dir, &tried, now).await;
            let Ok(Some(id)) = listed else {
                // Nothing is in custody here: a consumed credit awaiting
                // propagation returned above, and a pending operation never
                // lists credits. Only a list that was read, in an attempt that
                // had no credit refused, shows there is none; the caller
                // decides how long anything less may keep the request.
                let spent = spent_wall(&read, owner, now).filter(|_| purpose == Purpose::Recover);
                return match spent {
                    Some(wall) => Recovery::NoCredit {
                        wall,
                        conclusive: listed.is_ok() && tried.is_empty(),
                    },
                    None => Recovery::Wait,
                };
            };
            let Some(op) = begin(&dir, &account, &mut ledger, cycle, &id) else {
                return Recovery::Stop;
            };
            op
        };
        if !current() {
            return Recovery::Stop;
        }
        let previously_usable = op.outcome == Outcome::RestoredPending
            || (op.outcome == Outcome::Pending && op.cycle != cycle);
        if op.outcome != Outcome::AwaitingAck {
            // A lost ACK after this attempt must not be mistaken for historical
            // reconciliation on a later process or a changed reset timestamp.
            let Some(current_op) = ledger.current.as_mut() else {
                return Recovery::Stop;
            };
            current_op.outcome = Outcome::AwaitingAck;
            if save(&state_path, &ledger).is_none() {
                return Recovery::Stop;
            }
        }
        if !current() {
            return Recovery::Stop;
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
            None => Outcome::AwaitingAck,
        };
        if !current() {
            return Recovery::Stop;
        }
        if outcome == Outcome::Consumed {
            let Some(mut credit) = credit_at(&dir, &op.credit) else {
                return Recovery::Stop;
            };
            credit.consumed = true;
            if previously_usable && code == Some(ConsumeRateLimitResetCreditCode::AlreadyRedeemed) {
                // This known reply did not apply a reset to the currently denied
                // cycle. Retire the old retry right without granting a sampler retry.
                credit.retry_released = true;
            }
            if save(&credit_path(&dir, &op.credit), &credit).is_none() {
                return Recovery::Stop;
            }
        }
        let Some(current_op) = ledger.current.as_mut() else {
            return Recovery::Stop;
        };
        current_op.outcome = outcome;
        if save(&state_path, &ledger).is_none() {
            return Recovery::Stop;
        }
        let Ok(fresh) = client.get_rate_limits_with_reset_credits().await else {
            return Recovery::Wait;
        };
        if !current() {
            return Recovery::Stop;
        }
        let restored = match purpose {
            Purpose::Recover => restored(&fresh, owner),
            Purpose::Preflight => permitted_after_preflight(&fresh, owner),
        };
        if restored {
            if remember_restoration(&state_path, &mut ledger).is_none() {
                return Recovery::Stop;
            }
            return release(&dir, Some(&op.credit), turn_id);
        }
        // Unknown POST custody must be reconciled with this same immutable key.
        // A newly applied reset that has not restored usage cannot burn another.
        if outcome.pending()
            || code == Some(ConsumeRateLimitResetCreditCode::Reset)
            || (purpose == Purpose::Preflight && outcome == Outcome::Consumed)
        {
            return Recovery::Wait;
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
        outcome: Outcome::AwaitingAck,
    };
    ledger.next_generation = next;
    ledger.current = Some(op.clone());
    save(&dir.join("state.json"), ledger)?;
    Some(op)
}

/// The earned-credit list could not be read: it shows neither a credit nor
/// the absence of one.
struct Unlisted;

/// The credit to redeem next, `None` where the list was read and offers none.
async fn available(
    client: &Client,
    dir: &Path,
    tried: &std::collections::BTreeSet<String>,
    now: i64,
) -> Result<Option<String>, Unlisted> {
    let details = client
        .list_rate_limit_reset_credits()
        .await
        .map_err(|_| Unlisted)?;
    if details.available_count <= 0 {
        return Ok(None);
    }
    let next = details
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
        .map(|c| c.id.clone());
    Ok(next)
}

fn release(dir: &Path, id: Option<&str>, turn: &str) -> Recovery {
    let external = format!("turn-{}.external", digest(turn));
    let Some(id) = id else {
        return retry_marker(dir, &external);
    };
    let Some(mut credit) = credit_at(dir, id) else {
        return Recovery::Stop;
    };
    if !credit.consumed {
        return retry_marker(dir, &external);
    }
    if retry_marker(dir, &external) == Recovery::Stop {
        return Recovery::Stop;
    }
    if !credit.retry_released {
        // Fresh usage already proved account-wide restoration. Persist that
        // witness before issuing a turn's independently bounded retry.
        credit.retry_released = true;
        if save(&credit_path(dir, &credit.id), &credit).is_none() {
            return Recovery::Stop;
        }
    }
    // Another turn settling this credit must not spend this turn's retry.
    // A spent grant retains polling, without repeating the sampler request.
    retry_marker(
        dir,
        &format!("turn-{}.credit-{}.retry", digest(turn), digest(id)),
    )
}
fn retry_marker(dir: &Path, name: &str) -> Recovery {
    let path = dir.join(name);
    match read_receipt(&path) {
        Ok(Some(bytes)) if bytes.is_empty() => return Recovery::Spent,
        Ok(None) => {}
        _ => return Recovery::Stop,
    }
    use std::os::unix::fs::OpenOptionsExt;
    let Ok(file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
    else {
        return Recovery::Stop;
    };
    if file.sync_all().is_err()
        || std::fs::File::open(dir)
            .and_then(|dir| dir.sync_all())
            .is_err()
    {
        return Recovery::Stop;
    }
    Recovery::Restored
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
            clear: std::sync::Mutex::new(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(dir: &Path, id: Option<&str>, turn: &str) -> bool {
        super::release(dir, id, turn) == Recovery::Restored
    }
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
    fn recovery_accepts_near_resets_and_rejects_unusable_inputs() {
        let owner = ("account".into(), "user".into(), 1);
        assert_eq!(
            eligible(&reading(100.0, 10080, 1300), &owner, 1000),
            Some(1300)
        );
        assert_eq!(
            eligible(&reading(100.0, 10080, 1001), &owner, 1000),
            Some(1001)
        );
        for read in [reading(100.0, 10080, 1000), reading(99.9, 10080, 1300)] {
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
    fn stale_exhaustion_is_only_a_valid_same_account_wait() {
        let owner = ("account".into(), "user".into(), 1);
        for minutes in [300, 10080] {
            for reset in [999, 1000] {
                assert!(stale_exhaustion(
                    &reading(100.0, minutes, reset),
                    &owner,
                    1000
                ));
            }
        }
        for (used, minutes, reset) in [
            (99.0, 10080, 999),
            (100.0, 60, 999),
            (100.0, 10080, 0),
            (100.0, 10080, 1001),
        ] {
            assert!(!stale_exhaustion(
                &reading(used, minutes, reset),
                &owner,
                1000
            ));
        }
        let mut nonfinite = reading(100.0, 10080, 999);
        nonfinite.rate_limits[0]
            .secondary
            .as_mut()
            .unwrap()
            .used_percent = f64::NAN;
        assert!(!stale_exhaustion(&nonfinite, &owner, 1000));
        let mut read = reading(100.0, 10080, 999);
        read.account_id = Some("other".into());
        assert!(!stale_exhaustion(&read, &owner, 1000));
        read.account_id = Some("account".into());
        read.user_id = Some("other".into());
        assert!(!stale_exhaustion(&read, &owner, 1000));
        read.user_id = Some("user".into());
        for permission in [None, Some(true)] {
            read.ordinary_usage_allowed = permission;
            assert!(!stale_exhaustion(&read, &owner, 1000));
        }
        read.ordinary_usage_allowed = Some(false);
        read.rate_limits[0].limit_id = Some("premium".into());
        assert!(!stale_exhaustion(&read, &owner, 1000));
        read.rate_limits[0].limit_id = Some("codex".into());
        read.rate_limits[0].spend_control_reached = Some(true);
        assert!(!stale_exhaustion(&read, &owner, 1000));
        read.rate_limits[0].spend_control_reached = None;
        read.rate_limits[0].rate_limit_reached_type =
            Some(codex_protocol::protocol::RateLimitReachedType::WorkspaceOwnerCreditsDepleted);
        assert!(!stale_exhaustion(&read, &owner, 1000));
        read.rate_limits[0].rate_limit_reached_type = None;
        read.rate_limits[0].secondary.as_mut().unwrap().resets_at = None;
        assert!(!stale_exhaustion(&read, &owner, 1000));
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
                None,
            )
            .await;
        }
        let account = digest(&format!("{}\0account\0user", server.uri()));
        let dir = home.path().join(format!("weekly-reset-{account}.d"));
        let credit = credit_at(&dir, "A").unwrap();
        assert!(credit.consumed && credit.retry_released);
    }
    async fn late_credit(cancel_first_wait: bool, delayed_usage: bool, stale_start: bool) {
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
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_reads = Arc::clone(&reads);
        let reset = chrono::Utc::now().timestamp() + 3600;
        Mock::given(method("GET"))
            .and(path("/api/codex/usage"))
            .respond_with(move |_: &wiremock::Request| {
                let count = observed_reads.fetch_add(1, Ordering::SeqCst);
                let propagated = !delayed_usage || count >= 4;
                let reported_reset = if stale_start && count == 0 {
                    chrono::Utc::now().timestamp() - 1
                } else if delayed_usage && read_redeemed.load(Ordering::SeqCst) {
                    reset + 3600
                } else {
                    reset
                };
                let healthy = read_redeemed.load(Ordering::SeqCst) && propagated;
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "account_id":"account", "user_id":"user", "plan_type":"plus",
                    "rate_limit":{"allowed":healthy, "limit_reached":!healthy,
                        "primary_window":{"used_percent":if healthy {0}else{100},
                        "limit_window_seconds":18000,"reset_at":reported_reset,"reset_after_seconds":3600}}
                }))
            })
            .expect(if delayed_usage { 5 } else if stale_start { 4 } else { 3 })
            .mount(&server)
            .await;
        let lists = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&lists);
        Mock::given(method("GET")).and(path("/api/codex/rate-limit-reset-credits"))
            .respond_with(move |_: &wiremock::Request| {
                let available = observed.fetch_add(1, Ordering::SeqCst) > 0;
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "available_count":if available {if delayed_usage {2} else {1}}else{0},
                    "credits":if available {
                        let mut credits = vec![serde_json::json!({"id":"A","reset_type":"codex_rate_limits",
                            "status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-01-01T00:00:00Z"})];
                        if delayed_usage {
                            credits.push(serde_json::json!({"id":"B","reset_type":"codex_rate_limits",
                                "status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-02-01T00:00:00Z"}));
                        }
                        serde_json::json!(credits)
                    } else {serde_json::json!([])}
                }))
            }).expect(2).mount(&server).await;
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
        let config = Settings {
            home: home.path().to_owned(),
            base: server.uri(),
            factory: HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
        };
        if cancel_first_wait {
            assert!(
                tokio::time::timeout(
                    Duration::from_secs(2),
                    wait_for_reset(
                        &manager,
                        &config,
                        state.owner.as_ref().unwrap(),
                        "turn",
                        Duration::from_secs(3600),
                        None,
                        None
                    )
                )
                .await
                .is_err()
            );
            assert_eq!(lists.load(Ordering::SeqCst), 1);
        }
        assert!(
            tokio::time::timeout(
                Duration::from_secs(5),
                wait_for_reset(
                    &manager,
                    &config,
                    state.owner.as_ref().unwrap(),
                    "turn",
                    Duration::from_millis(10),
                    None,
                    None
                )
            )
            .await
            .unwrap()
        );
        let posts = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method.as_str() == "POST")
            .count();
        assert_eq!(
            posts, 1,
            "propagating usage must not spend a second reset credit"
        );
        assert_eq!(lists.load(Ordering::SeqCst), 2);
        let account = digest(&format!("{}\0account\0user", server.uri()));
        let dir = home.path().join(format!("weekly-reset-{account}.d"));
        let credit = credit_at(&dir, "A").unwrap();
        assert!(credit.consumed && credit.retry_released);
    }
    #[tokio::test]
    async fn credit_granted_after_exhaustion_resumes_the_original_turn() {
        late_credit(false, false, false).await;
    }

    #[tokio::test]
    async fn cancelling_a_wait_releases_account_custody() {
        late_credit(true, false, false).await;
    }

    #[tokio::test]
    async fn advanced_reset_with_delayed_usage_does_not_consume_another_credit() {
        late_credit(false, true, false).await;
    }

    #[tokio::test]
    async fn stale_reset_timestamp_keeps_polling_without_redeeming_stale_usage() {
        late_credit(false, false, true).await;
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
    fn spent_retry_is_distinct_from_an_invalid_marker() {
        for id in [None, Some("A")] {
            let dir = tempfile::tempdir().unwrap();
            if let Some(id) = id {
                let mut ledger = Ledger::default();
                begin(dir.path(), "account", &mut ledger, 44200, id).unwrap();
                let mut credit = credit_at(dir.path(), id).unwrap();
                credit.consumed = true;
                save(&credit_path(dir.path(), id), &credit).unwrap();
            }
            assert_eq!(super::release(dir.path(), id, "turn"), Recovery::Restored);
            assert_eq!(super::release(dir.path(), id, "turn"), Recovery::Spent);
            let marker = dir.path().join(if let Some(id) = id {
                format!("turn-{}.credit-{}.retry", digest("turn"), digest(id))
            } else {
                format!("turn-{}.external", digest("turn"))
            });
            std::fs::write(&marker, "invalid marker").unwrap();
            assert_eq!(super::release(dir.path(), id, "turn"), Recovery::Stop);
            std::fs::remove_file(&marker).unwrap();
            let target = dir.path().join("untouched");
            std::fs::write(&target, "untouched").unwrap();
            std::os::unix::fs::symlink(&target, &marker).unwrap();
            assert_eq!(super::release(dir.path(), id, "turn"), Recovery::Stop);
            assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
        }
    }

    #[test]
    fn shared_credits_release_each_waiting_turn_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::default();
        assert!(release(dir.path(), None, "waiting-turn"));
        for id in ["A", "B"] {
            begin(dir.path(), "account", &mut ledger, 44200, id).unwrap();
            let mut credit = credit_at(dir.path(), id).unwrap();
            credit.consumed = true;
            save(&credit_path(dir.path(), id), &credit).unwrap();
            assert!(release(dir.path(), Some(id), "redeeming-turn"));
            assert!(
                release(dir.path(), Some(id), "waiting-turn"),
                "another turn settling a reset must not strand this waiting turn"
            );
            assert!(!release(dir.path(), Some(id), "waiting-turn"));
            assert!(!release(dir.path(), Some(id), "redeeming-turn"));
        }
        assert!(!release(dir.path(), None, "waiting-turn"));
    }

    #[test]
    fn fifo_retry_marker_is_refused_without_waiting_for_a_writer() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let name = format!("turn-{}.external", digest("turn"));
        let path = CString::new(dir.path().join(&name).as_os_str().as_bytes()).unwrap();
        // No writer opens this FIFO: a blocking receipt read would hang this test.
        // SAFETY: path owns a live NUL-terminated string for the duration of the call.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert_eq!(retry_marker(dir.path(), &name), Recovery::Stop);
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
                &state,
                crate::Wall::default()
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
        let first = available(&client, dir.path(), &tried, 1000)
            .await
            .ok()
            .flatten()
            .unwrap();
        assert_eq!(first, "A", "parsed expiry orders offsets correctly");
        let a = begin(dir.path(), "account", &mut ledger, 44200, &first).unwrap();
        assert_eq!(a.key, operation_key("account", 44200, "A", 1));
        let mut record = credit_at(dir.path(), "A").unwrap();
        record.consumed = true;
        save(&credit_path(dir.path(), "A"), &record).unwrap();
        let second = available(&client, dir.path(), &tried, 1000)
            .await
            .ok()
            .flatten()
            .unwrap();
        assert_eq!(second, "B");
        let b = begin(dir.path(), "account", &mut ledger, 44200, &second).unwrap();
        assert_ne!(a.key, b.key);
        let mut record = credit_at(dir.path(), "B").unwrap();
        record.consumed = true;
        save(&credit_path(dir.path(), "B"), &record).unwrap();
        assert!(
            matches!(available(&client, dir.path(), &tried, 1000).await, Ok(None)),
            "stale A/B lists never forget either consumed credit"
        );
    }

    #[tokio::test]
    async fn invalid_and_uncertain_credits_do_not_hide_the_next_usable_credit() {
        use wiremock::Mock;
        use wiremock::MockServer;
        use wiremock::ResponseTemplate;
        use wiremock::matchers::method;
        use wiremock::matchers::path;
        let server = MockServer::start().await;
        let now = 1_893_456_000; // 2030-01-01T00:00:00Z
        let mut credits = Vec::new();
        for (id, expires) in [
            ("expired", Some("2029-12-31T23:59:59Z")),
            ("boundary", Some("2030-01-01T00:00:00Z")),
            ("malformed", Some("not-a-date")),
            ("uncertain", Some("2030-01-01T00:00:01Z")),
            ("tried", Some("2030-01-01T00:00:02Z")),
            ("usable", Some("2030-01-01T00:00:03Z")),
            ("no-expiry", None),
        ] {
            credits.push(serde_json::json!({"id":id,"reset_type":"codex_rate_limits",
                "status":"available","granted_at":"2026-01-01T00:00:00Z","expires_at":expires}));
        }
        Mock::given(method("GET"))
            .and(path("/api/codex/rate-limit-reset-credits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "available_count":7,"credits":credits
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = Client::new(
            server.uri(),
            HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
        );
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(credit_path(dir.path(), "uncertain"), "interrupted receipt").unwrap();
        let tried = std::collections::BTreeSet::from(["tried".to_owned()]);
        assert_eq!(
            available(&client, dir.path(), &tried, now)
                .await
                .ok()
                .flatten()
                .as_deref(),
            Some("usable")
        );
    }
}

#[cfg(test)]
#[path = "cancel_tests.rs"]
mod cancel_tests;

#[cfg(test)]
#[path = "assignment_tests.rs"]
mod assignment_tests;

#[cfg(test)]
#[path = "reconciliation_tests.rs"]
mod reconciliation_tests;

#[cfg(test)]
#[path = "deadline_tests.rs"]
mod deadline_tests;

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod handoff_tests;

#[cfg(test)]
#[path = "clear_tests.rs"]
mod clear_tests;

#[cfg(test)]
#[path = "fixture_tests.rs"]
mod fixture_tests;
