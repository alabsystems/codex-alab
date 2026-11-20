// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recover the original request with an earned reset or existing usage credits.
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
use codex_backend_client::Client;
use codex_backend_client::ConsumeRateLimitResetCreditCode;
use codex_backend_client::RateLimitsWithResetCredits;
use codex_http_client::HttpClientFactory;
use codex_login::AuthManager;
use codex_protocol::protocol::CreditsSnapshot;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

/// Every automatic redemption, including an uncertain operation's retry, needs
/// one ordinary window to satisfy both limits in the same fresh usage read.
const REDEMPTION_USED_PERCENT: f64 = 99.0;
/// Preserve earned resets when the weekly allowance returns within one day.
const REDEMPTION_MIN_REMAINING_SECONDS: i64 = 24 * 3600;
/// Stop reusing healthy reads before the redemption threshold is reachable.
const PREFLIGHT_CLEAR_USED_PERCENT: f64 = 95.0;
/// How long one point of headroom below the conservative read threshold excuses the
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

fn redemption_cycle(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
) -> Option<i64> {
    if !matches(read, owner) || explicit_denial(read) || read.ordinary_usage_allowed.is_none() {
        return None;
    }
    read.rate_limits
        .iter()
        .filter(|s| s.limit_id.as_deref().is_none_or(|id| id == "codex"))
        .flat_map(|s| [s.primary.as_ref(), s.secondary.as_ref()])
        .flatten()
        .filter(|w| w.used_percent.is_finite() && w.used_percent >= REDEMPTION_USED_PERCENT)
        .filter_map(|w| {
            if w.window_minutes != Some(10080) {
                return None;
            }
            let reset = w.resets_at?;
            (reset.saturating_sub(now) > REDEMPTION_MIN_REMAINING_SECONDS).then_some(reset)
        })
        .max()
}

fn timestamp(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map_or_else(|| "unknown time".into(), |time| time.to_rfc3339())
}

fn decline_reason(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
    now: i64,
) -> String {
    let reason = if !matches(read, owner) {
        "the usage reading does not match the active account".into()
    } else if explicit_denial(read) {
        "a workspace or spending restriction cannot be cleared by an ordinary usage reset".into()
    } else if read.ordinary_usage_allowed.is_none() {
        "the server has not confirmed ordinary usage permission".into()
    } else {
        let weekly = read
            .rate_limits
            .iter()
            .filter(|snapshot| snapshot.limit_id.as_deref().is_none_or(|id| id == "codex"))
            .flat_map(|snapshot| [snapshot.primary.as_ref(), snapshot.secondary.as_ref()])
            .flatten()
            .filter(|window| window.window_minutes == Some(10080))
            .filter(|window| {
                window.used_percent.is_finite() && window.used_percent >= REDEMPTION_USED_PERCENT
            })
            .filter_map(|window| window.resets_at)
            .max();
        match weekly {
            Some(reset) if reset.saturating_sub(now) <= REDEMPTION_MIN_REMAINING_SECONDS =>
                format!("the weekly allowance refreshes at {} (within 24 hours); preserving earned resets", timestamp(reset)),
            _ => "no verified weekly window is at least 99% used with its refresh more than 24 hours away; a five-hour limit alone does not qualify".into(),
        }
    };
    format!("Reset not used: {reason}.")
}

/// An authenticated spending restriction settles a passive check on its own.
/// Otherwise the decision requires a complete set of ordinary window percentages.
fn preserves_preflight(read: &RateLimitsWithResetCredits, owner: &(String, String, u64)) -> bool {
    if !matches(read, owner) || read.ordinary_usage_allowed.is_none() {
        return false;
    }
    if explicit_denial(read) {
        return true;
    }
    let mut high = false;
    for window in read
        .rate_limits
        .iter()
        .filter(|snapshot| snapshot.limit_id.as_deref().is_none_or(|id| id == "codex"))
        .flat_map(|snapshot| [snapshot.primary.as_ref(), snapshot.secondary.as_ref()])
        .flatten()
        .filter(|window| matches!(window.window_minutes, Some(300 | 10080)))
    {
        if !window.used_percent.is_finite() || window.used_percent < 0.0 {
            return false;
        }
        high |= window.used_percent >= REDEMPTION_USED_PERCENT;
    }
    high
}

fn permitted_after_preflight(
    read: &RateLimitsWithResetCredits,
    owner: &(String, String, u64),
) -> bool {
    usage_below(read, owner, PREFLIGHT_CLEAR_USED_PERCENT)
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
        let good_for = ((PREFLIGHT_CLEAR_USED_PERCENT - used) * PREFLIGHT_SECONDS_PER_POINT)
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
    credit_restriction(read).is_some()
}

fn credit_restriction(read: &RateLimitsWithResetCredits) -> Option<&'static str> {
    use codex_protocol::protocol::RateLimitReachedType::*;
    read.rate_limits
        .iter()
        .filter(|snapshot| snapshot.limit_id.as_deref().is_none_or(|id| id == "codex"))
        .find_map(|snapshot| match snapshot.rate_limit_reached_type {
            Some(WorkspaceOwnerCreditsDepleted | WorkspaceMemberCreditsDepleted) => {
                Some("The workspace is out of credits.")
            }
            Some(WorkspaceOwnerUsageLimitReached) => {
                Some("The workspace spending limit was reached.")
            }
            Some(WorkspaceMemberUsageLimitReached) => {
                Some("Your workspace spending limit was reached.")
            }
            Some(RateLimitReached) | None => (snapshot.spend_control_reached == Some(true))
                .then_some("The account spending limit was reached."),
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
    redemption_cycle(read, owner, now)
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

/// The backend can report an ordinary rate limit while existing usage credits
/// are available. A workspace limit, spend control or upsell prevents retry.
fn usable_usage_credits<'a>(
    read: &'a RateLimitsWithResetCredits,
    owner: &(String, String, u64),
) -> Option<&'a CreditsSnapshot> {
    if !matches(read, owner)
        || read.ordinary_usage_allowed.is_none()
        || read.rate_limit_upsell.is_some()
        || explicit_denial(read)
    {
        return None;
    }
    let ordinary = read
        .rate_limits
        .iter()
        .find(|s| s.limit_id.as_deref() == Some("codex"))?;
    if ordinary
        .individual_limit
        .as_ref()
        .is_some_and(|limit| limit.remaining_percent <= 0)
    {
        return None;
    }
    ordinary
        .credits
        .as_ref()
        .filter(|credits| credits.has_credits || credits.unlimited)
}

