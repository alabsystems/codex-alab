// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Follow native account recovery without replacing a running AuthManager.
//! The host restarts the process; only the committed
//! receipt permits recovery of the original native turn in the new process.

use super::account_broker;
use super::account_broker::Outcome;
use super::account_broker::Receipt;
use super::*;
use crate::supervisor::AdmissionFence;
use codex_alab_aterm::renewal::Identity;
use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
use codex_alab_core::session::AccountHandoff;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_alab_core::session::RecoveryWall;
use codex_alab_core::wall_kind::LimitClass;
use codex_history::RolloutItem;

use codex_alab_aterm::account_recovery::policy_allows as policy_allows_recovery;

/// HOW SOON A BOUND RECEIPT'S POINT IS LOOKED AT AGAIN, a login's as a usage wall's: the host
/// answered `ready` behind it, so what is left is its restart of this session (`waiting`) or
/// this process's own recovery of the turn (`committed`), and no person answers either. The
/// minute a login waited here before lost the happy successor's turn to a 270 s budget.
const BOUND_RECHECK: Duration = Duration::from_secs(5);

/// How soon a renewal point of `purpose` is looked at again where nothing else says when. A
/// usage wall's quota preparation runs asynchronously in aterm and its observation expires
/// after a minute, so a ready result is consumed promptly, and a lost login's move is asked
/// about as promptly because its ask is bounded; a login's own renewal waits the ordinary
/// policy-recheck period. Any is looked at after [`BOUND_RECHECK`] at most once a receipt
/// binds its intent (`bound`), on every path that keeps the point: a pass that could not read
/// the goal's custody or the thread's history as much as one that read the host.
fn point_recheck(purpose: Purpose, bound: bool) -> Duration {
    let unbound = match purpose {
        Purpose::WeeklyQuota | Purpose::LoginRotation => Duration::from_secs(5),
        Purpose::Login => RECHECK,
    };
    if bound {
        unbound.min(BOUND_RECHECK)
    } else {
        unbound
    }
}

/// Whether a receipt binds `owed`'s renewal intent.
fn receipt_bound(owed: &OwedAct) -> bool {
    owed.renewal
        .as_ref()
        .is_some_and(|renewal| renewal.receipt.is_some())
}

/// HOW SOON A PASS FOLLOWS A RECOVERED TURN'S REFUSED ADMISSION ([`admission_retry`]). A
/// refusal starts no work and keeps the intent and its receipt, and it is most often a fact
/// that moved during the admission's host reads and stands still a moment later. It is not
/// free, though: core reserves the turn and writes its settings into the rollout (one
/// `ThreadSettingsApplied` item) before it asks the admission, and a refusal clears the
/// reservation only. So core is not asked while the TUI itself says wait
/// ([`Autopilot::recover_renewal`]; that look is paced by this too), and a refusal that
/// stands is asked again less often ([`ADMISSION_STANDS`]).
const ADMISSION_RETRY: Duration = Duration::from_secs(1);

/// HOW LONG ONE REASON IS REFUSED IN A ROW BEFORE IT IS A WAIT and no longer a fact that
/// moved (a hold, a host that goes on answering otherwise): from then on it is asked again
/// every [`RECHECK`], as a wait with no deadline of its own is anywhere else. Seconds apart
/// without end, each ask wrote its rollout item for as long as the reason stood.
const ADMISSION_STANDS: Duration = RECHECK;

/// The wait before the next pass after `refusals` refused admissions in a row before this
/// one, whose reason has been given for `stood`: [`ADMISSION_RETRY`], doubling with each and
/// never past [`BOUND_RECHECK`], until the reason has stood for [`ADMISSION_STANDS`]; then
/// [`RECHECK`].
fn admission_retry(refusals: u64, stood: Duration) -> Duration {
    if stood >= ADMISSION_STANDS {
        return RECHECK;
    }
    let doublings = u32::try_from(refusals).unwrap_or(u32::MAX);
    ADMISSION_RETRY
        .saturating_mul(2u32.saturating_pow(doublings))
        .min(BOUND_RECHECK)
}

/// A walled turn's recovered starts that did not go, in a row: a series is one walled turn's
/// and ends when the turn starts. Memory only.
pub(super) struct AdmissionRefusals {
    /// The walled turn they were asked for.
    turn: String,
    /// Those among them that lengthen the wait ([`admission_retry`]): the admissions core
    /// refused, and the looks the TUI's own wait stopped before core was asked.
    refused: u64,
    /// The latest one's reason, which the log and the journal already said.
    reason: String,
    /// When that reason was first given in this row ([`ADMISSION_STANDS`]).
    since: Instant,
    /// Whether the row's refusals have slowed to [`RECHECK`], which was said too.
    standing: bool,
}

impl AdmissionRefusals {
    /// `turn`'s recovered start did not go at `now`, for `reason`; `refused` where that
    /// lengthens the next wait (core asked the admission and it refused, or the TUI's own wait
    /// stopped the look), and not where core did not take the start. Returns how many of this
    /// series were `refused` before this one, whether this one is news, and how long `reason`
    /// has been the reason in a row: the log and the journal say why once per reason in a row,
    /// and once more when it has stood long enough to be looked at less often
    /// ([`ADMISSION_STANDS`]), not once per pass, since a reason that stands (a person's
    /// draft, a hold) is met again at every look.
    fn record(
        series: &mut Option<Self>,
        turn: String,
        reason: &str,
        refused: bool,
        now: Instant,
    ) -> (u64, bool, Duration) {
        let (before, row) = match series.take() {
            Some(was) if was.turn == turn => (
                was.refused,
                (was.reason == reason).then_some((was.since, was.standing)),
            ),
            _ => (0, None),
        };
        let since = row.map_or(now, |(since, _)| since);
        let stood = now.saturating_duration_since(since);
        let standing = refused && stood >= ADMISSION_STANDS;
        let news = row.is_none_or(|(_, was_standing)| standing && !was_standing);
        *series = Some(Self {
            turn,
            refused: before.saturating_add(u64::from(refused)),
            reason: reason.to_owned(),
            since,
            standing,
        });
        (before, news, stood)
    }
}

/// THE PERSON'S LINE FOR A MOVE THE SESSION'S BACKGROUND TERMINALS HOLD
/// ([`Autopilot::tell_terminals_hold`]): fixed words that name no account and no thread, as
/// they are said in every root thread's history, within the 256 bytes the app server keeps
/// of a warning. They say another account has room, so they are said only where the host's
/// pool shows one.
pub(super) const MOVE_WAITS_FOR_TERMINALS: &str = "The usage limit is reached, and another \
                                                   signed-in account has room. Background \
                                                   terminals run in this session, and the \
                                                   restart would end them, so the move waits \
                                                   until they finish or you stop them.";

/// THE HOST'S REFUSALS OF ONE ASK SERIES THE JOURNAL TOLD ([`Autopilot::note_handoff_refused`]):
/// the asked turn, and each closed word ([`codex_alab_aterm::rotation::REFUSALS`]) the host
/// answered it with. A refusal that stands is met at every pass, five seconds apart, so it is
/// said once a series. Memory only.
#[derive(Default)]
pub(super) struct HandoffRefusals {
    turn: String,
    words: Vec<String>,
}

impl HandoffRefusals {
    /// The host refused `turn`'s handoff in `word`: whether that is news to this series. A
    /// series is one asked turn's; another turn's starts a new one.
    fn record(&mut self, turn: &str, word: &str) -> bool {
        if self.turn != turn {
            self.turn = turn.to_owned();
            self.words.clear();
        }
        if self.words.iter().any(|told| told == word) {
            return false;
        }
        self.words.push(word.to_owned());
        true
    }
}

#[cfg(test)]
#[path = "renewal_admission_retry_tests.rs"]
mod admission_retry_tests;

#[path = "renewal_completion.rs"]
pub(super) mod completion;
use completion::Seen;

fn starts(items: &[RolloutItem], turn: &str) -> u64 {
    items
        .iter()
        .filter(|item| {
            matches!(item,
        RolloutItem::EventMsg(EventMsg::TurnStarted(start)) if start.turn_id == turn)
        })
        .count() as u64
}

fn bound(receipt: &Receipt, owed: &OwedAct) -> bool {
    let (Some(handoff), Some(origin)) = (&owed.renewal, &owed.native_origin) else {
        return false;
    };
    receipt.matches_handoff(&owed.thread, &origin.turn_id, handoff)
}

/// A lost mutation ACK is reconciled through the host's exact idempotent state.
/// Both calls keep their existing deadlines; uncertainty never releases custody.
async fn confirmed_consumption(client: &codex_alab_aterm::Client, owed: &OwedAct) -> bool {
    let Some(renewal) = owed.renewal.as_ref() else {
        return false;
    };
    let (Some(id), Some(origin)) = (renewal.receipt.as_deref(), owed.native_origin.as_ref()) else {
        return false;
    };
    let outcome = match account_broker::consume(client, renewal.purpose, id, &origin.turn_id).await
    {
        Ok(outcome) => Ok(outcome),
        Err(_) => account_broker::status(client, renewal.purpose, id).await,
    };
    matches!(outcome, Ok(Outcome::Consumed(ref consumed)) if bound(consumed, owed))
}

/// Whether the host's account pool shows another account this process's
/// conversation could move to (`Snapshot::another_account_can_run`). A read
/// only: no claim, selection or credential travels with it. A host that does
/// not serve the pool, or does not answer for this very process, shows none.
///
/// Schema 2 is asked first: it names the row aterm judges this session to run
/// on, so one able row beside it is enough. An aterm before schema 2 is asked
/// schema 1, which names no row, so two able rows are needed there: with one
/// spare account a walled session on such an aterm keeps waiting for its reset.
async fn rotation_target_in_sight(client: &codex_alab_aterm::Client) -> bool {
    let Some(birth) = codex_alab_tui::identity::current()
        .ok()
        .and_then(|identity| codex_alab_tui::identity::proc_start_utc(&identity))
    else {
        return false;
    };
    client
        .accounts_snapshot_for_move(&birth)
        .await
        .is_ok_and(|pool| pool.another_account_can_run())
}

/// Whether an account handoff that no receipt binds has been asked for long
/// enough: the usage wall it was asked at named its own reset, and that reset
/// plus `grace` has come. From then the wall's ordinary resume is due, which
/// is all a `not-needed` answer would have left, so the ask ends and that
/// resume takes the point.
///
/// For a turn the host's yes ended (quota recovery's handoff), the reset is
/// the one the walled request's usage read named for the window the turn was
/// yielded for, whatever the 429 named: the turn's error builds its wall from
/// the read the yes was given for (`TrackData::handoff_wall`). A usage wall
/// no yes ended (a turn whose request ended on its own, a point a restart
/// rebuilt from history alone) carries the 429's reset, or none.
///
/// A bound receipt is never abandoned here: the host may already have
/// committed it. A wall that names no reset has no such moment, and neither
/// has an intent whose wall this process does not hold. An unbound intent
/// carries its wall across a restart (`carry_asked_wall`) and a bound one does
/// not, so an intent bound before a restart and retired after it is asked for
/// without this limit.
fn rotation_ask_lapsed(
    wall: Option<&Wall>,
    receipt_bound: bool,
    now: Instant,
    grace: Duration,
) -> bool {
    !receipt_bound
        && matches!(
            wall,
            Some(Wall::Usage { reset_at: Some(reset), .. })
                if reset.checked_add(grace).is_some_and(|due| now >= due)
        )
}

/// What a host that permits a recovery was read with ([`RecoveryHost::Permits`]).
type Permitted = (
    codex_alab_aterm::Client,
    decide::Policy,
    codex_alab_aterm::policy::HarnessPolicy,
);

/// What the host says about recovery for one purpose when it is asked
/// ([`Autopilot::recovery_host`]).
enum RecoveryHost {
    /// Its policy reply permits the recovery: the claim's connection, the
    /// policy codex-alab acts under, and that reply (boxed: the other answers
    /// carry nothing).
    Permits(Box<Permitted>),
    /// Its policy reply was read and does not permit the recovery.
    Refuses,
    /// No policy reply was read: the claim's connection or the policy it acts
    /// under is gone, the read failed, or the answer was no policy this reader
    /// takes (an `ERR`). `follow_renewal` runs only after `evaluate` found a
    /// proven claim's policy, so a missing connection here is a claim lost
    /// since, never a session outside aterm.
    Unread,
}

/// What one pass of [`Autopilot::follow_renewal`] came to.
pub(super) enum Follow {
    /// It keeps the point: the decision ends here, and the pass's own recheck is the next.
    Keeps,
    /// It lets the point go to the rest of the decision. `checked`: the pass stood at a scope a
    /// renewal may be asked for ([`Autopilot::eligible_renewal_scope`]) and read on from
    /// there, so the lost login's watch it arms owes that profile no second pass
    /// ([`Autopilot::watch_login`]).
    Leaves { checked: bool },
}

/// What one pass of [`Autopilot::follow_renewal`] holds the claim by.
enum Pass {
    /// No receipt binds the intent: the before-every-act checks ran and re-confirmed the claim
    /// for this pass ([`Supervisor::ready_for_admission`]). It may ask the host to prepare,
    /// and commit what the host answers `ready`, sealing the claim to that receipt.
    Confirmed(AdmissionFence),
    /// A receipt binds the intent: the host sealed the claim as it stood at its `ready`, so
    /// this pass only reads (the receipt's `status`), under the claim as already observed
    /// ([`Supervisor::admission_observation`]), and re-confirms nothing.
    Sealed(AdmissionFence),
}

impl Pass {
    /// Whether the authority this pass was taken under still stands after an await. A sealed
    /// pass only reads, and the reads need no more than its caller's own `current()`.
    fn stands(&self, sup: &Supervisor) -> bool {
        match self {
            Self::Confirmed(fence) => sup.admission_fence_current(fence),
            Self::Sealed(_) => true,
        }
    }

    /// Whether this pass may send `commit` for a receipt the host answered `ready`: a
    /// confirmed pass under its unchanged readiness; a sealed one only while the claim it
    /// holds is still the one the host sealed, unchanged since it was observed.
    fn may_commit(&self, sup: &Supervisor) -> bool {
        match self {
            Self::Confirmed(fence) => sup.admission_fence_current(fence),
            Self::Sealed(fence) => sup.claim_sealed() && sup.admission_fence_current(fence),
        }
    }
}

enum RenewalAckCause<'a> {
    /// Core persists TurnComplete only after this typed lifecycle callback returns.
    NativeStop {
        store: &'a ExtensionData,
        epoch: u64,
    },
    PersistedTerminal,
}

pub(super) struct RenewalAckRetry {
    owed: OwedAct,
    accepted: u64,
    starts: u64,
    pub(super) attempts: AtomicU64,
}

pub(super) struct TransientGoalRecovery {
    runtime: std::sync::Weak<codex_goal_extension::GoalRuntimeHandle>,
    wall: String,
    goal_id: Option<String>,
    token: u64,
}

impl Drop for TransientGoalRecovery {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.alab_finish_transient_auth_recovery(self.token);
        }
    }
}

impl Autopilot {
    fn renewal_ack_owned(&self, rt: &ThreadRt, owed: &OwedAct, epoch: u64) -> bool {
        let registered = {
            let threads = lock(&self.threads);
            Self::current_runtime(&threads, rt)
        };
        registered
            && !rt.stopped.load(Ordering::SeqCst)
            && rt.accepted_turns.load(Ordering::SeqCst) == epoch
            && self.sup.shared().owed_act().as_ref() == Some(owed)
            && owed.native_origin.as_ref().is_some_and(|origin| {
                match self.sup.shared().latest_turn(&rt.key) {
                    Some(turn) => turn == origin.turn_id,
                    // A fresh restored process may have run no local turn.
                    // Only fresh durable terminal history can grant its RPC.
                    None => epoch == 0,
                }
            })
    }