fn paid_retry_notice(credits: &CreditsSnapshot) -> String {
    if credits.unlimited {
        return "Retrying this request once with unlimited usage credits.".into();
    }
    match codex_alab_core::quota::reported_credit_amount(credits.balance.as_deref()) {
        Some(amount) => {
            format!("Retrying this request once with usage credits (reported balance {amount}).")
        }
        None => "Retrying this request once with available usage credits.".into(),
    }
}

fn usage_credit_marker(turn_id: &str) -> String {
    format!("turn-{}.usage-credits", digest(turn_id))
}

/// Retrying sends the original model request. Only server billing can say that
/// credits were actually used. This marker never settles an earned-reset receipt.
fn try_usage_credits(
    dir: &Path,
    marker: &str,
    credits: &CreditsSnapshot,
    state: Option<&State>,
) -> Recovery {
    let outcome = retry_marker(dir, marker);
    if outcome != Recovery::Restored {
        return outcome;
    }
    if let Some(state) = state {
        state.notice(NoticeKind::Credits, paid_retry_notice(credits));
    }
    Recovery::RetryWithUsageCredits
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
    notices: std::sync::Mutex<Notices>,
    /// Distinguish a timed-out redemption from a passive preflight lookup.
    redemption_attempts: std::sync::atomic::AtomicU64,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NoticeKind {
    Credits,
    Decision,
    Inventory,
    Result,
    Status,
}

#[derive(Default)]
struct Notices {
    sender: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    last: std::collections::BTreeMap<NoticeKind, String>,
}

impl State {
    /// Attach the turn's transcript delivery without discarding decision deduplication.
    pub fn set_notice_sender(&self, sender: tokio::sync::mpsc::UnboundedSender<String>) {
        self.notices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sender = Some(sender);
    }

    fn reports(&self) -> bool {
        self.notices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sender
            .is_some()
    }

    fn notice(&self, kind: NoticeKind, message: impl Into<String>) {
        let message = message.into();
        let mut notices = self
            .notices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if notices.last.get(&kind) == Some(&message) {
            return;
        }
        let prefix = match kind {
            NoticeKind::Credits => codex_alab_core::quota::USAGE_CREDIT_NOTICE,
            NoticeKind::Decision
            | NoticeKind::Inventory
            | NoticeKind::Result
            | NoticeKind::Status => codex_alab_core::quota::EARNED_RESET_NOTICE,
        };
        if notices
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(format!("{prefix}{message}")).is_ok())
        {
            notices.last.insert(kind, message);
        }
    }

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
    /// Fresh permitted usage below the preflight threshold, cleared before POST.
    /// A reactive retry's weaker restoration witness cannot grant this right.
    #[serde(default)]
    preflight_cleared: bool,
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
fn remember_restoration(path: &Path, ledger: &mut Ledger, preflight_clear: bool) -> Option<()> {
    let Some(op) = ledger.current.as_mut() else {
        return Some(());
    };
    let reconcile = op.outcome.pending() && op.outcome != Outcome::RestoredPending;
    let cleared = preflight_clear && !op.preflight_cleared;
    if reconcile {
        op.outcome = Outcome::RestoredPending;
    }
    if cleared {
        op.preflight_cleared = true;
    }
    if reconcile || cleared {
        save(path, ledger)
    } else {
        Some(())
    }
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
    /// No reset or additional retry is needed. Return to native scheduling
    /// without caching the reading or reporting a failed verification.
    NotNeeded,
    /// A checked ordinary preflight preserves earned resets under the policy;
    /// this is neither clearance nor a verification failure.
    Preserved,
    Wait,
    /// One of the account's own windows is spent (`wall`), nothing is in
    /// redemption custody and no earned credit can be redeemed now. Polling is
    /// one answer; a host that would leave that wall for another account is
    /// the other. `conclusive` when the current policy forbids redemption, or
    /// this attempt read a credit list that offered nothing: an unread list,
    /// or one whose credit the backend just refused, may hide a credit.
    NoCredit {
        wall: SpentWall,
        conclusive: bool,
    },
    Spent,
    Restored,
    RetryWithUsageCredits,
}

#[cfg(test)]
async fn wait_for_reset(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    interval: Duration,
    reset_at: Option<i64>,
    handoff: Option<&codex_alab_core::quota::WallHandoff>,
) -> bool {
    wait_for_reset_observed(
        manager,
        config,
        owner,
        turn_id,
        interval,
        crate::Wall { reset_at, handoff },
        None,
    )
    .await
}

async fn wait_for_reset_observed(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    interval: Duration,
    wall: crate::Wall<'_>,
    state: Option<&State>,
) -> bool {
    let crate::Wall { reset_at, handoff } = wall;
    let max_attempt = Duration::from_secs(45);
    // When the unbroken run of creditless answers that did not rest on a clean
    // list read began: the start of its first look.
    let mut unread_since: Option<tokio::time::Instant> = None;
    // Made before the first look, so a signal the host sends while a look or its
    // question is in flight is kept for the wait that follows.
    let mut wake = handoff.map(codex_alab_core::quota::WallHandoff::watch);
    loop {
        // Told at every look, before it reads anything. A look can end before the
        // question (an unread credit list, a read that failed, a timeout), and a
        // host that signals only for looks that may predate its change must hear
        // of those too, or this request waits out the interval below.
        if let Some(handoff) = handoff {
            handoff.look_begins();
        }
        let looked = tokio::time::Instant::now();
        if !codex_alab_core::active() || identity(manager).as_ref() != Some(owner) {
            if let Some(state) = state {
                state.notice(NoticeKind::Status, "Reset recovery stopped: the authenticated account changed or ALab recovery was disabled; no further reset is authorized by this request.");
            }
            return false;
        }
        // Backend recovery must not postpone the native quota policy's reset,
        // including while an HTTP attempt is still waiting for its response.
        let attempt = reset_at.map_or(max_attempt, until_reset).min(max_attempt);
        if attempt.is_zero() {
            if let Some(state) = state {
                state.notice(NoticeKind::Status, "Reset not used: the provider refresh deadline has arrived; returning to the ordinary usage retry path.");
            }
            return false;
        }
        let outcome = tokio::time::timeout(
            attempt,
            recover_with(manager, config, owner, turn_id, Purpose::Recover, state),
        )
        .await;
        // The earned reset comes before another account, so an unread credit
        // list does not end the request by itself: it is listed again, and the
        // host is asked only at a later look that finds the list unread still,
        // a whole poll interval or more after the first. With no signal from
        // the host that is the second of two polls, so a list outage cannot
        // hold a spent allowance away from another account.
        //
        // The rule is a span of time, not a count of looks, because a host's
        // signal brings looks of its own (below), as little as a second apart.
        // Counted, two of those would end the request over an outage of a
        // second or two, with a credit behind it that the next poll would have
        // found; and a rule that counted only the poll's own looks would never
        // yield while signals kept coming, since each one starts the poll's
        // wait again. The span is of unread lists, not of readings of one wall:
        // its answers may name different walls (a five-hour window, then the
        // week), and the wall offered is the last one's.
        let outage = match outcome {
            Ok(Recovery::NoCredit {
                conclusive: false, ..
            }) => match unread_since {
                Some(first) => looked.duration_since(first) >= interval,
                None => {
                    unread_since = Some(looked);
                    false
                }
            },
            _ => {
                unread_since = None;
                false
            }
        };
        match outcome {
            Ok(Recovery::Restored | Recovery::RetryWithUsageCredits) => return true,
            Ok(Recovery::Stop | Recovery::NotNeeded | Recovery::Preserved) => return false,
            Ok(Recovery::Spent) if reset_at.is_none() => return false,
            Ok(Recovery::NoCredit { wall, conclusive }) if conclusive || outage => {
                // No reset may be redeemed now: the policy forbids it, or the
                // earned-credit check has nothing eligible to redeem.
                // `recover` has returned, so neither the account resource nor
                // the custody lock is held while the host is asked. Ending
                // here returns the original usage error to the turn; a host
                // that is not ready leaves this request polling for a credit.
                //
                // The host is told the usage read's wall, whatever reset the
                // 429 named: none, the window's own, or another moment. A yes
                // is given for that wall, and its owner keeps it as the ended
                // turn's (`WallHandoff`), so what follows the turn (the bound
                // on asking the host, the wall's own resume) runs on the
                // read's reset. The 429's reset bounds only this request's
                // own wait below.
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
            Err(_) => {
                if let Some(state) = state {
                    state.notice(NoticeKind::Status, "Reset recovery check timed out; availability, expiry or restored allowance could not be confirmed. Retrying without replacing any unconfirmed reset operation.");
                }
            }
            Ok(Recovery::Spent | Recovery::Wait | Recovery::NoCredit { .. }) => {}
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
        //
        // A HOST THAT WAS NOT READY CAN SAY WHEN TO ASK IT AGAIN. This request keeps
        // its turn running while it waits, and a running turn keeps the session's
        // record busy; a host that moves accounts only under an idle record would
        // otherwise wait out the rest of this interval for the turn it needs
        // ended. Its signal ends the wait early and nothing more: the loop looks
        // for an earned reset first, as at every poll, and asks the host again
        // only if there is none. A host that never signals leaves the interval.
        let Some(wake) = wake.as_mut() else {
            tokio::time::sleep(delay).await;
            continue;
        };
        if tokio::time::timeout(delay, wake.changed()).await.is_ok() {
            // Each look is a usage read and a credit list read, so signals that
            // keep coming buy one look per floor (or per poll, where that is
            // the shorter), measured between the starts of two looks.
            let since = looked.elapsed();
            let floor = WAKE_FLOOR.min(delay);
            if since < floor {
                tokio::time::sleep(floor - since).await;
            }
        }
    }
}

/// The least time between the starts of two looks a host's signal asks for
/// ([`wait_for_reset_observed`]).
const WAKE_FLOOR: Duration = Duration::from_secs(1);

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
        state.notice(NoticeKind::Decision, "Considered earned usage reset: available count and expiry unknown because authentication changed or is unavailable. No reset used.");
        return false;
    };
    let config = Settings {
        home: home.to_owned(),
        base: base.to_owned(),
        factory,
    };
    wait_for_reset_observed(
        manager,
        &config,
        &owner,
        turn_id,
        Duration::from_secs(30),
        wall,
        Some(state),
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
    let attempts = state
        .redemption_attempts
        .load(std::sync::atomic::Ordering::Relaxed);
    if tokio::time::timeout(
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
    .await
    .is_err()
        && state
            .redemption_attempts
            .load(std::sync::atomic::Ordering::Relaxed)
            != attempts
    {
        state.notice(NoticeKind::Status, "Earned reset confirmation timed out. The next check will reconcile the recorded operation.");
    }
}

#[cfg(test)]
async fn recover(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    purpose: Purpose,
) -> Recovery {
    recover_with(manager, config, owner, turn_id, purpose, None).await
}

/// `clearing` reports the turn's reset decisions and owns its permitting preflight
/// reading. Only a successful preflight records clearance for later requests.
async fn recover_with(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    purpose: Purpose,
    clearing: Option<&State>,
) -> Recovery {
    let outcome =
        recover_with_expiry_clock(manager, config, owner, turn_id, purpose, clearing, &|| {
            chrono::Utc::now().timestamp()
        })
        .await;
    if outcome == Recovery::Stop
        && let Some(state) = clearing
        && (purpose == Purpose::Recover || state.clear().is_none())
    {
        state.notice(NoticeKind::Status, "Reset recovery stopped: account identity, quota eligibility or the redemption ledger could not authorize continuing safely. Any earlier unconfirmed reset remains recorded. No further reset will be spent until that operation is reconciled.");
    }
    outcome
}

/// The supplied clock judges only newly selected credit expiry, not quota policy.
async fn recover_with_expiry_clock(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    purpose: Purpose,
    clearing: Option<&State>,
    expiry_now: &(dyn Fn() -> i64 + Sync),
) -> Recovery {
    recover_with_clocks(
        manager,
        config,
        owner,
        turn_id,
        purpose,
        clearing,
        RecoveryClocks {
            quota: &|| chrono::Utc::now().timestamp(),
            expiry: expiry_now,
        },
    )
    .await
}

struct RecoveryClocks<'a> {
    quota: &'a (dyn Fn() -> i64 + Sync),
    expiry: &'a (dyn Fn() -> i64 + Sync),
}

async fn recover_with_clocks(
    manager: &AuthManager,
    config: &Settings,
    owner: &(String, String, u64),
    turn_id: &str,
    purpose: Purpose,
    clearing: Option<&State>,
    clocks: RecoveryClocks<'_>,
) -> Recovery {
    let expiry_now = clocks.expiry;
    let current = || codex_alab_core::active() && identity(manager).as_ref() == Some(owner);
    if !current() {
        return Recovery::Stop;
    }
    // Native reassignment drains this bounded attempt, including its receipts.
    // This lease ends before wait_for_reset sleeps between attempts.
    let Ok(_account_resource) = manager.alab_account_resource() else {
        if let Some(state) = clearing.filter(|_| purpose == Purpose::Recover) {
            state.notice(NoticeKind::Decision, "Reset not used: the active account is being reassigned; waiting for stable authentication.");
        }
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
        if let Some(state) = clearing.filter(|_| purpose == Purpose::Recover) {
            state.notice(NoticeKind::Decision, "Reset not used: the account reset lock could not be acquired; waiting to avoid spending twice.");
        }
        return Recovery::Wait;
    };
    let dir = config.home.join(format!("weekly-reset-{account}.d"));
    if private_dir(&dir).is_err() {
        if let Some(state) = clearing {
            state.notice(
                NoticeKind::Decision,
                "Reset not used: a private redemption ledger could not be opened safely.",
            );
        }
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
    let read_at = (clocks.quota)();
    let read_started = std::time::Instant::now();
    let Ok(mut read) = client.get_rate_limits_with_reset_credits().await else {
        if let Some(state) = clearing.filter(|_| {
            purpose == Purpose::Recover
                || ledger
                    .current
                    .as_ref()
                    .is_some_and(|op| op.outcome.pending())
        }) {
            state.notice(NoticeKind::Decision, "Reset not used: current account usage could not be verified; available count and expiry remain unknown until the backend can be read.");
        }
        return Recovery::Wait;
    };
    if !current() {
        return Recovery::Stop;
    }
    if purpose == Purpose::Recover && restored(&read, owner) {
        return resume_on_allowance(
            &dir,
            &mut ledger,
            turn_id,
            permitted_after_preflight(&read, owner),
            clearing,
        );
    }
    if purpose == Purpose::Preflight && permitted_after_preflight(&read, owner) {
        if remember_restoration(&state_path, &mut ledger, true).is_none() {
            return Recovery::Stop;
        }
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
    let mut tried = std::collections::BTreeSet::new();
    loop {
        if purpose == Purpose::Preflight
            && ledger.current.as_ref().is_some_and(|op| {
                op.outcome == Outcome::Consumed
                    && (!op.preflight_cleared
                        || credit_at(&dir, &op.credit).is_none_or(|credit| !credit.retry_released))
            })
        {
            return Recovery::Stop;
        }
        let now = (clocks.quota)();
        let cycle = match purpose {
            Purpose::Recover => eligible(&read, owner, now),
            Purpose::Preflight => redemption_cycle(&read, owner, now),
        };
        let Some(cycle) = cycle else {
            if purpose == Purpose::Preflight && usage_below(&read, owner, REDEMPTION_USED_PERCENT) {
                // A valid reading between the cache and redemption thresholds
                // needs another read next time, without implying a failed reset.
                return Recovery::NotNeeded;
            }
            if purpose == Purpose::Preflight
                && current()
                && preserves_preflight(&read, owner)
                && ledger
                    .current
                    .as_ref()
                    .is_none_or(|op| !op.outcome.pending())
            {
                return Recovery::Preserved;
            }
            if purpose == Purpose::Recover {
                if stale_exhaustion(&read, owner, now) {
                    return Recovery::Wait;
                }
                let wall = spent_wall(&read, owner, now);
                let held = ledger.current.as_ref().is_some_and(|op| {
                    op.outcome.pending()
                        || (op.outcome == Outcome::Consumed
                            && credit_at(&dir, &op.credit)
                                .is_none_or(|credit| !credit.retry_released))
                });
                if !held && current() {
                    if matches(&read, owner)
                        && read.ordinary_usage_allowed.is_some()
                        && let Some(reason) = credit_restriction(&read)
                    {
                        if let Some(state) = clearing {
                            state.notice(
                                NoticeKind::Credits,
                                format!("Credit retry skipped. {reason}"),
                            );
                        }
                        return Recovery::NotNeeded;
                    }
                    if let Some(credits) = usable_usage_credits(&read, owner) {
                        let retry = try_usage_credits(
                            &dir,
                            &usage_credit_marker(turn_id),
                            credits,
                            clearing,
                        );
                        if retry != Recovery::Spent {
                            return retry;
                        }
                        if wall.is_none() {
                            return Recovery::NotNeeded;
                        }
                    }
                }
                if let Some(wall) = wall {
                    if held {
                        return Recovery::Wait;
                    }
                    if let Some(state) = clearing.filter(|state| state.reports())
                        && let Ok(Some(_)) = available_observed(
                            &client,
                            &dir,
                            &tried,
                            expiry_now,
                            purpose,
                            Some(state),
                        )
                        .await
                    {
                        state.notice(NoticeKind::Decision, decline_reason(&read, owner, now));
                    }
                    return if current() {
                        Recovery::NoCredit {
                            wall,
                            conclusive: true,
                        }
                    } else {
                        Recovery::Stop
                    };
                }
            }
            return Recovery::Stop;
        };
        // A successful POST whose new usage has not propagated must not burn
        // another credit on the next poll, even if its reset timestamp advanced.
        if ledger.current.as_ref().is_some_and(|op| {
            op.outcome == Outcome::Consumed
                && credit_at(&dir, &op.credit).is_some_and(|credit| !credit.retry_released)
        }) {
            if let Some(state) = clearing {
                state.notice(NoticeKind::Decision, "A reset was already consumed; waiting for restored allowance before considering another reset.");
            }
            return Recovery::Wait;
        }
        let pending = ledger
            .current
            .as_ref()
            .filter(|op| op.outcome.pending())
            .cloned();
        let (op, selected) = if let Some(op) = pending {
            if let Some(state) = clearing {
                state.notice(NoticeKind::Inventory, "Considered earned usage reset: available count and expiry are not re-listed while reconciling an earlier reset. Retaining its operation key to avoid spending twice.");
            }
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
            (op, None)
        } else {
            let listed =
                available_observed(&client, &dir, &tried, expiry_now, purpose, clearing).await;
            if !current() {
                return Recovery::Stop;
            }
            let Ok(Some(selected)) = listed else {
                if purpose == Purpose::Recover
                    && listed.is_ok()
                    && tried.is_empty()
                    && usable_usage_credits(&read, owner).is_some()
                {
                    // A resumed turn may have spent its one paid retry. Only an
                    // unused retry needs another credit authorization after listing.
                    let marker = usage_credit_marker(turn_id);
                    let retry = if let Some(existing) = existing_retry(&dir.join(&marker)) {
                        Some(existing)
                    } else {
                        let Ok(fresh) = client.get_rate_limits_with_reset_credits().await else {
                            return Recovery::Wait;
                        };
                        if !current() {
                            return Recovery::Stop;
                        }
                        read = fresh;
                        if restored(&read, owner) {
                            return resume_on_allowance(
                                &dir,
                                &mut ledger,
                                turn_id,
                                permitted_after_preflight(&read, owner),
                                clearing,
                            );
                        }
                        usable_usage_credits(&read, owner)
                            .map(|credits| try_usage_credits(&dir, &marker, credits, clearing))
                    };
                    if let Some(outcome) = retry {
                        if outcome != Recovery::Spent {
                            return outcome;
                        }
                        if spent_wall(&read, owner, (clocks.quota)()).is_none() {
                            return Recovery::NotNeeded;
                        }
                    }
                }
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
            if let Some(state) = clearing {
                state.notice(NoticeKind::Decision, format!("Using an earned usage reset automatically: the weekly allowance refreshes at {}, more than 24 hours away. Choosing the earliest-expiring usable reset.", timestamp(cycle)));
            }
            let Some(op) = begin(&dir, &account, &mut ledger, cycle, &selected.id) else {
                return Recovery::Stop;
            };
            (op, Some(selected))
        };
        if !current() {
            return Recovery::Stop;
        }
        let previously_usable = op.outcome == Outcome::RestoredPending
            || (op.outcome == Outcome::Pending && op.cycle != cycle);
        if op.outcome != Outcome::AwaitingAck || op.preflight_cleared {
            // A lost ACK after this attempt must not be mistaken for historical
            // reconciliation on a later process or a changed reset timestamp.
            let Some(current_op) = ledger.current.as_mut() else {
                return Recovery::Stop;
            };
            current_op.outcome = Outcome::AwaitingAck;
            current_op.preflight_cleared = false;
            if save(&state_path, &ledger).is_none() {
                return Recovery::Stop;
            }
        }
        if !current() {
            return Recovery::Stop;
        }
        // Listing credits and persisting custody can cross the natural reset.
        // A window that qualified before that wait cannot authorize a POST now.
        if redemption_cycle(&read, owner, (clocks.quota)()) != Some(cycle) {
            if let Some(state) = clearing {
                state.notice(NoticeKind::Decision, "Reset not used: the weekly allowance is now within 24 hours of its refresh; preserving the earned reset.");
            }
            return Recovery::Wait;
        }
        // Another reset or a concurrent usage update can invalidate the read
        // while credits are listed or custody is persisted. Every POST needs
        // a fresh read of the same owner's ordinary quota window.
        let Ok(before_post) = client.get_rate_limits_with_reset_credits().await else {
            if let Some(state) = clearing {
                state.notice(NoticeKind::Decision, "Reset not used: the final account usage check failed; waiting for a verified reading.");
            }
            return Recovery::Wait;
        };
        if !current() {
            return Recovery::Stop;
        }
        let now = (clocks.quota)();
        let fresh_cycle = match purpose {
            Purpose::Recover => eligible(&before_post, owner, now),
            Purpose::Preflight => redemption_cycle(&before_post, owner, now),
        };
        if fresh_cycle != Some(cycle) {
            if let Some(state) = clearing {
                state.notice(
                    NoticeKind::Decision,
                    format!(
                        "Reset not used: quota changed before redemption. {}",
                        decline_reason(&before_post, owner, now)
                    ),
                );
            }
            return Recovery::Wait;
        }
        // Only this call's newly selected credit carries expiry evidence. Do
        // not send its initial POST after that evidence expires during I/O.
        // Persisted pending operations still reconcile their exact key with
        // the authoritative backend, without listing or changing custody.
        if selected.is_some_and(|credit| !credit.unexpired(expiry_now())) {
            if let Some(state) = clearing {
                state.notice(
                    NoticeKind::Decision,
                    "Reset not used: the selected earned reset expired before redemption.",
                );
            }
            return Recovery::Wait;
        }
        tried.insert(op.credit.clone());
        if let Some(state) = clearing {
            state
                .redemption_attempts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let reply = client
            .consume_rate_limit_reset_credit_by_id(&op.key, &op.credit)
            .await;
        let code = reply.ok().map(|reply| reply.code);
        if let Some(state) = clearing {
            state.notice(NoticeKind::Result, match code {
                Some(ConsumeRateLimitResetCreditCode::Reset) => "Earned usage reset applied; checking that allowance is restored.",
                Some(ConsumeRateLimitResetCreditCode::AlreadyRedeemed) => "The earned usage reset was already redeemed; checking restored allowance without spending another.",
                Some(ConsumeRateLimitResetCreditCode::NothingToReset) => "Reset not used: the backend reports nothing to reset.",
                Some(ConsumeRateLimitResetCreditCode::NoCredit) => "Reset not used: the backend reports the selected reset is unavailable.",
                None => "Reset outcome is unconfirmed; retaining the same operation key and checking allowance before any retry.",
            });
        }
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
            if remember_restoration(
                &state_path,
                &mut ledger,
                permitted_after_preflight(&fresh, owner),
            )
            .is_none()
            {
                return Recovery::Stop;
            }
            let outcome = release(&dir, Some(&op.credit), turn_id);
            if let Some(state) = clearing {
                state.notice(NoticeKind::Result, if outcome == Recovery::Restored {
                    "Usage allowance restored after the earned reset; continuing the original request."
                } else {
                    "Usage allowance restored; the recorded retry could not be released, so no additional reset will be spent."
                });
            }
            return outcome;
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
        preflight_cleared: false,
    };
    ledger.next_generation = next;
    ledger.current = Some(op.clone());
    save(&dir.join("state.json"), ledger)?;
    Some(op)
}

/// The earned-credit list could not be read: it shows neither a credit nor
/// the absence of one.
struct Unlisted;

struct AvailableCredit {
    id: String,
    expires_at: Option<i64>,
}

impl AvailableCredit {
    fn unexpired(&self, now: i64) -> bool {
        self.expires_at.is_none_or(|expiry| expiry > now)
    }
}

/// The credit to redeem next, `None` where the list was read and offers none.
#[cfg(test)]
async fn available(
    client: &Client,
    dir: &Path,
    tried: &std::collections::BTreeSet<String>,
    expiry_now: impl FnOnce() -> i64,
) -> Result<Option<AvailableCredit>, Unlisted> {
    available_observed(client, dir, tried, expiry_now, Purpose::Recover, None).await
}

async fn available_observed(
    client: &Client,
    dir: &Path,
    tried: &std::collections::BTreeSet<String>,
    expiry_now: impl FnOnce() -> i64,
    purpose: Purpose,
    state: Option<&State>,
) -> Result<Option<AvailableCredit>, Unlisted> {
    let details = match client.list_rate_limit_reset_credits().await {
        Ok(details) => details,
        Err(_) => {
            if let Some(state) = state.filter(|_| purpose == Purpose::Recover) {
                state.notice(NoticeKind::Inventory, "Considered earned usage reset: available count and expiry unknown because the reset list could not be read. No new reset will be used until availability is confirmed.");
            }
            return Err(Unlisted);
        }
    };
    if details.available_count <= 0 {
        return Ok(None);
    }
    // The awaited list may itself have crossed a credit's expiry.
    let now = expiry_now();
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
        .map(|c| AvailableCredit {
            id: c.id.clone(),
            expires_at: c.expires_at.as_deref().and_then(|expiry| {
                chrono::DateTime::parse_from_rfc3339(expiry)
                    .ok()
                    .map(|expiry| expiry.timestamp())
            }),
        });
    if let Some(state) = state.filter(|_| next.is_some()) {
        let mut expiries = details
            .credits
            .iter()
            .filter(|credit| {
                credit.status == "available" && credit.reset_type == "codex_rate_limits"
            })
            .map(|credit| match credit.expires_at.as_deref() {
                Some(expiry) => chrono::DateTime::parse_from_rfc3339(expiry)
                    .map(|time| time.to_utc().to_rfc3339())
                    .unwrap_or_else(|_| "unknown expiry (invalid server timestamp)".into()),
                None => "expiry not provided".into(),
            })
            .collect::<Vec<_>>();
        // The provider can reorder unchanged grants between polls.
        expiries.sort_unstable();
        state.notice(
            NoticeKind::Inventory,
            format!(
                "Considered earned usage reset: {} available; expirations: {}.",
                details.available_count,
                expiries.join(", ")
            ),
        );
    }
    Ok(next)
}

/// A confirmed ordinary allowance uses its own retry receipt, including when
/// it returns during the final check before a paid retry.
fn resume_on_allowance(
    dir: &Path,
    ledger: &mut Ledger,
    turn_id: &str,
    preflight_clear: bool,
    state: Option<&State>,
) -> Recovery {
    if remember_restoration(&dir.join("state.json"), ledger, preflight_clear).is_none() {
        return Recovery::Stop;
    }
    let outcome = release(
        dir,
        ledger.current.as_ref().map(|op| op.credit.as_str()),
        turn_id,
    );
    if let Some(state) = state {
        state.notice(NoticeKind::Result, if outcome == Recovery::Restored {
            "Allowance restored; continuing the original request without spending another reset."
        } else {
            "Allowance restored; the recorded retry could not be released, so no additional reset will be spent."
        });
    }
    outcome
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
fn existing_retry(path: &Path) -> Option<Recovery> {
    match read_receipt(path) {
        Ok(Some(bytes)) if bytes.is_empty() => Some(Recovery::Spent),
        Ok(None) => None,
        _ => Some(Recovery::Stop),
    }
}
fn retry_marker(dir: &Path, name: &str) -> Recovery {
    let path = dir.join(name);
    if let Some(outcome) = existing_retry(&path) {
        return outcome;
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

/// Premium and spend-control walls cannot consume an ordinary allowance reset. Reserve can: it
/// is ordinary usage's fallback ([`crate::wall_kind`], which defines this so the two never
/// drift).
pub fn ordinary_weekly_wall(details: &codex_protocol::error::UsageLimitReachedError) -> bool {
    crate::wall_kind(details) == crate::WallKind::Ordinary
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
            ..Default::default()
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
    fn paid_retry_reports_only_requested_account_credit_facts() {
        for (unlimited, balance, expected) in [
            (
                true,
                Some("1234"),
                "Retrying this request once with unlimited usage credits.",
            ),
            (
                false,
                Some(" 0.0001200 "),
                "Retrying this request once with usage credits (reported balance 0.0001200).",
            ),
            (
                false,
                None,
                "Retrying this request once with available usage credits.",
            ),
            (
                false,
                Some("NaN"),
                "Retrying this request once with available usage credits.",
            ),
            (
                false,
                Some("12\nnotice"),
                "Retrying this request once with available usage credits.",
            ),
        ] {
            let credits = CreditsSnapshot {
                has_credits: true,
                unlimited,
                balance: balance.map(str::to_owned),
            };
            assert_eq!(paid_retry_notice(&credits), expected);
        }
    }

    #[test]
    fn credit_retry_requires_authenticated_usable_ordinary_credits() {
        use codex_protocol::protocol::CreditsSnapshot;
        use codex_protocol::protocol::RateLimitReachedType;

        let owner = ("account".to_string(), "user".to_string(), 1);
        let mut read = reading(100.0, 10080, 200_000);
        assert!(usable_usage_credits(&read, &owner).is_none());
        read.rate_limits[0].credits = Some(CreditsSnapshot {
            has_credits: false,
            unlimited: false,
            balance: Some("1234".into()),
        });
        assert!(usable_usage_credits(&read, &owner).is_none());
        read.rate_limits[0].credits.as_mut().unwrap().unlimited = true;
        assert!(usable_usage_credits(&read, &owner).is_some());
        read.rate_limits[0].rate_limit_reached_type =
            Some(RateLimitReachedType::WorkspaceMemberUsageLimitReached);
        assert!(usable_usage_credits(&read, &owner).is_none());
        read.rate_limits[0].rate_limit_reached_type = Some(RateLimitReachedType::RateLimitReached);
        assert!(usable_usage_credits(&read, &owner).is_some());
        read.rate_limits[0].spend_control_reached = Some(false);
        read.rate_limits[0].individual_limit =
            Some(codex_protocol::protocol::SpendControlLimitSnapshot {
                limit: "100".into(),
                used: "100".into(),
                remaining_percent: 0,
                resets_at: 200_000,
            });
        assert!(usable_usage_credits(&read, &owner).is_none());
        read.rate_limits[0].individual_limit = None;
        read.rate_limits[0].limit_id = Some("premium".into());
        assert!(usable_usage_credits(&read, &owner).is_none());
        read.rate_limits[0].limit_id = Some("codex".into());
        read.rate_limit_upsell = Some(serde_json::json!({"offer": "add_credits"}));
        assert!(usable_usage_credits(&read, &owner).is_none());
        read.rate_limit_upsell = None;
        read.ordinary_usage_allowed = None;
        assert!(usable_usage_credits(&read, &owner).is_none());
        read.ordinary_usage_allowed = Some(true);
        read.user_id = Some("another-user".into());
        assert!(usable_usage_credits(&read, &owner).is_none());
    }

    #[test]
    fn recovery_accepts_distant_weekly_resets_and_rejects_unusable_inputs() {
        let owner = ("account".into(), "user".into(), 1);
        let distant = 1000 + 72 * 3600;
        assert_eq!(
            eligible(&reading(100.0, 10080, distant), &owner, 1000),
            Some(distant)
        );
        let just_over_boundary = 1000 + 24 * 3600 + 1;
        assert_eq!(
            eligible(&reading(100.0, 10080, just_over_boundary), &owner, 1000),
            Some(just_over_boundary)
        );
        for read in [reading(100.0, 10080, 1000), reading(98.9, 10080, distant)] {
            assert_eq!(eligible(&read, &owner, 1000), None);
        }
        let mut read = reading(100.0, 10080, distant);
        assert_eq!(redemption_cycle(&read, &owner, 1000), Some(distant));
        read.user_id = Some("other".into());
        assert_eq!(eligible(&read, &owner, 1000), None);
        assert_eq!(redemption_cycle(&read, &owner, 1000), None);
        read.user_id = Some("user".into());
        read.account_id = Some("other".into());
        assert_eq!(eligible(&read, &owner, 1000), None);
        assert_eq!(redemption_cycle(&read, &owner, 1000), None);
        read.account_id = Some("account".into());
        read.ordinary_usage_allowed = None;
        assert_eq!(eligible(&read, &owner, 1000), None);
        assert_eq!(redemption_cycle(&read, &owner, 1000), None);
        read.ordinary_usage_allowed = Some(true);
        assert_eq!(eligible(&read, &owner, 1000), None);
        assert_eq!(redemption_cycle(&read, &owner, 1000), Some(distant));
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
            let distant = 1000 + 72 * 3600;
            let mut read = reading(100.0, 10080, distant);
            assert_eq!(eligible(&read, &owner, 1000), Some(distant));
            assert_eq!(redemption_cycle(&read, &owner, 1000), Some(distant));
            read.rate_limits[0].rate_limit_reached_type = reason;
            read.rate_limits[0].spend_control_reached = reason.is_none().then_some(true);
            assert_eq!(eligible(&read, &owner, 1000), None);
            assert_eq!(redemption_cycle(&read, &owner, 1000), None);
            read.ordinary_usage_allowed = Some(true);
            read.rate_limits[0].secondary.as_mut().unwrap().used_percent = 0.0;
            assert!(!restored(&read, &owner));
        }
    }

    #[test]
    fn automatic_redemption_requires_99_percent_and_weekly_refresh_beyond_24_hours() {
        let owner = ("account".into(), "user".into(), 1);
        for minutes in [300, 10080] {
            for (used, remaining, expected) in [
                (f64::from_bits(99.0_f64.to_bits() - 1), 72 * 3600, false),
                (99.0, 1, false),
                (100.0, 24 * 3600 - 1, false),
                (99.0, 24 * 3600, false),
                (99.0, 24 * 3600 + 1, true),
                (99.0, 25 * 3600, true),
                (99.0, 48 * 3600, true),
                (99.0, 49 * 3600, true),
                (100.0, 7 * 24 * 3600, true),
                (100.0, 0, false),
                (100.0, -1, false),
            ] {
                let mut read = reading(used, minutes, 1000 + remaining);
                let expected = (expected && minutes == 10080).then_some(1000 + remaining);
                assert_eq!(eligible(&read, &owner, 1000), expected);
                for permission in [Some(false), Some(true)] {
                    read.ordinary_usage_allowed = permission;
                    assert_eq!(redemption_cycle(&read, &owner, 1000), expected);
                }
                read.ordinary_usage_allowed = None;
                assert_eq!(redemption_cycle(&read, &owner, 1000), None);
            }
            for used in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut read = reading(100.0, minutes, 1000 + 72 * 3600);
                read.rate_limits[0].secondary.as_mut().unwrap().used_percent = used;
                assert_eq!(eligible(&read, &owner, 1000), None);
                assert_eq!(redemption_cycle(&read, &owner, 1000), None);
            }
            let mut missing = reading(100.0, minutes, 1000 + 72 * 3600);
            missing.rate_limits[0].secondary.as_mut().unwrap().resets_at = None;
            assert_eq!(eligible(&missing, &owner, 1000), None);
            assert_eq!(redemption_cycle(&missing, &owner, 1000), None);
        }
    }

    #[test]
    fn ineligible_weekly_refresh_cannot_use_another_windows_refresh() {
        let owner = ("account".into(), "user".into(), 1);
        for reset in [Some(1000), Some(999), None] {
            let mut read = reading(100.0, 10080, 1000 + 72 * 3600);
            assert_eq!(eligible(&read, &owner, 1000), Some(1000 + 72 * 3600));
            read.rate_limits[0].secondary.as_mut().unwrap().resets_at = reset;
            read.rate_limits[0].primary = reading(0.0, 10080, 1000 + 72 * 3600).rate_limits[0]
                .secondary
                .take();
            assert_eq!(eligible(&read, &owner, 1000), None);
            for permission in [Some(false), Some(true)] {
                read.ordinary_usage_allowed = permission;
                assert_eq!(redemption_cycle(&read, &owner, 1000), None);
            }
        }
    }

    #[test]
    fn redemption_uses_future_ordering_at_timestamp_bounds() {
        let owner = ("account".into(), "user".into(), 1);
        for (now, reset, expected) in [
            (i64::MIN, i64::MIN, None),
            (i64::MAX, i64::MAX, None),
            (i64::MAX, i64::MIN, None),
            (i64::MIN, i64::MAX, Some(i64::MAX)),
            (i64::MAX - 86400, i64::MAX, None),
            (i64::MAX - 86401, i64::MAX, Some(i64::MAX)),
            (i64::MIN, i64::MIN + 86401, Some(i64::MIN + 86401)),
        ] {
            let read = reading(99.0, 10080, reset);
            assert_eq!(eligible(&read, &owner, now), expected);
            assert_eq!(redemption_cycle(&read, &owner, now), expected);
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
        let reset = chrono::Utc::now().timestamp() + 72 * 3600;
        Mock::given(method("GET")).and(path("/api/codex/usage"))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "account_id":"account", "user_id":"user", "plan_type":"plus",
                    "rate_limit":{"allowed":true, "limit_reached":false,
                        "primary_window":{"used_percent":if read_redeemed.load(Ordering::SeqCst) {0}else{99},
                        "limit_window_seconds":604800,"reset_at":reset,"reset_after_seconds":259200}}
                }))
            }).expect(4).mount(&server).await;
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
        let reset = chrono::Utc::now().timestamp() + 72 * 3600;
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
                        "limit_window_seconds":604800,"reset_at":reported_reset,"reset_after_seconds":259200}}
                }))
            })
            .expect(if delayed_usage || stale_start { 5 } else { 4 })
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
        let cycle = chrono::Utc::now().timestamp() + 72 * 3600;
        let observed = Arc::clone(&restored_flag);
        Mock::given(method("GET")).and(path("/api/codex/usage"))
            .respond_with(move |_:&wiremock::Request| {
                let restored=observed.load(Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "account_id":"account","user_id":"user","plan_type":"plus",
                    "rate_limit":{"allowed":restored,"limit_reached":!restored,
                        "secondary_window":{"used_percent":if restored {0}else{100},"limit_window_seconds":604800,"reset_at":cycle,"reset_after_seconds":259200}}
                }))
            }).expect(5).mount(&server).await;
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
        let first = available(&client, dir.path(), &tried, || 1000)
            .await
            .ok()
            .flatten()
            .unwrap();
        assert_eq!(first.id, "A", "parsed expiry orders offsets correctly");
        let a = begin(dir.path(), "account", &mut ledger, 44200, &first.id).unwrap();
        assert_eq!(a.key, operation_key("account", 44200, "A", 1));
        let mut record = credit_at(dir.path(), "A").unwrap();
        record.consumed = true;
        save(&credit_path(dir.path(), "A"), &record).unwrap();
        let second = available(&client, dir.path(), &tried, || 1000)
            .await
            .ok()
            .flatten()
            .unwrap();
        assert_eq!(second.id, "B");
        let b = begin(dir.path(), "account", &mut ledger, 44200, &second.id).unwrap();
        assert_ne!(a.key, b.key);
        let mut record = credit_at(dir.path(), "B").unwrap();
        record.consumed = true;
        save(&credit_path(dir.path(), "B"), &record).unwrap();
        assert!(
            matches!(
                available(&client, dir.path(), &tried, || 1000).await,
                Ok(None)
            ),
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
            available(&client, dir.path(), &tried, || now)
                .await
                .ok()
                .flatten()
                .map(|credit| credit.id),
            Some("usable".to_owned())
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

#[cfg(test)]
#[path = "notice_tests.rs"]
mod notice_tests;