    fn renewal_ack_current(&self, rt: &ThreadRt, retry: &Arc<RenewalAckRetry>) -> bool {
        let token = lock(&rt.renewal_ack_retry)
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, retry));
        token && self.renewal_ack_owned(rt, &retry.owed, retry.accepted)
    }

    fn finish_renewal_ack_retry(rt: &ThreadRt, retry: &Arc<RenewalAckRetry>) {
        let mut current = lock(&rt.renewal_ack_retry);
        if current
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, retry))
        {
            *current = None;
        }
    }

    fn renewal_ack_history(
        rt: &ThreadRt,
        owed: &OwedAct,
        history: &[RolloutItem],
        cause: &RenewalAckCause<'_>,
    ) -> bool {
        let (Some(origin), Some(renewal)) = (&owed.native_origin, &owed.renewal) else {
            return false;
        };
        let latest = crate::resume_history::latest(history);
        owed.thread == rt.key
            && renewal.accepted
            && origin.phase == NativePhase::Complete
            && renewal.turn.as_deref() == Some(origin.turn_id.as_str())
            && latest.turn_id == Some(origin.turn_id.as_str())
            && match cause {
                RenewalAckCause::NativeStop { store, epoch } => {
                    store.level_id() == origin.turn_id.as_str()
                        && rt.accepted_turns.load(Ordering::SeqCst) == *epoch
                }
                RenewalAckCause::PersistedTerminal => matches!(
                    latest.point,
                    crate::resume_history::NativePoint::Complete(_)
                ),
            }
            && starts(history, &origin.turn_id) > renewal.starts
    }

    fn retry_renewal_ack(
        &self,
        rt: &Arc<ThreadRt>,
        owed: OwedAct,
        history: &[RolloutItem],
        cause: RenewalAckCause<'_>,
    ) {
        if !Self::renewal_ack_history(rt, &owed, history, &cause)
            || self.sup.shared().owed_act().as_ref() != Some(&owed)
        {
            return;
        }
        let observed_starts = owed
            .native_origin
            .as_ref()
            .map_or(0, |origin| starts(history, &origin.turn_id));
        let retry = Arc::new(RenewalAckRetry {
            owed,
            accepted: match cause {
                RenewalAckCause::NativeStop { epoch, .. } => epoch,
                RenewalAckCause::PersistedTerminal => rt.accepted_turns.load(Ordering::SeqCst),
            },
            starts: observed_starts,
            attempts: AtomicU64::new(0),
        });
        {
            let mut current = lock(&rt.renewal_ack_retry);
            if current.as_ref().is_some_and(|current| {
                current.owed == retry.owed
                    && current.accepted == retry.accepted
                    && current.starts == retry.starts
            }) {
                return;
            }
            *current = Some(Arc::clone(&retry));
        }
        let owner = self.me.clone();
        let thread = Arc::downgrade(rt);
        tokio::spawn(async move {
            let mut delay = Duration::from_secs(1);
            loop {
                tokio::time::sleep(delay).await;
                // Strong owners exist only for one bounded attempt, never over sleep.
                let (Some(me), Some(rt)) = (owner.upgrade(), thread.upgrade()) else {
                    return;
                };
                if !me.renewal_ack_current(&rt, &retry) {
                    Self::finish_renewal_ack_retry(&rt, &retry);
                    return;
                }
                retry.attempts.fetch_add(1, Ordering::SeqCst);
                if let Some(thread) = me.thread(rt.id).await
                    && let Ok(history) = resumed_history(&thread).await
                {
                    let latest = crate::resume_history::latest(&history);
                    let superseded = retry.owed.native_origin.as_ref().is_none_or(|origin| {
                        latest
                            .turn_id
                            .is_some_and(|turn| turn != origin.turn_id.as_str())
                            || starts(&history, &origin.turn_id) > retry.starts
                    }) || matches!(
                        latest.point,
                        crate::resume_history::NativePoint::Superseded
                    );
                    if superseded {
                        Self::finish_renewal_ack_retry(&rt, &retry);
                        return;
                    }
                    // Another lifecycle contributor can still be finishing.
                    // A same-ID nonterminal history waits without any RPC.
                    if Self::renewal_ack_history(
                        &rt,
                        &retry.owed,
                        &history,
                        &RenewalAckCause::PersistedTerminal,
                    ) && retry
                        .owed
                        .native_origin
                        .as_ref()
                        .is_some_and(|origin| starts(&history, &origin.turn_id) == retry.starts)
                        && me.renewal_ack_current(&rt, &retry)
                        && write_through(me.sup.shared()).await
                        && me.renewal_ack_current(&rt, &retry)
                        && let Some(client) = me.sup.renewal_ack_client()
                    {
                        let id = retry.owed.renewal.as_ref().and_then(|renewal| {
                            renewal.receipt.as_deref().map(|id| (renewal.purpose, id))
                        });
                        if let Some((purpose, id)) = id {
                            let outcome = account_broker::status(&client, purpose, id).await;
                            let consumed = match outcome {
                                Ok(Outcome::Consumed(ref receipt)) => bound(receipt, &retry.owed),
                                Ok(Outcome::Committed(ref receipt))
                                    if bound(receipt, &retry.owed)
                                        && me.renewal_ack_current(&rt, &retry) =>
                                {
                                    confirmed_consumption(&client, &retry.owed).await
                                }
                                _ => false,
                            };
                            if consumed && me.renewal_ack_current(&rt, &retry) {
                                me.report_completion(&rt, &retry.owed, Seen::History(&history));
                            }
                            if consumed
                                && me.renewal_ack_current(&rt, &retry)
                                && me.sup.shared().discard_renewal(&retry.owed)
                            {
                                let _ = write_through(me.sup.shared()).await;
                                Self::finish_renewal_ack_retry(&rt, &retry);
                                return;
                            }
                        }
                    }
                }
                if !me.renewal_ack_current(&rt, &retry) {
                    Self::finish_renewal_ack_retry(&rt, &retry);
                    return;
                }
                delay = delay.saturating_mul(2).min(Duration::from_secs(30));
            }
        });
    }

    fn request_owner_revision(&self) -> Option<(u64, Option<u64>)> {
        let manager = self.auth_manager()?;
        let auth = manager.auth_cached()?;
        let revision = manager.auth_change_state_for(&auth)?;
        Some((
            revision.owner_generation,
            auth.is_api_key_auth().then_some(revision.generation),
        ))
    }
    /// Absence is unknown; only resolved incompatible routing or a loaded
    /// replacement identity revokes an old subscription recovery witness.
    fn renewal_source_revoked(&self, rt: &ThreadRt, renewal: &AccountHandoff) -> bool {
        let Some(config) = rt.config.as_ref() else {
            return false;
        };
        let provider = &config.model_provider;
        if !provider.is_openai()
            || !provider.requires_openai_auth
            || provider.env_key.is_some()
            || provider.experimental_bearer_token.is_some()
            || provider.auth.is_some()
            || provider.aws.is_some()
        {
            return true;
        }
        let Some(auth) = self
            .auth_manager()
            .and_then(|manager| manager.auth_cached())
        else {
            return false;
        };
        if auth.auth_mode() != codex_protocol::auth::AuthMode::Chatgpt {
            return true;
        }
        // Missing identity evidence is not a positive replacement.
        self.renewal_identity().is_some_and(|identity| {
            let observed = (identity.account.as_str(), identity.user.as_str());
            observed != renewal.source() && renewal.target() != Some(observed)
        })
    }

    /// Observe unknown cached auth without lending it execution authority. Only
    /// genuinely changed same-identity credentials promote the original scope.
    pub(super) fn observe_auth_wall(&self, rt: &ThreadRt) -> bool {
        let mut slot = lock(&rt.auth_observation);
        let Some(observation) = slot.as_ref() else {
            return false;
        };
        let manager = self.auth_manager();
        let valid = self.login_provider_eligible(rt)
            && manager
                .as_ref()
                .is_none_or(|manager| Arc::ptr_eq(manager, &observation.manager))
            && observation.scope.registered_to(&observation.manager)
            && lock(&rt.state)
                .native_origin
                .as_ref()
                .is_some_and(|origin| origin.turn_id == observation.scope.turn)
            && observation
                .cause
                .proves_cause(&observation.scope, &observation.scope.turn);
        if !valid {
            slot.take();
            return false;
        }
        if manager.is_none() {
            return true;
        }
        if observation.baseline.read(&observation.manager) == login::Read::Back {
            *lock(&rt.auth_wall_scope) = Some(Arc::clone(&observation.scope));
            slot.take();
            return true;
        }
        true
    }

    /// Native renewal can authenticate a currently known original profile even
    /// when its bearer is still the refused one. It cannot retry the local turn.
    pub(super) fn eligible_renewal_scope(&self, rt: &ThreadRt) -> bool {
        if self.eligible_login_scope(rt) {
            return true;
        }
        if !self.observe_auth_wall(rt) {
            return self.eligible_login_scope(rt);
        }
        lock(&rt.auth_observation)
            .as_ref()
            .is_some_and(|observation| {
                let before = *observation.manager.auth_change_state_receiver().borrow();
                let known = self
                    .auth_manager()
                    .is_some_and(|manager| Arc::ptr_eq(&manager, &observation.manager))
                    && observation
                        .scope
                        .snapshot()
                        .auth
                        .as_ref()
                        .is_some_and(|expected| {
                            login::request_profile(&observation.manager, expected) == Some(true)
                        })
                    && observation
                        .cause
                        .proves_cause(&observation.scope, &observation.scope.turn);
                known && before == *observation.manager.auth_change_state_receiver().borrow()
            })
            || self.eligible_login_scope(rt)
    }

    pub(super) fn login_provider_eligible(&self, rt: &ThreadRt) -> bool {
        let Some(config) = rt.config.as_ref() else {
            return false;
        };
        let provider = &config.model_provider;
        provider.is_openai()
            && provider.requires_openai_auth
            && provider.env_key.is_none()
            && provider.experimental_bearer_token.is_none()
            && provider.auth.is_none()
            && provider.aws.is_none()
    }

    pub(super) fn eligible_login_scope(&self, rt: &ThreadRt) -> bool {
        use codex_alab_core::renewal::RequestAuth;
        if !self.login_provider_eligible(rt) {
            return false;
        }
        let Some(auth) = self
            .auth_manager()
            .and_then(|manager| manager.auth_cached())
        else {
            return false;
        };
        if let Some(scope) = lock(&rt.auth_wall_scope).as_ref() {
            return match scope.snapshot().auth.as_ref() {
                Some(RequestAuth::ChatGpt { account, user }) => {
                    auth.auth_mode() == codex_protocol::auth::AuthMode::Chatgpt
                        && auth.get_account_id().as_ref() == Some(account)
                        && auth.get_chatgpt_user_id().as_ref() == Some(user)
                }
                Some(RequestAuth::OpenAiApiKey) => auth.is_api_key_auth(),
                None => false,
            };
        }
        // A restarted process cannot compare the predecessor manager's numeric
        // generations. Existing receipt/local approval and identity checks own
        // that transfer; an unproven saved login wall grants nothing.
        let identity = self.renewal_identity();
        self.sup.shared().owed_act().is_some_and(|owed| {
            owed.thread == rt.key
                && owed.renewal.is_some_and(|renewal| {
                    renewal.openai_scope
                        && identity.as_ref().is_some_and(|identity| {
                            let observed = (identity.account.as_str(), identity.user.as_str());
                            observed == renewal.source() || renewal.target() == Some(observed)
                        })
                })
        })
    }
    pub(super) async fn reconcile_transient_goal(&self, rt: &Arc<ThreadRt>) {
        let Some(pending) = lock(&rt.transient_auth_goal).clone() else {
            return;
        };
        let Some(runtime) = pending.runtime.upgrade() else {
            lock(&rt.transient_auth_goal).take();
            return;
        };
        let context = match runtime.alab_auth_goal_context(&pending.wall).await {
            Ok(context) => context,
            Err(_) => return,
        };
        if runtime.alab_auth_recovery_enabled()
            && context.as_ref().map(|goal| goal.as_deref()) == Some(pending.goal_id.as_deref())
        {
            return;
        }
        let removed = {
            let mut current = lock(&rt.transient_auth_goal);
            if current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &pending))
            {
                current.take();
                true
            } else {
                false
            }
        };
        if removed {
            runtime
                .alab_retire_transient_auth_recovery(pending.token)
                .await;
        }
    }

    /// Missing/changed proof is a positive revocation; a read failure is not.
    pub(super) async fn reconcile_renewal_goal(
        &self,
        rt: &Arc<ThreadRt>,
        owed: &OwedAct,
    ) -> Result<bool, String> {
        let Some(renewal) = &owed.renewal else {
            return Ok(true);
        };
        let Some(wall) = owed
            .native_origin
            .as_ref()
            .map(|origin| origin.turn_id.as_str())
        else {
            return Ok(false);
        };
        let Some(thread) = self.thread(rt.id).await else {
            return Err("renewal thread unavailable".into());
        };
        let Some(runtime) = thread
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
        else {
            let db = thread
                .state_db()
                .ok_or("renewal custody database unavailable")?;
            let store = db.thread_goals();
            let context = store
                .alab_auth_goal_context(rt.id, wall)
                .await
                .map_err(|error| error.to_string())?;
            if context.as_ref().map(|goal| goal.as_deref()) == Some(renewal.goal_id.as_deref())
                && !self.renewal_source_revoked(rt, renewal)
            {
                return Ok(true);
            }
            store
                .alab_retire_auth_recovery(rt.id, wall, renewal.goal_id.as_deref())
                .await
                .map_err(|error| error.to_string())?;
            self.sup.shared().discard_renewal(owed);
            let _ = write_through(self.sup.shared()).await;
            return Ok(false);
        };
        let context = runtime.alab_auth_goal_context(wall).await?;
        if context.as_ref().map(|goal| goal.as_deref()) == Some(renewal.goal_id.as_deref())
            && !self.renewal_source_revoked(rt, renewal)
        {
            return Ok(true);
        }
        // Preserve custody if clearing the durable deferral fails.
        runtime
            .alab_retire_auth_recovery(wall, renewal.goal_id.as_deref())
            .await?;
        self.sup.shared().discard_renewal(owed);
        let _ = write_through(self.sup.shared()).await;
        Ok(false)
    }

    /// Capture passive custody before a new intent is persisted. Disabled
    /// goals still need a witness so a subsequent human edit can revoke it.
    /// Missing original association never licenses downgrading a known goal.
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "this thread's async native lifecycle gate serializes SQLite retirement with accepted native IDs; no synchronous lock or network is held across the wait"
    )]
    pub(super) async fn renewal_goal_for_intent(
        &self,
        rt: &ThreadRt,
        thread: &CodexThread,
        wall: &str,
    ) -> Result<Option<String>, String> {
        // Retry a failed start-time cleanup only under the same authoritative
        // native gate. A stale A callback cannot retire current B's proof.
        {
            let native = rt.native_turn.lock().await;
            if native.as_deref() != Some(wall) {
                return Err("renewal native turn was superseded".into());
            }
            let db = thread
                .state_db()
                .ok_or("renewal custody database unavailable")?;
            db.thread_goals()
                .alab_native_auth_turn_started(rt.id, wall)
                .await
                .map_err(|error| error.to_string())?;
        }
        let origin = lock(&rt.auth_wall_goal_origin).clone();
        if let Some(runtime) = thread
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
        {
            if let Some(context) = runtime.alab_auth_goal_context(wall).await? {
                if origin.as_ref().is_some_and(|origin| origin != &context) {
                    return Err("renewal original goal association changed".into());
                }
                return Ok(context);
            }
            if runtime.alab_auth_recovery_enabled() || origin.flatten().is_some() {
                return Err("renewal original goal witness unavailable".into());
            }
            if !runtime.alab_capture_auth_standalone(wall).await? {
                return Err("renewal standalone context unavailable".into());
            }
            return Ok(None);
        }
        let db = thread
            .state_db()
            .ok_or("renewal custody database unavailable")?;
        let store = db.thread_goals();
        if let Some(context) = store
            .alab_auth_goal_context(rt.id, wall)
            .await
            .map_err(|error| error.to_string())?
        {
            if origin.as_ref().is_some_and(|origin| origin != &context) {
                return Err("renewal original goal association changed".into());
            }
            return Ok(context);
        }
        if origin.flatten().is_some() {
            return Err("renewal original goal witness unavailable".into());
        }
        if !store
            .alab_capture_auth_standalone(rt.id, wall)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err("renewal standalone context unavailable".into());
        }
        Ok(None)
    }

    pub(super) async fn acknowledge_renewal_at_stop(
        &self,
        rt: &Arc<ThreadRt>,
        turn_store: &ExtensionData,
    ) {
        let shared = self.sup.shared();
        let Some(owed) = shared.owed_act().filter(|owed| owed.thread == rt.key) else {
            return;
        };
        let Some(renewal) = owed
            .renewal
            .as_ref()
            .filter(|renewal| !renewal.local && renewal.accepted)
        else {
            return;
        };
        if renewal.receipt.is_none() {
            return;
        }
        let Some(epoch) = turn_store
            .get::<TurnTrack>()
            .and_then(|track| lock(&track.0).native_epoch)
        else {
            return;
        };
        if !self.renewal_ack_owned(rt, &owed, epoch) {
            return;
        }
        let Some(thread) = self.thread(rt.id).await else {
            return;
        };
        let Ok(history) = resumed_history(&thread).await else {
            return;
        };
        let cause = RenewalAckCause::NativeStop {
            store: turn_store,
            epoch,
        };
        if !Self::renewal_ack_history(rt, &owed, &history, &cause)
            || !self.renewal_ack_owned(rt, &owed, epoch)
        {
            return;
        }
        // This actual stop precedes TurnComplete persistence. It can acknowledge
        // existing admission, or retain a typed observer; retries require the
        // later durable terminal event before any native receipt exchange.
        if write_through(shared).await
            && self.renewal_ack_owned(rt, &owed, epoch)
            && let Some(client) = self.sup.renewal_ack_client()
            && confirmed_consumption(&client, &owed).await
        {
            self.report_completion(rt, &owed, Seen::Stop);
            if self.renewal_ack_owned(rt, &owed, epoch) && shared.discard_renewal(&owed) {
                let _ = write_through(shared).await;
            }
        } else {
            self.retry_renewal_ack(rt, owed, &history, cause);
        }
    }

    pub(super) fn same_file_auth_request(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        authority: &DecisionAuthority,
        runtime: Option<Arc<codex_goal_extension::GoalRuntimeHandle>>,
        request: codex_protocol::turn_input::RecoverTurnRequest,
        goal: Option<String>,
        act: Act,
    ) -> codex_core::WithTurnExtensionData<codex_protocol::turn_input::RecoverTurnRequest> {
        let wall = request.turn_id.clone();
        let me = Arc::clone(self);
        let rt = Arc::clone(rt);
        let policy = authority.policy.clone();
        let point = authority.point;
        let accepted = authority.accepted_turns;
        let offs = authority.offs;
        let mut init = codex_extension_api::ExtensionDataInit::new();
        init.insert(codex_alab_core::renewal::TurnAdmission::new(move |turn| {
            let me = Arc::clone(&me);
            let rt = Arc::clone(&rt);
            let runtime = runtime.clone();
            let wall = wall.clone();
            let goal = goal.clone();
            let act = act.clone();
            let policy = policy.clone();
            Box::pin(async move {
                let generation = me.sup.shared().admission_generation();
                let owner_revision = me.request_owner_revision();
                let current = || {
                    me.sup.acting()
                        && me.sup.shared().admission_generation() == generation
                        && me.eligible_login_scope(&rt)
                        && owner_revision.is_some()
                        && me.request_owner_revision() == owner_revision
                        && me.sup.offs() == offs
                        && me.sup.policy().as_ref() == Some(&policy)
                        && rt.accepted_turns.load(Ordering::SeqCst) == accepted
                        && rt.off_seen.load(Ordering::SeqCst) == offs
                        && lock(&rt.state).point_at() == point
                        && !me.upgrade_requested.load(Ordering::SeqCst)
                };
                if turn != wall
                    || !current()
                    || !me.login_for(&rt, false).1.back
                    || !matches!(me.sup.ready().await, Ready::Go)
                {
                    return Err("same-file login recovery changed".into());
                }
                let thread = me
                    .thread(rt.id)
                    .await
                    .ok_or("same-file thread unavailable")?;
                if me.renewal_goal_for_intent(&rt, &thread, &wall).await? != goal {
                    return Err("same-file goal context changed".into());
                }
                if goal.is_some()
                    && runtime
                        .as_ref()
                        .is_none_or(|runtime| !runtime.alab_auth_recovery_enabled())
                {
                    return Err("associated goal recovery is disabled".into());
                }
                let expected_identity = me.renewal_identity();
                if let Some(identity) = expected_identity.clone() {
                    let history = resumed_history(&thread)
                        .await
                        .map_err(|_| "same-file history unavailable")?;
                    let count = starts(&history, &wall);
                    if count == 0 || !current() {
                        return Err("same-file native origin changed".into());
                    }
                    let owed = OwedAct {
                        thread: rt.key.clone(),
                        native_origin: Some(codex_alab_core::session::NativeOrigin {
                            turn_id: wall.clone(),
                            phase: NativePhase::Complete,
                        }),
                        renewal: Some(AccountHandoff {
                            purpose: codex_alab_core::account_handoff::Purpose::Login,
                            destination: None,
                            limit: None,
                            openai_scope: true,
                            local: true,
                            account: identity.account,
                            user: identity.user,
                            receipt: None,
                            turn: Some(wall.clone()),
                            accepted: false,
                            starts: count,
                            goal_id: goal.clone(),
                        }),
                        rule: Some(words::RULE_LOGIN_BACK.into()),
                        text: Some(words::LOGIN_BACK.into()),
                        mode: Some(policy.autopilot.word().into()),
                        ..Default::default()
                    };
                    let shared = me.sup.shared();
                    if !shared.publish_renewal(shared.admission_generation(), owed)
                        || !write_through(shared).await
                    {
                        return Err("same-file recovery intent was not persisted".into());
                    }
                }
                let _permit = match runtime.as_ref() {
                    Some(runtime)
                        if !runtime.alab_auth_recovery_enabled() || expected_identity.is_some() =>
                    {
                        lock(&rt.transient_auth_goal).take();
                        Some(
                            runtime
                                .alab_auth_recovery_permit(&wall, goal.as_deref(), &turn)
                                .await?,
                        )
                    }
                    Some(runtime) => {
                        {
                            let mut pending = lock(&rt.transient_auth_goal);
                            let token = runtime.alab_begin_transient_auth_recovery()?;
                            *pending = Some(Arc::new(TransientGoalRecovery {
                                runtime: Arc::downgrade(runtime),
                                wall: wall.clone(),
                                goal_id: goal.clone(),
                                token,
                            }));
                        }
                        Some(
                            runtime
                                .alab_auth_recovery_permit_transient(&wall, goal.as_deref(), &turn)
                                .await?,
                        )
                    }
                    None => {
                        let db = thread
                            .state_db()
                            .ok_or("same-file custody database unavailable")?;
                        if goal.is_some()
                            || db
                                .thread_goals()
                                .alab_auth_goal_context(rt.id, &wall)
                                .await
                                .map_err(|error| error.to_string())?
                                != Some(None)
                        {
                            return Err("same-file standalone witness changed".into());
                        }
                        None
                    }
                };
                if !current()
                    || me.renewal_identity() != expected_identity
                    || !me.login_for(&rt, false).1.back
                {
                    return Err("same-file login recovery changed".into());
                }
                let readiness = me
                    .sup
                    .ready_for_admission()
                    .await
                    .map_err(|_| "same-file admission readiness changed".to_string())?;
                // This write lease linearizes the last proof check with
                // native admission, including writers in another process.
                // No network or await is allowed between it and admission.
                let db = thread
                    .state_db()
                    .ok_or("same-file custody database unavailable")?;
                let proof_lease = db
                    .thread_goals()
                    .alab_auth_admission_lease(rt.id, &wall, goal.as_deref())
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or("same-file admission witness changed")?;
                if !me.sup.admission_fence_current(&readiness)
                    || !current()
                    || me.renewal_identity() != expected_identity
                    || !me.login_for(&rt, false).1.back
                {
                    return Err("same-file admission witness changed".into());
                }
                if !me.sup.shared().admit_recovered_turn(&rt.key, &turn) {
                    return Err("same-file native point changed".into());
                }
                lock(&rt.state).acted(&act, clock(), &me.timing);
                me.sync_pace(&rt);
                drop(proof_lease);
                Ok(())
            })
        }));
        codex_core::WithTurnExtensionData::new(request, init)
    }

    fn renewal_identity(&self) -> Option<Identity> {
        let auth = self.auth_manager()?.auth_cached()?;
        if auth.auth_mode() != codex_protocol::auth::AuthMode::Chatgpt {
            return None;
        }
        Identity::from_native(&auth.get_account_id()?, &auth.get_chatgpt_user_id()?)
    }

    /// The host's freshly read word on recovery for `purpose`: the acting
    /// claim's connection and policy with the policy reply that permits it,
    /// a reply that was read and does not, or no reply to read. One gate for
    /// the recovery itself and for the in-turn question whether it would
    /// follow; only the recovery tells the last two apart.
    async fn recovery_host(&self, purpose: Purpose) -> RecoveryHost {
        self.first_recovery_host(&[purpose], purpose).await.1
    }

    /// [`Self::recovery_host`] for the first of `preferred` the host's one policy reply
    /// permits, else for `purpose`, which is also the purpose of a reply that permits none or
    /// was not read. One read either way: a lost login whose host prints no word for its move
    /// sends the same requests as before that move existed.
    async fn first_recovery_host(
        &self,
        preferred: &[Purpose],
        purpose: Purpose,
    ) -> (Purpose, RecoveryHost) {
        let (Some(client), Some(policy)) = (self.sup.renewal_client(), self.sup.policy()) else {
            return (purpose, RecoveryHost::Unread);
        };
        match client.harness_policy().await {
            Ok(codex_alab_aterm::policy::PolicyAnswer::Supported(remote)) => {
                match preferred
                    .iter()
                    .copied()
                    .chain([purpose])
                    .find(|purpose| policy_allows_recovery(&remote, *purpose))
                {
                    Some(permitted) => (
                        permitted,
                        RecoveryHost::Permits(Box::new((client, policy, remote))),
                    ),
                    None => (purpose, RecoveryHost::Refuses),
                }
            }
            Ok(codex_alab_aterm::policy::PolicyAnswer::Unsupported(_)) | Err(_) => {
                (purpose, RecoveryHost::Unread)
            }
        }
    }

    /// WHETHER THIS LOST LOGIN MAY BE MOVED TO ANOTHER ACCOUNT AT ALL, before the host is asked
    /// (`alab/design/accounts-lost-login.md`): the move was not declined since the last native
    /// start, and codex-alab's own reading of the refusal does not veto it (any reason but an
    /// expired token, from this turn, codex's held refusal, or a veto kept for the token).
    /// The host decides the rest, `refresh_token_expired` on its own record among it.
    fn login_rotation_open(&self, rt: &ThreadRt) -> bool {
        rt.login_rotation_declined.load(Ordering::SeqCst)
            != rt.accepted_turns.load(Ordering::SeqCst)
            && !self.login_move_vetoed(rt)
    }

    /// When this process began asking for the lost login's move of `turn`: now, at the first
    /// ask of it.
    fn login_rotation_asked_since(&self, rt: &ThreadRt, turn: &str) -> Instant {
        let mut asked = lock(&rt.login_rotation_asked);
        match asked.as_ref() {
            Some((asked_turn, since)) if asked_turn == turn => *since,
            _ => {
                let now = clock();
                *asked = Some((turn.to_owned(), now));
                now
            }
        }
    }

    /// Whether this process has asked for the lost login's move of `turn` for
    /// [`decide::Timing::login_rotation_ask`] with no receipt bound.
    fn login_rotation_lapsed(&self, rt: &ThreadRt, turn: &str) -> bool {
        clock().saturating_duration_since(self.login_rotation_asked_since(rt, turn))
            >= self.timing.login_rotation_ask
    }

    /// THE LOST LOGIN'S MOVE IS DECLINED: the host said no, or answered nothing it could act on,
    /// or no receipt came in time, or a veto now stands. The intent comes off the record
    /// (the goal's sign-in recovery stays: the login wall's own path carries it on), no move
    /// is asked for again until the next native start, and the point is looked at again at
    /// once, where the login wall goes the way it goes without a move: the same account's
    /// renewal (`codex-renewal`) where the host renews logins, else a person's sign-in.
    async fn decline_login_rotation(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        owed: &OwedAct,
        accepted: u64,
        why: &str,
    ) -> bool {
        rt.login_rotation_declined.store(accepted, Ordering::SeqCst);
        lock(&rt.login_rotation_asked).take();
        self.sup.note(format!(
            "{}: the lost login is not moved to another account ({why}); the login wall's own \
             path takes the point",
            rt.key
        ));
        let shared = self.sup.shared();
        if shared.discard_renewal(owed) {
            let _ = write_through(shared).await;
        }
        self.sup.end_handoff_seal(&rt.key);
        self.schedule(rt, clock());
        true
    }

    /// Whether ending this thread's running turn at the creditless usage wall
    /// `wall` would be followed by account rotation: the gates `evaluate` and
    /// `follow_renewal` apply before the host is asked to prepare one, the
    /// host's own word that it leaves such a wall (`leaves_wall`: a spent
    /// week, or a five-hour wall further from its reset than the bound the
    /// host advertises), and the host's pool showing an account to move to.
    /// Policy alone is not enough: a host that allows rotation over a pool
    /// with nowhere to go answers `waiting` to an ended turn, which no longer
    /// polls for an earned reset. Advisory; it selects no account, and every
    /// check runs again after the turn ends. A five-hour wall is refused with
    /// no request to a host whose remembered policy reply prints no bound. A
    /// yes ends the walled request, and the turn's handoff keeps `wall` as the
    /// wall that turn ends at, with the read's reset whatever the 429 named.
    ///
    /// That is the primary thread's answer: its turn is the one a move restarts
    /// on the other account. Any other root thread's turn (a side chat's) starts
    /// no move, and is answered by whether the session is already leaving
    /// ([`Self::session_leaves_account`]), with no request to the host.
    ///
    /// THE SESSION'S BACKGROUND TERMINALS HOLD IT (gap 1 of the audit of
    /// 2026-11-14, as its review amended it). The move's restart ends every
    /// unified-exec terminal (a dev server, a watch) the session runs, in any of
    /// its threads, and aterm's rest fence sees only the ones of the thread the
    /// TUI shows (its footer's count keeps the frame `working` and the record
    /// `busy`); a move never silently ends a running server. So while
    /// [`Self::session_background_terminals`] (core's count over the threads the
    /// thread manager lists) is above zero the answer is no: the turn keeps
    /// looking for an earned reset at its own cadence, and its first question
    /// after the terminals end is answered on the gates alone. Where every other
    /// gate says the move would follow (the pool's among them, which the word
    /// to the person reads), the person is told once a wall why it waits
    /// ([`Self::tell_terminals_hold`]); from then on, while the terminals still
    /// run, the question sends the host nothing. Another root thread's answer
    /// and the ask itself are held the same way
    /// ([`Self::session_leaves_account`], `follow_renewal`).
    pub(super) async fn quota_handoff_ready(&self, rt: &ThreadRt, wall: SpentWall) -> bool {
        let shared = self.sup.shared();
        let primary = shared.view().primary_thread;
        if primary.as_deref() != Some(rt.key.as_str()) {
            return self.session_leaves_account(rt, primary.as_deref()).await;
        }
        let standing = || {
            shared.view().primary_thread.as_deref() == Some(rt.key.as_str())
                && shared.is_registered(&rt.key)
                && !rt.stopped.load(Ordering::SeqCst)
                && !rt.vanilla()
                && !self.upgrade_requested.load(Ordering::SeqCst)
                && self.login_provider_eligible(rt)
                && self.renewal_identity().is_some()
        };
        if !standing() {
            return false;
        }
        let turn = rt.accepted_turns.load(Ordering::SeqCst);
        if rt.terminals_hold_told.load(Ordering::SeqCst) == turn
            && self.session_background_terminals().await > 0
        {
            return false;
        }
        // A HOST THAT LEAVES NO FIVE-HOUR WALL IS NOT ASKED ABOUT ONE. A creditless
        // wall asks this every 30 seconds for as long as it stands, and the
        // policy read below is a request to the host each time. The claim's
        // connection remembers the last policy reply it read: one that prints
        // no five-hour bound is refused here, as `leaves_wall` would refuse it
        // after the read. The host announces a policy that moves and the
        // supervisor reads it again, which replaces the remembered reply; a
        // spent week and every pass of `follow_renewal` read afresh too. With
        // none remembered the host is read; so is a host that prints the bound,
        // whose current word decides.
        if wall.window == SpentWindow::FiveHour
            && self
                .sup
                .renewal_client()
                .and_then(|client| client.policy())
                .is_some_and(|remembered| {
                    codex_alab_aterm::account_recovery::five_hour_bound(&remembered).is_none()
                })
        {
            return false;
        }
        let RecoveryHost::Permits(permitted) = self.recovery_host(Purpose::WeeklyQuota).await
        else {
            return false;
        };
        let (client, _, remote) = *permitted;
        // The host's word first: a host that would not leave this wall is not
        // asked for its pool. The wall's time to its reset is measured now,
        // after the policy read, not when the walled request read its usage.
        let asked_at = i64::try_from(now_ms() / 1000).unwrap_or(i64::MAX);
        if !(codex_alab_aterm::account_recovery::leaves_wall(&remote, wall, asked_at)
            && self.sup.acting()
            && standing())
        {
            return false;
        }
        // Read after the host's policy answer, which takes time: a terminal that
        // ended meanwhile holds nothing. The pool is read once either way: by the
        // hold's word to the person, which is said only where it shows somewhere
        // to go, or for the answer itself. A terminal that starts during that read
        // is held by the ask (`follow_renewal`), as one that starts after it is.
        let terminals = self.session_background_terminals().await;
        if terminals > 0 {
            self.tell_terminals_hold(&client, rt, turn, terminals).await;
            return false;
        }
        rotation_target_in_sight(&client).await && self.sup.acting() && standing()
    }

    /// THE SESSION'S BACKGROUND TERMINALS: how many run now in the threads the thread manager
    /// lists, as core keeps them (`CodexThread::list_background_terminals` over each thread
    /// `ThreadManager::list_thread_ids` names: the primary, every other root thread, a
    /// sub-agent's; it leaves out the internal ones, such as Guardian's, memory
    /// consolidation's or a review's, so their terminals are not counted), which the move's
    /// restart ends. The one reading every handoff path holds the usage
    /// wall's move on ([`Self::quota_handoff_ready`], [`Self::session_leaves_account`],
    /// `follow_renewal`). Not the TUI's footer count, which counts only the thread the TUI
    /// shows, and nothing where no frame hands one; that count stays the record's frame fact
    /// (`backgroundTerminals`, and the frame's `working`), which aterm's rest fence reads. A
    /// thread that leaves the manager between its listing and its read runs none. No thread
    /// manager (none was bound) counts none: no thread runs.
    pub(super) async fn session_background_terminals(&self) -> usize {
        #[cfg(test)]
        if let Some(count) = *lock(&self.terminals_seam) {
            return count;
        }
        let Some(manager) = lock(&self.thread_manager)
            .clone()
            .and_then(|weak| weak.upgrade())
        else {
            return 0;
        };
        let mut count = 0;
        for id in manager.list_thread_ids().await {
            if let Ok(thread) = manager.get_thread(id).await {
                count += thread.list_background_terminals().await.len();
            }
        }
        count
    }

    /// THE PERSON IS TOLD ONCE A WALL THAT THE MOVE WAITS FOR THE SESSION'S BACKGROUND
    /// TERMINALS, `count` of them now, whichever path the terminals held
    /// ([`Self::quota_handoff_ready`], [`Self::session_leaves_account`], `follow_renewal`):
    /// the journal, and a warning through the host's event sink (the app server sends it to
    /// the TUI as it does any extension's warning, as `/accounts use` says its refusal) in
    /// the history of EACH ROOT THREAD THE AUTOPILOT KEEPS (the TUI's own, which it
    /// registered). The wall and the move are the session's: every root thread runs on the
    /// walled account, and the restart ends them all. And the host routes a warning to the
    /// one thread it names: the TUI buffers one for a thread it does not show, unseen until
    /// the person switches to it. Said in every root thread, it is in the history of
    /// whichever root thread the person views, in any host, with no frame fact read; said
    /// only in a held thread, it would wait unseen while the person views another. A
    /// sub-agent's thread, which the person can view too, is never a root thread
    /// (`adopt_sub_agent`), so it is not told. `primary` is the primary thread and `wall` its
    /// walled turn (`accepted_turns` then): once for that wall, whichever thread was held.
    ///
    /// ONLY WHERE THE HOST'S POOL SHOWS ANOTHER ACCOUNT TO MOVE TO ([`rotation_target_in_sight`]
    /// over `client`, the claim's connection, read here for every path), as the line says
    /// one has room. The hold is kept with none, and costs nothing then: the ask it holds would
    /// be answered `not-needed`. But a person told the move waits for their terminals would
    /// stop a dev server for a move that never comes. Only the primary's question read the
    /// pool before it told; the follow pass and another root thread's question, whose move is
    /// live on the intent alone, told the same line with no read (the review of `af5f4fcc54`,
    /// its first finding: a turn whose 429 named no reset ended on its own, the follow pass
    /// made its intent from the usage wall alone, and the person was told with nowhere to
    /// go). The read is made at each held question or pass until the person is told for this
    /// wall, and at none after that.
    async fn tell_terminals_hold(
        &self,
        client: &codex_alab_aterm::Client,
        primary: &ThreadRt,
        wall: u64,
        count: usize,
    ) {
        if primary.terminals_hold_told.load(Ordering::SeqCst) == wall
            || !rotation_target_in_sight(client).await
            || primary.terminals_hold_told.swap(wall, Ordering::SeqCst) == wall
        {
            return;
        }
        self.sup.note(format!(
            "{}: the usage wall's move to another account waits until the session's background \
             terminals stop ({count} running), as the restart would end them",
            primary.key
        ));
        let Some(events) = self.events.get() else {
            return;
        };
        let shared = self.sup.shared();
        let roots = self
            .live_threads()
            .into_iter()
            .filter(|thread| {
                !thread.stopped.load(Ordering::SeqCst) && shared.is_root_thread(&thread.key)
            })
            .map(|thread| thread.key.clone());
        let mut told = std::collections::BTreeSet::new();
        for thread in std::iter::once(primary.key.clone()).chain(roots) {
            if told.insert(thread.clone()) {
                events.emit_warning(codex_extension_api::ExtensionWarning {
                    thread_id: thread,
                    turn_id: None,
                    message: MOVE_WAITS_FOR_TERMINALS.to_owned(),
                });
            }
        }
    }

    /// THE HOST REFUSED THE ACCOUNT HANDOFF ASKED FOR `turn` in its closed word `word`
    /// (`follow_renewal`): the point is kept and asked again at its recheck, as before, and
    /// the journal says so once per word of the series, where the refusal was dropped with
    /// no word. aterm answers `wall changed` to every ask under a record that does not rest,
    /// so one that stands would otherwise be one line every five seconds.
    fn note_handoff_refused(&self, rt: &ThreadRt, turn: &str, word: &str) {
        if lock(&rt.handoff_refusals).record(turn, word) {
            self.sup.note(format!(
                "{}: the host refused the account handoff ({word}); the point is kept and \
                 asked again",
                rt.key
            ));
        }
    }

    /// Whether the record holds the usage wall's move of the thread `primary` away from
    /// the login this process holds now: the intent `follow_renewal` publishes before it
    /// asks the host (`usage-rotate@v1`), unbound or bound to a receipt. An intent whose
    /// source is another login is not this process's to act on: the process the host
    /// started on the destination account holds one until its recovered turn is
    /// acknowledged.
    fn usage_move_stands(&self, primary: &str) -> bool {
        let Some(identity) = self.renewal_identity() else {
            return false;
        };
        self.sup.shared().owed_act().is_some_and(|owed| {
            owed.thread == primary
                && owed.rule.as_deref() == Some(Purpose::WeeklyQuota.rule())
                && owed.renewal.as_ref().is_some_and(|renewal| {
                    renewal.purpose == Purpose::WeeklyQuota
                        && !renewal.local
                        && renewal.source() == (identity.account.as_str(), identity.user.as_str())
                })
        })
    }

    /// WHETHER THE SESSION IS LEAVING THE ACCOUNT THIS WALLED TURN WAITS ON: the answer to
    /// a usage wall's handoff question for a root thread that is not the primary (a side
    /// chat). Ending such a turn starts no move, so it waits its wall out in quota
    /// recovery, looking for an earned reset every 30 seconds. But while it waits its turn
    /// runs, the session's record says `busy`, and aterm moves no account under a busy
    /// record: it refuses the primary thread's ask at `prepare`, in its preparation job, at
    /// `commit` and before its signal. One side chat left open at the wall would hold the
    /// primary's handoff off until the wall's reset.
    ///
    /// So the turn ends, on its own usage error, while a move is LIVE, and only then:
    ///
    /// - the record holds the primary thread's usage-wall move from this login
    ///   ([`Self::usage_move_stands`]);
    /// - codex-alab acts under its claim, and no upgrade's exec is under way;
    /// - the primary thread's last pass of `follow_renewal` kept its point and either sent
    ///   the host the ask (`prepare`) or holds the intent bound to the receipt the host
    ///   granted ([`ThreadRt::follows_move`]).
    ///
    /// Neither the intent nor a kept point is enough. The intent stays on record when a
    /// pass lets the point go (the host unread or no longer permitting the handoff),
    /// through an Autopilot Off and a lost claim, and a wall that names no reset never
    /// lapses it. And a pass keeps an unbound intent's point with no word to the host
    /// whenever its checks before an act say wait (a person's hold, a draft, a popup or
    /// queued input in the TUI, a person at the terminal, a `status` that cannot be read),
    /// while a turn runs on the primary thread, while the session's background terminals
    /// run, and when a read or a write of its own failed: nothing is asking the host then,
    /// and nothing will until that lifts. Ending a turn for a move nobody is asking for
    /// would give up its look for an earned reset for nothing, which is the one thing a
    /// parked turn is still good for. A bound receipt is
    /// the host's own grant, and its `commit` may have gone: the host then restarts the
    /// session on the other account, under a record at rest only, so a kept bound point
    /// counts whether or not this pass reached the host.
    ///
    /// A turn told no here is woken once the answer may be yes, and so is one whose looks
    /// ended before they came here: each look of a waiting turn leaves word that a turn
    /// waits as it begins ([`Self::walled_look_begins`]), and the primary's next pass that
    /// leaves the move live, while codex-alab acts, sends the wake
    /// ([`Self::note_move_followed`]).
    ///
    /// THE SESSION'S BACKGROUND TERMINALS HOLD IT TOO (gap 1 of the audit of 2026-11-14, as
    /// its review amended it): while one runs in any thread
    /// ([`Self::session_background_terminals`]) the move waits, as the restart would end it,
    /// so this turn ending for the move would give up its look for an earned reset for a
    /// move that is not coming yet. The answer is no; the person is told once a wall, where
    /// the host's pool shows another account to move to (read over the claim's connection by
    /// [`Self::tell_terminals_hold`]: the move is live on the intent alone, which no pool read
    /// made); and the first look after the terminals end, with the move live again, is
    /// answered yes.
    ///
    /// What it costs the turn: quota recovery asks this only after a look that found no
    /// earned reset to redeem, so a credit is still spent first, and a turn that waits its
    /// wall out ends on the same usage error at the reset anyway. Only a usage wall's move
    /// counts; a lost login's (`login-rotate@v1`) is not claimed here. Advisory: no request
    /// is sent, and the wall is not judged again for this thread (the intent is the
    /// session's decision about that account's wall, and the host re-checks everything at
    /// `prepare`). Memory reads, core's terminal count once they say leaving, and while the
    /// terminals hold a wall the person was not yet told of, one read of the host's pool.
    async fn session_leaves_account(&self, rt: &ThreadRt, primary: Option<&str>) -> bool {
        let Some(primary) = primary else {
            return false;
        };
        let Some(primary) = self.thread_rt(primary) else {
            return false;
        };
        let leaving = !rt.stopped.load(Ordering::SeqCst)
            && !rt.vanilla()
            && !self.upgrade_requested.load(Ordering::SeqCst)
            && self.sup.acting()
            && self.usage_move_stands(&primary.key)
            && !primary.stopped.load(Ordering::SeqCst)
            && primary.follows_move.load(Ordering::SeqCst);
        if !leaving {
            return false;
        }
        let terminals = self.session_background_terminals().await;
        if terminals > 0 {
            if let Some(client) = self.sup.renewal_client() {
                self.tell_terminals_hold(
                    &client,
                    &primary,
                    primary.accepted_turns.load(Ordering::SeqCst),
                    terminals,
                )
                .await;
            }
            return false;
        }
        let turn = rt.accepted_turns.load(Ordering::SeqCst);
        if rt.move_end_told.swap(turn, Ordering::SeqCst) != turn {
            self.sup.note(format!(
                "{}: its walled turn ends; the session is leaving this account",
                rt.key
            ));
        }
        true
    }

    /// A LOOK OF A WALLED TURN OF ANOTHER ROOT THREAD BEGINS (`WallHandoff::look_begins`,
    /// said at every look of quota recovery's wait, before it reads anything): leave word
    /// that a turn waits ([`Autopilot::move_awaited`]) for the primary's next pass that
    /// leaves its move live ([`Self::note_move_followed`]). At the look's start, not at its
    /// question ([`Self::session_leaves_account`]): a look can end before it asks (a credit
    /// list unread within a poll of the run's first unread one, a read that failed, an
    /// attempt out of time), and that turn waits on the next live pass as much as one told
    /// no. And before the look reads anything, so a pass that turns the answer to yes during
    /// it finds the word and wakes the turn, which would otherwise wait out its own 30
    /// seconds under a busy record. The primary thread's own looks leave none: its question
    /// is its own ([`Self::quota_handoff_ready`]), and a pass of its move does not answer it.
    pub(crate) fn walled_look_begins(&self, rt: &ThreadRt) {
        if self.sup.shared().view().primary_thread.as_deref() != Some(rt.key.as_str()) {
            self.move_awaited.store(true, Ordering::SeqCst);
        }
    }

    /// Record whether this pass of `follow_renewal` left `rt`'s handoff live
    /// ([`ThreadRt::follows_move`]), and WAKE THE TURNS THAT WAIT ON THE ANSWER. A live pass,
    /// with the usage wall's move from this login on record, is the moment another root
    /// thread's walled turn may end ([`Self::session_leaves_account`]). A turn that waits
    /// its wall out is told to look again now, where it would otherwise look at its next
    /// poll, up to 30 seconds on, with the record busy and every ask refused meanwhile.
    ///
    /// The wake goes only when a look of such a turn began since the last one
    /// ([`Self::walled_look_begins`]), whatever made the answer yes (the first ask, a hold
    /// lifted, a claim taken again after an Autopilot Off), and whatever that look did: it
    /// was told no, or it ended before it asked (its credit list unread). Neither a change
    /// of the mark nor the question alone would do. The mark a pass left before an Off is
    /// still set when codex-alab acts again, so a wake sent only when it turned from no to
    /// yes would never come for a turn told no under the Off. And a turn whose looks since
    /// the last wake all ended before the question asked nothing, so word left only by the
    /// question would never come for it, at the move's first live pass or after an Off.
    /// The pass has returned when this runs, so the turn's next question reads the mark set.
    ///
    /// Each conjunct saves backend reads, since a wake costs every waiting turn a usage
    /// read and a credit list read: a pass with no look since the last wake wakes nobody
    /// (every waiting turn looks after that wake anyway), and a pass for another handoff (a
    /// login's own renewal, a lost login's move), or one while codex-alab does not act (a
    /// bound receipt's point is kept then too), could only wake a turn to be told no again,
    /// at every such pass. The word that a turn waits is spent only by a wake that goes.
    pub(crate) fn note_move_followed(&self, rt: &ThreadRt, live: bool) {
        rt.follows_move.store(live, Ordering::SeqCst);
        if live
            && self.sup.acting()
            && self.usage_move_stands(&rt.key)
            && self.move_awaited.swap(false, Ordering::SeqCst)
        {
            self.sup.shared().note_account_move();
        }
    }

    /// THE USAGE WALL STAYS ON RECORD WHILE ITS HANDOFF IS ASKED FOR. This path owns
    /// the point until the host answers, so `evaluate` never decides the wall and
    /// never leaves its recovery (`sync_recovery`). Without one, the process after
    /// a restart holds the intent with no wall: no reset to stop asking at, and no
    /// point to resume from once the intent is released. The restart a session at
    /// an ask can take is a crash or a kill and aterm's relaunch, which restores
    /// the dead record's recovery where aterm is proven to have typed the line
    /// (`Relaunch::Recovery`). An adopted update's re-exec is not one: the ask's
    /// recheck is a timer that owes no act, and such a wait holds the live
    /// upgrade (`upgrade_facts`). So an intent no receipt binds carries the
    /// wall's recovery in the same record, written with it; the next process
    /// restores the wall as it restores any provider wait, and both the bound on
    /// asking and the wall's own resume read it there.
    ///
    /// A bound intent carries none. Once a receipt is bound the host may restart
    /// the session on the other account, and that process must not stand at the
    /// wall of the account it left; the bound is not judged for it either. The
    /// asked wall is taken off the record only until the recovered turn is
    /// reserved (`AccountHandoff::turn`, written to the record before that turn
    /// is admitted, so a restarted process reads it too). The recovered turn
    /// reuses the asked turn's id: from its reservation on, a usage wall on
    /// record under that id is one it met on the account it moved to, and stays.
    ///
    /// The wall this path puts or finds on record is marked as the asked one
    /// (`ThreadRt::asked_wall`). Released, the point's own wait replaces it; a
    /// point left to a person takes it off (`drop_asked_wall_left_to_a_person`).
    ///
    /// False only where a wall that should be on record could not be published;
    /// the caller retries before it asks the host. A point that holds no usage
    /// wall for the intent's turn (a process that restored none) carries nothing.
    fn carry_asked_wall(
        &self,
        rt: &ThreadRt,
        owed: &OwedAct,
        generation: codex_alab_core::session::AdmissionGeneration,
    ) -> bool {
        let Some(renewal) = owed
            .renewal
            .as_ref()
            .filter(|renewal| renewal.purpose == Purpose::WeeklyQuota)
        else {
            return true;
        };
        let Some(turn) = owed
            .native_origin
            .as_ref()
            .map(|origin| origin.turn_id.as_str())
        else {
            return true;
        };
        let shared = self.sup.shared();
        // A usage wall on record under the asked turn's id. Another turn's
        // recovery, or another kind of wall, is never this path's to touch.
        let carried = shared.recovery().is_some_and(|carried| {
            carried.thread == rt.key
                && carried.turn_id == turn
                && matches!(carried.wall, RecoveryWall::Usage { .. })
        });
        if renewal.receipt.is_some() {
            // Until the recovered turn is reserved that wall is the one the
            // handoff was asked at, on the account the session is leaving. Once
            // it is reserved, a wall under the same id is the recovered turn's
            // own: a bound intent stands until that turn is acknowledged.
            //
            // The whole wall leaves, whatever its clock. A wait whose clock
            // cannot be verified is kept where it is still this account's
            // (`Shared::clear_recovery_intent`); this one is the left account's,
            // and kept, the process on the destination would refuse it as an
            // unverifiable provider wait and stand at it.
            if carried && renewal.turn.is_none() {
                shared.set_recovery(None);
            }
            lock(&rt.asked_wall).take();
            return true;
        }
        if carried {
            // Published by an earlier pass, or restored from the record the
            // process before left: the asked wall either way.
            *lock(&rt.asked_wall) = Some(turn.to_owned());
            return true;
        }
        // The reset is carried as a moment of real time. The clock it is held on
        // stops while the host sleeps, so the sleep is counted before it is read.
        // Monotonic first: a pause before the epoch read can only move the carried
        // reset later, never before the wall's own.
        let now = clock();
        self.count_host_sleep(rt, now);
        let sample = self.retry_clock_sample();
        let epoch = now_ms();
        let snapshot = lock(&rt.state).clone();
        let previous = shared.recovery();
        let Some(mut recovery) = crate::recovery::capture_asked_wall(
            &rt.key,
            turn,
            &snapshot,
            now,
            epoch,
            sample.as_ref(),
            previous.as_ref(),
        ) else {
            return true;
        };
        if let RecoveryWall::Usage { quota, .. } = &mut recovery.wall {
            *quota = self.quota_scope(rt);
        }
        let published = shared.publish_recovery(generation, recovery, || {
            let current = lock(&rt.state);
            current.point_at() == snapshot.point_at()
                && current.point_turn_id == snapshot.point_turn_id
        });
        if published {
            *lock(&rt.asked_wall) = Some(turn.to_owned());
        }
        published
    }

    /// [`Follow::Keeps`] while this path owns the pending point. Failures retain its
    /// durable intent and a bounded wake; no peer text is logged or displayed.
    ///
    /// A SEAL ENDS WITH THE FOLLOW THAT KEPT ITS POINT (review of 06dfcad367, finding 1). Every
    /// way a pass lets the point go hands it to `evaluate`, and some leave the record's intent
    /// bound to its receipt: a host whose policy no longer permits the handoff, a login back in
    /// its own file, an identity that is neither end of the handoff, a scope no longer eligible.
    /// Nothing then reads that receipt's status again, so the seal on the claim would stand for
    /// as long as the claim did, and every act's checks, the very act the point went to among
    /// them, would wait on it. The seal ends there instead ([`Supervisor::end_handoff_seal`]).
    ///
    /// WHETHER THE PASS LEFT THE HANDOFF LIVE is recorded apart from whether it kept the
    /// point ([`Self::note_move_followed`]): many passes keep it and ask nothing. It is live
    /// where the pass kept the point and either sent the host the ask this pass or holds
    /// the intent bound to the receipt the host granted. An unbound point kept with no ask
    /// (a person's hold, a draft, the session's background terminals, a read that failed) is
    /// a move nobody is asking for.
    pub(super) async fn follow_renewal(self: &Arc<Self>, rt: &Arc<ThreadRt>) -> Follow {
        let asked = AtomicBool::new(false);
        let mut checked = false;
        let keeps = self.follow_renewal_point(rt, &asked, &mut checked).await;
        if !keeps {
            self.sup.end_handoff_seal(&rt.key);
        }
        let granted = || {
            self.sup
                .shared()
                .owed_act()
                .is_some_and(|owed| owed.thread == rt.key && receipt_bound(&owed))
        };
        self.note_move_followed(rt, keeps && (asked.load(Ordering::SeqCst) || granted()));
        if keeps {
            Follow::Keeps
        } else {
            Follow::Leaves { checked }
        }
    }

    /// A model-specific refusal requires a quota binding in the current request owner's
    /// generation. The refusal retains its kind even when that binding could not be made.
    fn quota_handoff_limit(
        &self,
        rt: &ThreadRt,
        policy: &codex_alab_aterm::policy::HarnessPolicy,
    ) -> Result<Option<String>, ()> {
        match lock(&rt.state)
            .point
            .as_ref()
            .and_then(|point| point.wall.as_ref())
        {
            // A restored ordinary intent can predate its in-memory point. A named intent
            // still needs an exact limit at the final prepare check.
            None => return Ok(None),
            Some(Wall::Usage {
                limit_class: LimitClass::Ordinary,
                ..
            }) => return Ok(None),
            Some(Wall::Usage {
                limit_class: LimitClass::Model,
                ..
            }) => {}
            _ => return Err(()),
        }
        let scope = self.quota_scope(rt).ok_or(())?;
        if account_broker::leaves_limits(policy)
            && codex_alab_core::wall_kind::valid_limit_id(&scope.bucket)
        {
            Ok(Some(scope.bucket))
        } else {
            Err(())
        }
    }

    /// One pass of [`Self::follow_renewal`]: true while it keeps the point. `asked` is set
    /// as the pass sends the host the ask (`prepare`) for an intent no receipt binds; every
    /// other way out leaves it clear, a kept point included. `checked` is set once the pass
    /// stands at a scope a renewal may be asked for ([`Follow::Leaves`]).
    async fn follow_renewal_point(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        asked: &AtomicBool,
        checked: &mut bool,
    ) -> bool {
        let shared = self.sup.shared();
        if shared.view().primary_thread.as_deref() != Some(rt.key.as_str()) {
            return false;
        }
        // Existing custody must be reconciled even when a new provider/auth
        // mode now forbids renewal, or remote renewal policy is unavailable.
        // Only proven revocation releases it; unknown reads keep the intent.
        if let Some(pending) = shared
            .owed_act()
            .filter(|owed| owed.thread == rt.key && owed.renewal.is_some())
        {
            match self.reconcile_renewal_goal(rt, &pending).await {
                Ok(true) => {}
                Ok(false) => return false,
                Err(_) => {
                    // A bound receipt's point is not left a minute for one read that failed.
                    let recheck = if receipt_bound(&pending) {
                        BOUND_RECHECK
                    } else {
                        RECHECK
                    };
                    self.schedule(rt, clock() + recheck);
                    return true;
                }
            }
        }
        if !self.eligible_renewal_scope(rt) {
            return false;
        }
        *checked = true;
        let existing = shared
            .owed_act()
            .filter(|owed| owed.thread == rt.key && owed.renewal.is_some());
        let purpose = existing
            .as_ref()
            .and_then(|owed| owed.renewal.as_ref())
            .map(|handoff| handoff.purpose)
            .or_else(|| match lock(&rt.state).point.as_ref()?.wall.as_ref()? {
                Wall::Login => Some(Purpose::Login),
                Wall::Usage { .. } => Some(Purpose::WeeklyQuota),
                _ => None,
            });
        let Some(purpose) = purpose else {
            return false;
        };
        if purpose == Purpose::WeeklyQuota
            && existing.is_none()
            && rt.quota_rotation_declined.load(Ordering::SeqCst)
                == rt.accepted_turns.load(Ordering::SeqCst)
        {
            return false;
        }
        // AN ASK FOR A LOST LOGIN'S MOVE NO RECEIPT BINDS IS BOUNDED, AND A VETO ENDS IT: no
        // `commit` was sent for it, so the session is still on the account the wall belongs
        // to, and the login wall's own path is all declining leaves.
        if let Some(asked) = existing.as_ref().filter(|owed| {
            owed.renewal.as_ref().is_some_and(|renewal| {
                renewal.purpose == Purpose::LoginRotation && renewal.receipt.is_none()
            })
        }) {
            let accepted = rt.accepted_turns.load(Ordering::SeqCst);
            let turn = asked
                .native_origin
                .as_ref()
                .map(|origin| origin.turn_id.as_str())
                .unwrap_or_default();
            if self.login_rotation_lapsed(rt, turn) {
                let why = format!(
                    "no receipt in {} s",
                    self.timing.login_rotation_ask.as_secs()
                );
                return self.decline_login_rotation(rt, asked, accepted, &why).await;
            }
            if self.login_move_vetoed(rt) {
                return self
                    .decline_login_rotation(rt, asked, accepted, "codex's own refusal vetoes it")
                    .await;
            }
        }
        // A LOST LOGIN WITH NO INTENT YET MAY BE MOVED TO ANOTHER ACCOUNT where the host's
        // policy reply says it moves one; otherwise, on that same reply, the login wall goes
        // as it went before such a move existed.
        let preferred: &[Purpose] =
            if purpose == Purpose::Login && existing.is_none() && self.login_rotation_open(rt) {
                &[Purpose::LoginRotation]
            } else {
                &[]
            };
        let (purpose, host) = self.first_recovery_host(preferred, purpose).await;
        // Read after the host's reply, which may have made the purpose a lost login's move.
        let recheck = point_recheck(purpose, existing.as_ref().is_some_and(receipt_bound));
        let (client, policy, remote) = match host {
            RecoveryHost::Permits(permitted) => {
                if rt.bound_host_unread.swap(false, Ordering::SeqCst) {
                    self.sup.note(format!(
                        "{}: the host reads again; the bound handoff is asked about",
                        rt.key
                    ));
                }
                *permitted
            }
            // A BOUND RECEIPT KEEPS THE POINT WHILE THE HOST CANNOT BE READ. `commit`
            // may have been sent for it, so the host may be moving the session to
            // the other account. Letting the point go here hands it to `evaluate`,
            // which decides the usage wall as this account's own: it writes that
            // wall back on the record and, at its reset, types the usage resume on
            // the account the session is leaving. One failed read proves nothing
            // about the handoff, so the point is kept and the host read again.
            // Nothing bounds this: a host that stays unread with its claim standing
            // holds the point as a bound receipt answered `waiting` does, and the
            // journal says so once. A read that times out is another case: it closes
            // the connection, and the reconnect owns the next read.
            //
            // Only a usage-limit handoff is kept. A login handoff has releases of
            // its own on a readable pass (a superseded turn, a sign-in in the same
            // file) that this arm would sit ahead of, and the harm it prevents, a
            // usage resume typed on the account being left, is that purpose's alone.
            //
            // An intent no receipt binds is not kept: no `commit` was sent for it,
            // the session is on the account the wall belongs to, and the wall's own
            // wait and resume are all a declined ask would leave. The bound on
            // asking (`rotation_ask_lapsed`) is judged after this read, so keeping
            // an unbound point here would hold it past the wall's reset for as
            // long as the host stayed unread. Its intent stays on record, and the
            // next pass that reads the host asks again.
            //
            // A lost login's move is kept the same way, bound or not: the account it is on
            // has no login to resume with, so `evaluate` would only leave the wall to a
            // person while the host is unread, and the ask is bounded
            // ([`Self::login_rotation_lapsed`]) without the host.
            RecoveryHost::Unread
                if purpose.moves_account()
                    && existing.as_ref().is_some_and(|owed| {
                        owed.renewal.as_ref().is_some_and(|renewal| {
                            renewal.receipt.is_some() || renewal.purpose == Purpose::LoginRotation
                        })
                    }) =>
            {
                if !rt.bound_host_unread.swap(true, Ordering::SeqCst) {
                    self.sup.note(format!(
                        "{}: the host cannot be read while a bound handoff stands; \
                         its point is kept and the host read again",
                        rt.key
                    ));
                }
                self.schedule(rt, clock() + recheck);
                return true;
            }
            // A host that no longer moves a lost login declines an ask no receipt binds:
            // the login wall's own path, which this host may still permit, is all that is
            // left, and an intent kept on record would hold it off.
            RecoveryHost::Refuses
                if purpose == Purpose::LoginRotation
                    && existing.as_ref().is_some_and(|owed| {
                        owed.renewal
                            .as_ref()
                            .is_some_and(|renewal| renewal.receipt.is_none())
                    }) =>
            {
                rt.bound_host_unread.store(false, Ordering::SeqCst);
                let Some(asked) = existing.as_ref() else {
                    return false;
                };
                let accepted = rt.accepted_turns.load(Ordering::SeqCst);
                return self
                    .decline_login_rotation(rt, asked, accepted, "the host no longer moves one")
                    .await;
            }
            // A reply that was read and forbids this recovery still releases the
            // point, receipt or not: a host with rotation (or renewal) switched off
            // will not finish the move, and the wall's own resume is what such a
            // host leaves the session.
            RecoveryHost::Unread | RecoveryHost::Refuses => {
                rt.bound_host_unread.store(false, Ordering::SeqCst);
                return false;
            }
        };
        let generation = shared.admission_generation();
        let accepted = rt.accepted_turns.load(Ordering::SeqCst);
        let offs = rt.off_seen.load(Ordering::SeqCst);
        let owner_revision = self.request_owner_revision();
        let current = || {
            self.sup.acting()
                && self.sup.policy().as_ref() == Some(&policy)
                && rt.accepted_turns.load(Ordering::SeqCst) == accepted
                && rt.off_seen.load(Ordering::SeqCst) == offs
                && owner_revision.is_some()
                && self.request_owner_revision() == owner_revision
                && shared.admission_generation() == generation
                && !self.upgrade_requested.load(Ordering::SeqCst)
                && !rt.stopped.load(Ordering::SeqCst)
        };
        let mut owed = shared
            .owed_act()
            .filter(|owed| owed.thread == rt.key && owed.renewal.is_some());
        if let Some(pending) = &owed {
            match self.reconcile_renewal_goal(rt, pending).await {
                Ok(true) => {}
                Ok(false) => return false,
                Err(_) => {
                    self.schedule(rt, clock() + recheck);
                    return true;
                }
            }
        }
        if let Some(previous) = &owed {
            let Some(thread) = self.thread(rt.id).await else {
                return true;
            };
            let Ok(items) = resumed_history(&thread).await else {
                self.schedule(rt, clock() + recheck);
                return true;
            };
            let native = crate::resume_history::latest(&items);
            if native.turn_id.is_some_and(|turn| {
                previous
                    .native_origin
                    .as_ref()
                    .is_some_and(|origin| origin.turn_id != turn)
            }) || matches!(native.point, crate::resume_history::NativePoint::Superseded)
            {
                if current() {
                    shared.discard_renewal(previous);
                    let _ = write_through(shared).await;
                }
                return false;
            }
        }
        if let Some(local) = owed
            .as_ref()
            .filter(|owed| owed.renewal.as_ref().is_some_and(|renewal| renewal.local))
        {
            return self.follow_local_renewal(rt, local.clone()).await;
        }
        if owed.as_ref().is_some_and(|owed| {
            owed.renewal
                .as_ref()
                .is_some_and(|renewal| !renewal.accepted)
        }) && matches!(purpose, Purpose::Login | Purpose::LoginRotation)
            && self.login_for(rt, true).1.back
        {
            return false;
        }
        if owed.is_none() {
            let state = lock(&rt.state).clone();
            // A file renewed in place retains the existing, identity-checked watcher.
            if matches!(purpose, Purpose::Login | Purpose::LoginRotation)
                && self.login_for(rt, true).1.back
            {
                return false;
            }
            let Some(identity) = self.renewal_identity() else {
                return false;
            };
            let Some(origin) = state
                .native_origin
                .clone()
                .filter(|origin| origin.phase == NativePhase::Complete)
            else {
                return false;
            };
            let Some(thread) = self.thread(rt.id).await else {
                return false;
            };
            let Ok(items) = resumed_history(&thread).await else {
                return false;
            };
            let count = starts(&items, &origin.turn_id);
            if count == 0 {
                return false;
            }
            let limit = if purpose == Purpose::WeeklyQuota {
                let Ok(limit) = self.quota_handoff_limit(rt, &remote) else {
                    return false;
                };
                limit
            } else {
                None
            };
            let goal_id = match self
                .renewal_goal_for_intent(rt, &thread, &origin.turn_id)
                .await
            {
                Ok(goal) => goal,
                Err(_) => {
                    self.schedule(rt, clock() + recheck);
                    return true;
                }
            };
            let text = if purpose == Purpose::Login {
                let decision = decide::login_back(&state, &policy, &self.timing, clock());
                let Act::Type { text, .. } = decision.act else {
                    return false;
                };
                if let Some(at) = decision.not_before.filter(|at| *at > clock()) {
                    self.schedule(rt, at);
                    return true;
                }
                text
            } else {
                "Continue the original task.".into()
            };
            owed = Some(OwedAct {
                thread: rt.key.clone(),
                native_origin: Some(origin),
                renewal: Some(AccountHandoff {
                    purpose,
                    destination: None,
                    limit,
                    openai_scope: true,
                    local: false,
                    account: identity.account,
                    user: identity.user,
                    receipt: None,
                    turn: None,
                    accepted: false,
                    starts: count,
                    goal_id,
                }),
                rule: Some(purpose.rule().into()),
                text: Some(text),
                mode: Some(policy.autopilot.word().into()),
                ..Default::default()
            });
            if purpose == Purpose::LoginRotation {
                self.sup.note(format!(
                    "{}: the lost login may be moved to another signed-in account; the host is \
                     asked to move it",
                    rt.key
                ));
            }
        }
        let Some(mut owed) = owed else {
            return false;
        };
        if !owed.renewal.as_ref().is_some_and(|renewal| {
            self.renewal_identity().is_some_and(|identity| {
                let observed = (identity.account.as_str(), identity.user.as_str());
                observed == renewal.source() || renewal.target() == Some(observed)
            })
        }) {
            return false;
        }
        if shared.turn_running_on(&rt.key) {
            return true;
        }
        // Reconfirm before the broker seals its claim authority. A second
        // readiness call after Ready would mutate the very claim it sealed, and
        // that holds for the whole window after the commit too: the host's commit
        // worker replays the sealed prepare against the claim's revision before it
        // signals this process, and a re-confirm moves that revision. So a pass
        // that finds a receipt bound sends no `meta set supervisor`: it reads the
        // receipt's status over the claim's connection (which keeps the
        // connection-bound claim held) until the host retires or consumes it, or
        // ends this process. The recovered turn a committed receipt admits in the
        // process the host started runs its own checks at its admission.
        let bound_receipt = receipt_bound(&owed);
        // The intent as it stands now, which this pass may have made or read again since.
        let recheck = point_recheck(purpose, bound_receipt);
        let pass = if bound_receipt {
            Pass::Sealed(self.sup.admission_observation())
        } else {
            match self.sup.ready_for_admission().await {
                Ok(readiness) => Pass::Confirmed(readiness),
                Err(_) => {
                    self.schedule(rt, clock() + recheck);
                    return true;
                }
            }
        };
        // Keep the original point durable before the first native request: the
        // intent, and beside an unbound one the usage wall it was asked at.
        if !current()
            || !shared.publish_renewal(generation, owed.clone())
            || !self.carry_asked_wall(rt, &owed, generation)
            || !write_through(shared).await
        {
            self.schedule(rt, clock() + WRITE_THROUGH_RETRY);
            return true;
        }
        if !pass.stands(&self.sup) || !current() {
            self.schedule(rt, clock() + recheck);
            return true;
        }
        let Some(renewal) = owed.renewal.as_ref() else {
            return false;
        };
        let Some(wall) = owed
            .native_origin
            .as_ref()
            .map(|origin| origin.turn_id.clone())
        else {
            return true;
        };
        let baseline_starts = renewal.starts;
        // The host is asked only until the wall's own reset. `waiting`, an
        // error or silence would otherwise be re-asked without end while the
        // account this session is on has long been able to run again. The
        // reset is a moment of real time on a clock that stops while the host
        // sleeps, and `evaluate` counts that sleep only after this function
        // lets the point go, so it is counted here first.
        let lapsed = purpose == Purpose::WeeklyQuota && {
            let now = clock();
            self.count_host_sleep(rt, now);
            let state = lock(&rt.state);
            let wall = state.point.as_ref().and_then(|point| point.wall.as_ref());
            rotation_ask_lapsed(
                wall,
                renewal.receipt.is_some(),
                now,
                self.timing.reset_grace,
            )
        };
        let outcome = if lapsed {
            self.sup.note(format!(
                "{}: the usage wall's reset came with no account handoff bound; the wall's own resume takes the point",
                rt.key
            ));
            // The host's own word for "nothing to hand off": the arm below
            // releases the intent exactly as it does for that answer.
            Ok(Outcome::NotNeeded)
        } else if let Some(id) = &renewal.receipt {
            account_broker::status(&client, renewal.purpose, id).await
        } else {
            // THE SESSION'S BACKGROUND TERMINALS HOLD THE ASK (gap 1 of the audit of
            // 2026-11-14, as its review amended it). The walled turn ended with none
            // running, but one can start after it, in any thread: aterm's rest fence
            // sees only the shown thread's, so an ask under a record at rest would be
            // granted and the restart would end the terminal silently. While one runs the
            // point is kept with no word to the host (the move is not live, so no other
            // root thread's walled turn ends for it), the person is told once a wall where
            // the host's pool shows somewhere to go (`tell_terminals_hold` reads it: this
            // intent may stand on the usage wall alone, for a turn no yes ended), and the
            // first pass after they end asks. A receipt already bound is the host's
            // own grant, whose `commit` may have gone: its `status` is read as before.
            if renewal.purpose == Purpose::WeeklyQuota {
                let terminals = self.session_background_terminals().await;
                if terminals > 0 {
                    self.tell_terminals_hold(&client, rt, accepted, terminals)
                        .await;
                    self.schedule(rt, clock() + recheck);
                    return true;
                }
            }
            if renewal.purpose == Purpose::LoginRotation {
                // The ask's span starts with its first ask in this process.
                self.login_rotation_asked_since(rt, &wall);
            }
            if !current() || !pass.stands(&self.sup) {
                self.schedule(rt, clock() + recheck);
                return true;
            }
            let source = Identity {
                account: renewal.account.clone(),
                user: renewal.user.clone(),
            };
            if renewal.purpose == Purpose::WeeklyQuota
                && (self.quota_handoff_limit(rt, &remote).as_ref() != Ok(&renewal.limit)
                    || self.renewal_identity().as_ref() != Some(&source))
            {
                Ok(Outcome::NotNeeded)
            } else {
                asked.store(true, Ordering::SeqCst);
                account_broker::prepare_with_limit(
                    &client,
                    renewal.purpose,
                    &owed.thread,
                    &wall,
                    &source,
                    renewal.limit.as_deref(),
                )
                .await
            }
        };
        if !pass.stands(&self.sup) || !current() {
            self.schedule(rt, clock() + recheck);
            return true;
        }
        let already_consumed = matches!(&outcome, Ok(Outcome::Consumed(_)));
        // A LOST LOGIN'S MOVE THE HOST DEFINITELY DECLINES. Asked with no receipt bound: it has
        // no account to move to (`not-needed`), knows no such verb, refuses the request or its
        // wall or purpose, or answers nothing this reader can take. A bound receipt, whose
        // `commit` may have gone, is declined only by the host retiring it. Every other
        // answer is asked about again: an unbound ask within its bound, a bound one until
        // the host retires or consumes it, as a usage wall's.
        if purpose == Purpose::LoginRotation {
            let why = match (&outcome, bound_receipt) {
                (Ok(Outcome::NotNeeded), false) => {
                    Some("the host has no account to move it to".to_owned())
                }
                (Ok(Outcome::Retired(receipt)), true) if bound(receipt, &owed) => {
                    Some("the host retired its receipt".to_owned())
                }
                (Err(error), false) if account_broker::login_rotation_declined(error) => {
                    Some(match error {
                        codex_alab_aterm::Error::Refused(word) => {
                            format!("the host refused: {word}")
                        }
                        _ => "the host's answer is unreadable".to_owned(),
                    })
                }
                _ => None,
            };
            if let Some(why) = why {
                return self.decline_login_rotation(rt, &owed, accepted, &why).await;
            }
        }
        match outcome {
            Ok(Outcome::NotNeeded) if purpose == Purpose::WeeklyQuota => {
                rt.quota_rotation_declined.store(accepted, Ordering::SeqCst);
                let Some(thread) = self.thread(rt.id).await else {
                    return true;
                };
                let goal = owed
                    .renewal
                    .as_ref()
                    .and_then(|handoff| handoff.goal_id.as_deref());
                let retired = if let Some(runtime) = thread
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>(
                ) {
                    runtime
                        .alab_retire_auth_recovery(&wall, goal)
                        .await
                        .map(|_| ())
                } else if let Some(db) = thread.state_db() {
                    db.thread_goals()
                        .alab_retire_auth_recovery(rt.id, &wall, goal)
                        .await
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                } else {
                    return true;
                };
                if retired.is_err() {
                    self.schedule(rt, clock() + recheck);
                    return true;
                }
                if current() && shared.discard_renewal(&owed) {
                    let _ = write_through(shared).await;
                    return false;
                }
            }
            Ok(Outcome::Ready(receipt)) if bound(&receipt, &owed) => {
                let Some(renewal) = owed.renewal.as_mut() else {
                    return true;
                };
                if !renewal.bind(
                    &receipt.id,
                    &receipt.destination.account,
                    &receipt.destination.user,
                    &wall,
                ) {
                    return true;
                }
                // The host sealed the claim as this pass's readiness re-confirmed it:
                // from the moment the record binds this receipt nothing re-confirms
                // it again. A sealed pass that reads `ready` once more (its `commit`
                // never reached the host) seals nothing new.
                if let Pass::Confirmed(fence) = &pass {
                    self.sup.seal_claim_for_handoff(fence, &rt.key, &receipt.id);
                }
                // The bound intent is written without the wall: `commit` follows
                // only a record the other account's process can start from.
                if shared.publish_renewal(generation, owed.clone())
                    && self.carry_asked_wall(rt, &owed, generation)
                    && write_through(shared).await
                    && pass.may_commit(&self.sup)
                    && current()
                {
                    // The native service derives the command and revalidates identity,
                    // the durable marker, human sequence, holds and policy atomically.
                    let _ = account_broker::commit(&client, receipt.purpose, &receipt.id).await;
                }
            }
            Ok(Outcome::Committed(receipt) | Outcome::Consumed(receipt))
                if bound(&receipt, &owed)
                    && self.renewal_identity().as_ref() == Some(&receipt.destination) =>
            {
                let Some(thread) = self.thread(rt.id).await else {
                    return true;
                };
                let Ok(items) = resumed_history(&thread).await else {
                    self.schedule(rt, clock() + recheck);
                    return true;
                };
                let native = crate::resume_history::latest(&items);
                let count = starts(&items, &wall);
                if count > baseline_starts
                    && native.turn_id == Some(wall.as_str())
                    && matches!(
                        native.point,
                        crate::resume_history::NativePoint::Complete(_)
                    )
                {
                    // A later start of this same ID, not its original failed start,
                    // reached a terminal boundary. Do not dispatch it twice.
                    let Some(renewal) = owed.renewal.as_mut() else {
                        return true;
                    };
                    renewal.accepted = true;
                    renewal.turn = Some(wall.clone());
                    if current()
                        && shared.publish_renewal(generation, owed.clone())
                        && write_through(shared).await
                        && confirmed_consumption(&client, &owed).await
                    {
                        self.report_completion(rt, &owed, Seen::History(&items));
                        if shared.discard_renewal(&owed) {
                            let _ = write_through(shared).await;
                        }
                        return false;
                    }
                    self.retry_renewal_ack(
                        rt,
                        owed.clone(),
                        &items,
                        RenewalAckCause::PersistedTerminal,
                    );
                } else if native.turn_id == Some(wall.as_str())
                    && (!already_consumed || count > baseline_starts)
                    // The process whose claim stands sealed for this handoff recovers nothing:
                    // the admission would only wait on the seal, once core had reserved a turn
                    // and written its settings for it. The host ends this process and starts
                    // the one that recovers the turn; past the seal's end this one may.
                    && !self.sup.claim_sealed()
                    && self
                        .recover_renewal(rt, thread, owed, Some(receipt))
                        .await
                {
                    // Its next pass is its own: a refused admission's retry, or the TUI's wait.
                    return true;
                }
            }
            Ok(Outcome::Retired(receipt)) if bound(&receipt, &owed) => {
                // A human/policy/generation change invalidates this receipt,
                // not the task. A fresh prepare must pass every native check.
                if let Some(renewal) = owed.renewal.as_mut() {
                    renewal.retire_selection();
                }
                // Unbound again: the wall goes back on record with the intent.
                if current()
                    && shared.publish_renewal(generation, owed.clone())
                    && self.carry_asked_wall(rt, &owed, generation)
                {
                    let _ = write_through(shared).await;
                }
            }
            Ok(Outcome::Waiting(Some(receipt))) if bound(&receipt, &owed) => {}
            // A refusal grants nothing either; it is said once a series.
            Err(codex_alab_aterm::Error::Refused(word)) => {
                self.note_handoff_refused(rt, &wall, &word);
            }
            // Invalid, stale and unavailable responses grant no execution. A
            // future same-file login can still be observed by the native watcher.
            _ => {}
        }
        self.schedule(rt, clock() + recheck);
        true
    }

    async fn follow_local_renewal(self: &Arc<Self>, rt: &Arc<ThreadRt>, owed: OwedAct) -> bool {
        let Some(renewal) = owed.renewal.as_ref() else {
            return false;
        };
        let identity = Identity {
            account: renewal.account.clone(),
            user: renewal.user.clone(),
        };
        if !renewal.local || self.renewal_identity().as_ref() != Some(&identity) {
            return true;
        }
        let shared = self.sup.shared();
        if shared.turn_running_on(&rt.key) {
            return true;
        }
        let Some(wall) = owed
            .native_origin
            .as_ref()
            .map(|origin| origin.turn_id.as_str())
        else {
            return true;
        };
        let Some(thread) = self.thread(rt.id).await else {
            return true;
        };
        let Ok(items) = resumed_history(&thread).await else {
            self.schedule(rt, clock() + RECHECK);
            return true;
        };
        let native = crate::resume_history::latest(&items);
        if starts(&items, wall) > renewal.starts
            && native.turn_id == Some(wall)
            && matches!(
                native.point,
                crate::resume_history::NativePoint::Complete(_)
            )
        {
            shared.discard_renewal(&owed);
            let _ = write_through(shared).await;
            return false;
        }
        if matches!(self.sup.ready().await, Ready::Go) && native.turn_id == Some(wall) {
            // No receipt binds this intent: a refusal is journaled with its reason and
            // schedules nothing, and the recheck below looks again.
            self.recover_renewal(rt, thread, owed, None).await;
        }
        self.schedule(rt, clock() + RECHECK);
        true
    }

    /// Recover the original native turn under `owed` (and `receipt`, where one binds it): core
    /// runs the admission below before it consumes any input. A start that did not go is
    /// journaled with its reason. True where this scheduled the point's next pass itself,
    /// which it does only under a bound receipt: after a refused admission and after a look
    /// the TUI's own wait stopped ([`admission_retry`] paces both).
    async fn recover_renewal(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        thread: Arc<CodexThread>,
        owed: OwedAct,
        receipt: Option<Receipt>,
    ) -> bool {
        let Some(wall) = owed
            .native_origin
            .as_ref()
            .map(|origin| origin.turn_id.clone())
        else {
            return false;
        };
        let bound = receipt.is_some();
        // THE TUI'S OWN WAIT IS READ BEFORE CORE IS ASKED. A person's draft, queued input, a
        // popup, an overlay or a key within the grace has the admission's readiness wait, and
        // it needs neither the host nor core to say so: asked anyway, core reserved the turn
        // and wrote its settings into the rollout at every look for as long as the draft stood.
        // The look that reads it is paced as a refusal is: no frame's change wakes this point,
        // so a wait scheduled at its own end (a draft's is RECHECK away) would leave a popup
        // that closed a second later a minute to be seen, and a wait that does stand is looked
        // at every RECHECK all the same. Never later than the wait's own end (a key's grace).
        if let Some((until, why)) = self
            .sup
            .policy()
            .and_then(|policy| self.sup.tui_wait(policy.human_grace))
        {
            let now = clock();
            let (waits, news, stood) = {
                let mut series = lock(&rt.admission_refusals);
                AdmissionRefusals::record(&mut series, wall, why, true, now)
            };
            let again = admission_retry(waits, stood).min(until.saturating_duration_since(now));
            if bound {
                self.schedule(rt, now + again);
            }
            if news {
                let again = if bound {
                    format!("the next pass comes in {} ms", again.as_millis())
                } else {
                    "its own recheck looks again".to_owned()
                };
                tracing::info!(
                    thread = %rt.key,
                    %why,
                    %again,
                    "the recovered turn waits on the TUI; core is not asked"
                );
                self.sup.note(format!(
                    "{}: the recovered turn waits ({why}); core is not asked, and {again}",
                    rt.key
                ));
            }
            return bound;
        }
        let me = Arc::clone(self);
        let pending = owed.clone();
        let admission_rt = Arc::clone(rt);
        let expected = wall.clone();
        let goal_runtime = thread
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>();
        let mut init = codex_extension_api::ExtensionDataInit::new();
        init.insert(codex_alab_core::renewal::TurnAdmission::new(move |turn| {
            let me = Arc::clone(&me);
            let rt = Arc::clone(&admission_rt);
            let mut pending = pending.clone();
            let expected = expected.clone();
            let identity = pending.renewal.as_ref().and_then(|renewal| renewal.target()).map(|(account, user)| Identity { account: account.into(), user: user.into() });
            let purpose = pending.renewal.as_ref().map_or(Purpose::Login, |renewal| renewal.purpose);
            let receipt = receipt.clone();
            let goal_runtime = goal_runtime.clone();
            Box::pin(async move {
                let shared = me.sup.shared();
                let generation = shared.admission_generation();
                let owner_revision = me.request_owner_revision();
                let policy = me.sup.policy();
                let Some(client) = me.sup.renewal_client() else { return Err("renewal claim missing".into()); };
                let remote = client.harness_policy().await.map_err(|_| "renewal policy unavailable".to_string())?;
                if !matches!(remote, codex_alab_aterm::policy::PolicyAnswer::Supported(ref p)
                    if policy_allows_recovery(p, purpose)) {
                    return Err("renewal policy changed".into());
                }
                if let Some(receipt) = &receipt {
                    let status = account_broker::status(&client, purpose, &receipt.id).await.map_err(|_| "renewal receipt unavailable".to_string())?;
                    if !matches!(status, Outcome::Committed(ref actual) | Outcome::Consumed(ref actual) if actual == receipt) {
                        return Err("renewal receipt changed".into());
                    }
                } else if !pending.renewal.as_ref().is_some_and(|renewal| renewal.local) {
                    return Err("renewal receipt missing".into());
                }
                let associated = pending.renewal.as_ref().and_then(|renewal| renewal.goal_id.clone());
                let _goal_permit = match goal_runtime.as_ref() {
                    Some(runtime) if associated.is_none() || runtime.alab_auth_recovery_enabled() =>
                        Some(runtime.alab_auth_recovery_permit(&expected, associated.as_deref(), &turn).await?),
                    Some(_) => return Err("original goal recovery is disabled".into()),
                    None => {
                        if associated.is_some() { return Err("original goal runtime unavailable".into()); }
                        let thread = me.thread(rt.id).await.ok_or("renewal thread unavailable")?;
                        let db = thread.state_db().ok_or("renewal custody database unavailable")?;
                        if db.thread_goals().alab_auth_goal_context(rt.id, &expected).await
                            .map_err(|error| error.to_string())? != Some(None) {
                            return Err("renewal standalone witness changed".into());
                        }
                        None
                    }
                };
                if turn != expected
                    || !me.eligible_renewal_scope(&rt)
                    || owner_revision.is_none() || me.request_owner_revision() != owner_revision
                    || me.renewal_identity() != identity
                    || !matches!(me.sup.ready().await, Ready::Go)
                    || me.upgrade_requested.load(Ordering::SeqCst)
                    || me.sup.policy() != policy
                    || shared.admission_generation() != generation
                {
                    return Err("renewal admission changed".into());
                }
                if shared.owed_act().as_ref() != Some(&pending) {
                    return Err("renewal intent changed".into());
                }
                let Some(renewal) = pending.renewal.as_mut() else {
                    return Err("renewal intent missing".into());
                };
                renewal.turn = Some(turn);
                if !shared.publish_renewal(generation, pending.clone())
                    || !write_through(shared).await
                {
                    return Err("renewal intent was not persisted".into());
                }
                let readiness = me.sup.ready_for_admission().await.map_err(|_| "renewal admission readiness changed".to_string())?;
                if !me.eligible_renewal_scope(&rt)
                    || me.request_owner_revision() != owner_revision
                    || me.renewal_identity() != identity
                    || me.upgrade_requested.load(Ordering::SeqCst)
                    || me.sup.policy() != policy
                    || shared.admission_generation() != generation
                    || shared.owed_act().as_ref() != Some(&pending)
                {
                    return Err("renewal changed during persistence".into());
                }
                // Source holds live on the host and can change while the writer waits.
                if let Some(receipt) = &receipt {
                    let status = account_broker::status(&client, purpose, &receipt.id).await
                        .map_err(|_| "renewal receipt unavailable".to_string())?;
                    if !matches!(status, Outcome::Committed(ref actual) | Outcome::Consumed(ref actual) if actual == receipt) {
                        return Err("renewal receipt changed".into());
                    }
                }
                if !matches!(me.sup.ready().await, Ready::Go)
                    || !me.eligible_renewal_scope(&rt)
                    || me.request_owner_revision() != owner_revision
                    || me.renewal_identity() != identity
                    || me.upgrade_requested.load(Ordering::SeqCst)
                    || me.sup.policy() != policy
                    || shared.admission_generation() != generation
                    || shared.owed_act().as_ref() != Some(&pending)
                { return Err("renewal admission changed".into()); }
                let thread = me.thread(rt.id).await.ok_or("renewal thread unavailable")?;
                let db = thread.state_db().ok_or("renewal custody database unavailable")?;
                let proof_lease = db.thread_goals().alab_auth_admission_lease(rt.id, &expected, associated.as_deref())
                    .await.map_err(|error| error.to_string())?.ok_or("renewal admission witness changed")?;
                if !me.sup.admission_fence_current(&readiness) || !me.sup.acting() || !me.eligible_renewal_scope(&rt)
                    || me.request_owner_revision() != owner_revision
                    || me.renewal_identity() != identity
                    || me.upgrade_requested.load(Ordering::SeqCst)
                    || me.sup.policy() != policy || shared.admission_generation() != generation
                    || shared.owed_act().as_ref() != Some(&pending) {
                    return Err("renewal admission witness changed".into());
                }
                if !shared.admit_recovered_turn(&pending.thread, &expected) {
                    return Err("renewal native point changed".into());
                }
                // This is the original turn's authorized automatic retry. Tag
                // its native start only after admission; no prompt is submitted.
                lock(&rt.state).acted(&Act::Type {
                    rule: words::RULE_LOGIN_BACK,
                    text: pending.text.clone().unwrap_or_else(|| words::LOGIN_BACK.into()),
                }, clock(), &me.timing);
                me.sync_pace(&rt);
                drop(proof_lease);
                Ok(())
            })
        }));
        let refused_wall = wall.clone();
        let request = codex_protocol::turn_input::RecoverTurnRequest {
            prompt_cache_ttl: thread.recovery_prompt_cache_ttl(&wall),
            turn_id: wall,
            thread_settings: Default::default(),
            trace: None,
            cyber_access_program: *lock(&rt.cyber_access_program),
        };
        // Native refusal returns here before consuming input or starting work, with the
        // admission's own reason (`alab-core/glue/renewal_admission.rs`). Keep the same
        // pending ID for the next bounded attempt, and say why this one did not go: an
        // unlogged refusal hid why the happy successor never reran its turn.
        match thread
            .recover_turn_if_idle(codex_core::WithTurnExtensionData::new(request, init))
            .await
        {
            Ok(codex_core::StartIfIdleSubmission::Started { .. }) => {
                *lock(&rt.admission_refusals) = None;
                false
            }
            // Core did not reserve the turn (a turn runs, plan mode, a drain): no admission
            // was asked, so nothing lengthens the wait and the receipt's own recheck looks
            // again.
            Ok(codex_core::StartIfIdleSubmission::NotSubmitted { reason }) => {
                let reason = format!("{reason:?}");
                let (_, news, _) = {
                    let mut series = lock(&rt.admission_refusals);
                    AdmissionRefusals::record(&mut series, refused_wall, &reason, false, clock())
                };
                if news {
                    tracing::info!(
                        thread = %rt.key,
                        %reason,
                        "core did not take the recovered turn; its receipt's recheck looks again"
                    );
                    self.sup.note(format!(
                        "{}: the recovered turn was not submitted ({reason}); its receipt's \
                         recheck looks again",
                        rt.key
                    ));
                }
                false
            }
            Err(error) => {
                let reason = error.to_string();
                let (refusals, news, stood) = {
                    let mut series = lock(&rt.admission_refusals);
                    AdmissionRefusals::record(&mut series, refused_wall, &reason, true, clock())
                };
                let retry = bound.then(|| admission_retry(refusals, stood));
                // Scheduled here, where the wait is decided, and before the journal says so:
                // whoever reads the line finds the timer it names.
                if let Some(retry) = retry {
                    self.schedule(rt, clock() + retry);
                }
                if news {
                    let again = retry.map_or_else(
                        || "its own recheck looks again".to_owned(),
                        |retry| format!("the next pass comes in {} ms", retry.as_millis()),
                    );
                    tracing::info!(
                        thread = %rt.key,
                        %reason,
                        %again,
                        "the recovered turn's admission was refused"
                    );
                    self.sup.note(format!(
                        "{}: the recovered turn's admission was refused ({reason}); {again}",
                        rt.key
                    ));
                }
                retry.is_some()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_limit_needs_its_current_quota_binding_and_an_advertised_host() {
        use crate::supervisor::SupervisorOptions;
        use codex_alab_aterm::policy::PolicyAnswer;

        let sup = Supervisor::new(SupervisorOptions {
            shared: Box::leak(Box::new(codex_alab_core::session::Shared::new())),
            endpoint: Arc::new(|| None),
            client: Default::default(),
        });
        let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
        let rt = ThreadRt::new(ThreadId::new(), None, false, Duration::ZERO);
        let mut state = at_usage_wall(clock(), None);
        let Some(Wall::Usage { limit_class, .. }) =
            state.point.as_mut().and_then(|point| point.wall.as_mut())
        else {
            panic!("expected a usage wall");
        };
        *limit_class = LimitClass::Model;
        rt.with_state(|current| *current = state);
        let PolicyAnswer::Supported(mut host) = PolicyAnswer::parse(
            "OK gen=1 effective=on codex_rotation=1 accounts=wall relaunch=true resume_limits=true codex_rotation_limits=1",
        ) else {
            panic!("expected a host policy");
        };
        let identity = QuotaIdentity {
            account: "source".into(),
            user: Some("user".into()),
            owner: 1,
        };
        *lock(&autopilot.quota_test_identity) = Some(identity.clone());
        let limit = |host: &codex_alab_aterm::policy::HarnessPolicy| {
            autopilot.quota_handoff_limit(&rt, host)
        };
        assert_eq!(limit(&host), Err(()));
        let scope = codex_alab_core::quota::Scope {
            account: identity.account.clone(),
            user: identity.user.clone(),
            bucket: "codex_other".into(),
            ..Default::default()
        };
        autopilot.restore_quota_scope(&rt, Some(&scope));
        assert_eq!(limit(&host), Ok(Some(scope.bucket.clone())));
        host.keys.remove("codex_rotation_limits");
        assert_eq!(limit(&host), Err(()));
        host.keys.insert("codex_rotation_limits".into(), "1".into());
        *lock(&autopilot.quota_test_identity) = Some(QuotaIdentity {
            owner: 2,
            ..identity
        });
        assert_eq!(limit(&host), Err(()));
        // Restoring this wait on another login still lets that login's authenticated
        // measurement wake the named bucket, but it did not receive this refusal.
        *lock(&autopilot.quota_test_identity) = Some(QuotaIdentity {
            account: "other".into(),
            user: Some("other-user".into()),
            owner: 3,
        });
        rt.with_state(|state| assert!(autopilot.restore_quota_wait(&rt, state, Some(&scope))));
        assert_eq!(limit(&host), Err(()));
        let rebound = autopilot.quota_scope(&rt).expect("watcher remains bound");
        assert_eq!(rebound.account, "other");
        assert_eq!(rebound.bucket, scope.bucket);
        assert!(matches!(
            lock(&rt.state)
                .point
                .as_ref()
                .and_then(|point| point.wall.as_ref()),
            Some(Wall::Usage {
                limit_class: LimitClass::Unverified,
                ..
            })
        ));
        rt.with_state(|current| *current = at_usage_wall(clock(), None));
        assert_eq!(limit(&host), Ok(None));
        rt.with_state(|current| current.point = None);
        assert_eq!(limit(&host), Ok(None));
    }

    /// A thread whose point is a usage wall met at `at`, naming `reset_at`.
    fn at_usage_wall(at: Instant, reset_at: Option<Instant>) -> ThreadState {
        let mut state = ThreadState::default();
        state.observe_person_turn();
        state.observe_turn_end(
            TurnEnd {
                at,
                by_autopilot: true,
                by_person: false,
                text: None,
                earlier: None,
                questions: false,
                worked: Duration::from_secs(5),
                wall: Some(Wall::Usage {
                    limit_class: LimitClass::Ordinary,
                    reset_at,
                    reset_label: None,
                }),
                aborted: false,
                ran_tools: false,
                restored: false,
            },
            &Timing::default(),
        );
        state
    }

    fn wall_of(state: &ThreadState) -> Option<&Wall> {
        state.point.as_ref().and_then(|point| point.wall.as_ref())
    }

    #[test]
    fn a_rotation_ask_lapses_only_past_the_walls_own_reset_with_no_receipt() {
        let start = Instant::now();
        let reset = start + Duration::from_secs(3600);
        let grace = Duration::from_secs(60);
        let due = reset + grace;
        let tick = Duration::from_nanos(1);
        let usage = Wall::Usage {
            limit_class: LimitClass::Ordinary,
            reset_at: Some(reset),
            reset_label: None,
        };
        assert!(
            rotation_ask_lapsed(Some(&usage), false, due, grace),
            "the reset and its grace have come and nothing is bound"
        );
        assert!(rotation_ask_lapsed(
            Some(&usage),
            false,
            due + Duration::from_secs(3600),
            grace
        ));
        // NEGATIVE CONTROLS, each one condition away from the lapse above.
        assert!(
            !rotation_ask_lapsed(Some(&usage), false, due - tick, grace),
            "one tick before the reset's grace ends"
        );
        assert!(
            !rotation_ask_lapsed(Some(&usage), false, reset, grace),
            "at the reset itself, inside the grace"
        );
        assert!(
            rotation_ask_lapsed(Some(&usage), false, reset, Duration::ZERO),
            "with no grace the reset is the moment"
        );
        assert!(
            !rotation_ask_lapsed(Some(&usage), true, due + Duration::from_secs(3600), grace),
            "a bound receipt is never abandoned"
        );
        let unnamed = Wall::Usage {
            limit_class: LimitClass::Ordinary,
            reset_at: None,
            reset_label: None,
        };
        assert!(
            !rotation_ask_lapsed(Some(&unnamed), false, due, grace),
            "a wall that names no reset has no such moment"
        );
        for other in [
            Wall::Login,
            Wall::Entitlement,
            Wall::Server {
                what: "an API error".into(),
                cut_off: false,
                retry_at: Some(reset),
            },
        ] {
            assert!(
                !rotation_ask_lapsed(Some(&other), false, due, grace),
                "{other:?}"
            );
        }
        assert!(
            !rotation_ask_lapsed(None, false, due, grace),
            "no wall this process saw (an intent carried across a restart)"
        );
    }

    /// The Autopilot's clock stops while the host sleeps. The ask's bound is
    /// judged after the sleep is counted at the point, so a reset the host
    /// slept through ends the ask at the wake, not as many hours later.
    #[test]
    fn a_reset_the_host_slept_through_lapses_the_ask_at_the_wake() {
        let start = Instant::now();
        let grace = Timing::default().reset_grace;
        let mut state = at_usage_wall(start, Some(start + Duration::from_secs(3600)));
        // Five waking minutes after the wall.
        let now = start + Duration::from_secs(300);
        let asleep = Duration::from_secs(5000);
        assert_eq!(state.slept_to(Some(asleep), now), None, "the mark");
        assert!(
            !rotation_ask_lapsed(wall_of(&state), false, now, grace),
            "the reset is 55 waking minutes ahead"
        );
        // The same reading again moves nothing: both callers may count.
        assert_eq!(state.slept_to(Some(asleep), now), None);
        assert!(!rotation_ask_lapsed(wall_of(&state), false, now, grace));
        // The host then slept two hours; the awake clock did not move.
        let slept = Duration::from_secs(2 * 3600);
        assert_eq!(state.slept_to(Some(asleep + slept), now), Some(slept));
        assert!(
            rotation_ask_lapsed(wall_of(&state), false, now, grace),
            "the reset passed during the sleep: {:?}",
            wall_of(&state)
        );
        assert!(
            !rotation_ask_lapsed(wall_of(&state), true, now, grace),
            "a bound receipt outlives the sleep too"
        );
        // NEGATIVE CONTROL: a sleep that ends before the reset's grace does not.
        let mut short = at_usage_wall(start, Some(start + Duration::from_secs(3600)));
        assert_eq!(short.slept_to(Some(asleep), now), None);
        let nap = Duration::from_secs(55 * 60);
        assert_eq!(short.slept_to(Some(asleep + nap), now), Some(nap));
        assert!(
            !rotation_ask_lapsed(wall_of(&short), false, now, grace),
            "the reset has come but its grace has not"
        );
        assert!(rotation_ask_lapsed(
            wall_of(&short),
            false,
            now + grace,
            grace
        ));
    }

    #[test]
    fn renewal_uses_the_native_renew_mode_instead_of_a_boolean_switch() {
        let mut policy = codex_alab_aterm::policy::HarnessPolicy {
            generation: 1,
            effective: true,
            why: "enabled".into(),
            keys: std::collections::BTreeMap::from([
                ("logins".into(), "renew".into()),
                ("relaunch".into(), "true".into()),
                ("resume_limits".into(), "true".into()),
            ]),
        };
        assert!(policy_allows_recovery(&policy, Purpose::Login));
        for word in ["true", "false", "off", "unknown"] {
            policy.keys.insert("logins".into(), word.into());
            assert!(!policy_allows_recovery(&policy, Purpose::Login), "{word}");
        }
        policy.keys.insert("logins".into(), "renew".into());
        policy.effective = false;
        assert!(!policy_allows_recovery(&policy, Purpose::Login));
    }

    #[test]
    fn quota_rotation_requires_host_capability_and_account_routing_not_login_renewal() {
        let mut policy = codex_alab_aterm::policy::HarnessPolicy {
            generation: 1,
            effective: true,
            why: "enabled".into(),
            keys: std::collections::BTreeMap::from([
                ("logins".into(), "off".into()),
                ("accounts".into(), "wall".into()),
                ("relaunch".into(), "true".into()),
                ("resume_limits".into(), "true".into()),
            ]),
        };
        assert!(!policy_allows_recovery(&policy, Purpose::WeeklyQuota));
        policy.keys.insert("codex_rotation".into(), "1".into());
        assert!(policy_allows_recovery(&policy, Purpose::WeeklyQuota));
        assert!(!policy_allows_recovery(&policy, Purpose::Login));
        for disabled in ["accounts", "relaunch", "resume_limits", "codex_rotation"] {
            let mut changed = policy.clone();
            changed.keys.insert(disabled.into(), "off".into());
            assert!(
                !policy_allows_recovery(&changed, Purpose::WeeklyQuota),
                "{disabled}"
            );
        }
    }

    /// A LOST LOGIN'S MOVE IS PERMITTED ONLY BY ITS OWN WORD (`codex_rotation_login=1`) beside
    /// a moving `accounts` policy and the relaunch and resume switches: aterm 0.199.0's policy
    /// (no such word) permits the usage wall's move and the same account's renewal as before,
    /// and never this one. The word permits nothing else, and a host that renews logins
    /// without it still renews.
    #[test]
    fn a_lost_logins_move_requires_its_own_word_and_a_moving_accounts_policy() {
        let mut policy = codex_alab_aterm::policy::HarnessPolicy {
            generation: 1,
            effective: true,
            why: "enabled".into(),
            keys: std::collections::BTreeMap::from([
                ("logins".into(), "renew".into()),
                ("codex_rotation".into(), "1".into()),
                ("accounts".into(), "wall".into()),
                ("relaunch".into(), "true".into()),
                ("resume_limits".into(), "true".into()),
            ]),
        };
        // aterm 0.199.0.
        assert!(!policy_allows_recovery(&policy, Purpose::LoginRotation));
        assert!(policy_allows_recovery(&policy, Purpose::Login));
        assert!(policy_allows_recovery(&policy, Purpose::WeeklyQuota));
        policy.keys.insert(
            codex_alab_aterm::account_recovery::LOGIN_ROTATION.into(),
            "1".into(),
        );
        assert!(policy_allows_recovery(&policy, Purpose::LoginRotation));
        for (key, off) in [
            ("accounts", "off"),
            ("relaunch", "false"),
            ("resume_limits", "false"),
            (codex_alab_aterm::account_recovery::LOGIN_ROTATION, "0"),
        ] {
            let mut changed = policy.clone();
            changed.keys.insert(key.into(), off.into());
            assert!(
                !policy_allows_recovery(&changed, Purpose::LoginRotation),
                "{key}={off}"
            );
        }
        let mut quota_off = policy.clone();
        quota_off.keys.remove("codex_rotation");
        quota_off.keys.insert("logins".into(), "off".into());
        assert!(policy_allows_recovery(&quota_off, Purpose::LoginRotation));
        assert!(!policy_allows_recovery(&quota_off, Purpose::WeeklyQuota));
        assert!(!policy_allows_recovery(&quota_off, Purpose::Login));
        policy.effective = false;
        assert!(!policy_allows_recovery(&policy, Purpose::LoginRotation));
        // The ask's own bound: four minutes of awake time with no receipt.
        assert_eq!(
            Timing::default().login_rotation_ask,
            Duration::from_secs(240)
        );
        assert_eq!(crate::decide::LOGIN_ROTATION_ASK, Duration::from_secs(240));
    }

    #[test]
    fn receipt_binds_identity_origin_and_exact_prepared_id() {
        let owed = OwedAct {
            thread: "thread".into(),
            native_origin: Some(codex_alab_core::session::NativeOrigin {
                turn_id: "wall".into(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                limit: None,
                openai_scope: true,
                local: false,
                account: "a".repeat(64),
                user: "b".repeat(64),
                receipt: Some("c".repeat(32)),
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: None,
            }),
            ..Default::default()
        };
        let receipt = Receipt {
            id: "c".repeat(32),
            thread: "thread".into(),
            wall: "wall".into(),
            purpose: Purpose::Login,
            destination: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
            identity: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
        };
        assert!(bound(&receipt, &owed));
        for changed in 0..8 {
            let mut wrong = receipt.clone();
            match changed {
                0 => wrong.id = "d".repeat(32),
                1 => wrong.thread = "other".into(),
                2 => wrong.wall = "other".into(),
                3 => wrong.identity.account = "d".repeat(64),
                4 => wrong.identity.user = "d".repeat(64),
                5 => wrong.destination.account = "d".repeat(64),
                6 => wrong.destination.user = "d".repeat(64),
                _ => wrong.purpose = Purpose::WeeklyQuota,
            }
            assert!(!bound(&wrong, &owed));
        }
    }

    #[test]
    fn original_failed_start_does_not_prove_renewal_acceptance() {
        let start = || {
            RolloutItem::EventMsg(EventMsg::TurnStarted(
                codex_protocol::protocol::TurnStartedEvent {
                    turn_id: "wall".into(),
                    turn_attribution: None,
                    root_turn_id: None,
                    trace_id: None,
                    started_at: None,
                    model_context_window: None,
                    collaboration_mode_kind: ModeKind::Default,
                },
            ))
        };
        let mut history = vec![start()];
        let before = starts(&history, "wall");
        assert_eq!(before, 1);
        assert_eq!(starts(&history, "wall"), before);
        history.push(start());
        assert!(starts(&history, "wall") > before);
        assert_eq!(starts(&history, "another"), 0);
    }
}
