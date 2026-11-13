// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Follow native account recovery without replacing a running AuthManager.
//! The host restarts the process; only the committed
//! receipt permits recovery of the original native turn in the new process.

use super::account_broker;
use super::account_broker::Outcome;
use super::account_broker::Receipt;
use super::*;
use codex_alab_aterm::renewal::Identity;
use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
use codex_alab_core::session::AccountHandoff;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_alab_core::session::RecoveryWall;
use codex_history::RolloutItem;

use codex_alab_aterm::account_recovery::policy_allows as policy_allows_recovery;

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
async fn rotation_target_in_sight(client: &codex_alab_aterm::Client) -> bool {
    let Some(birth) = codex_alab_tui::identity::current()
        .ok()
        .and_then(|identity| codex_alab_tui::identity::proc_start_utc(&identity))
    else {
        return false;
    };
    client
        .accounts_snapshot(&birth)
        .await
        .is_ok_and(|pool| pool.another_account_can_run())
}

/// Whether an account handoff that no receipt binds has been asked for long
/// enough: the usage wall it was asked at named its own reset, and that reset
/// plus `grace` has come. From then the wall's ordinary resume is due, which
/// is all a `not-needed` answer would have left, so the ask ends and that
/// resume takes the point.
///
/// The reset is the one the 429 named, not the reset of the window the turn
/// was yielded for. For a five-hour wall the two agree to within a minute
/// (the yield requires it). For a spent week they need not: a 429 that named
/// an earlier reset (a five-hour window's) lapses the ask then, and the
/// resume is typed into a week that is still spent. That costs one model
/// request; it meets the wall again, and that turn's end asks for a handoff
/// again. The window's own reset is not carried to this point.
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
                let generation = me.sup.shared().upgrade_generation();
                let owner_revision = me.request_owner_revision();
                let current = || {
                    me.sup.acting()
                        && me.sup.shared().upgrade_generation() == generation
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
                    || !me.login_for(&rt).1.back
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
                    if !shared.publish_renewal(shared.upgrade_generation(), owed)
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
                    || !me.login_for(&rt).1.back
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
                    || !me.login_for(&rt).1.back
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
        let (Some(client), Some(policy)) = (self.sup.renewal_client(), self.sup.policy()) else {
            return RecoveryHost::Unread;
        };
        match client.harness_policy().await {
            Ok(codex_alab_aterm::policy::PolicyAnswer::Supported(remote)) => {
                if policy_allows_recovery(&remote, purpose) {
                    RecoveryHost::Permits(Box::new((client, policy, remote)))
                } else {
                    RecoveryHost::Refuses
                }
            }
            Ok(codex_alab_aterm::policy::PolicyAnswer::Unsupported(_)) | Err(_) => {
                RecoveryHost::Unread
            }
        }
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
    /// no request to a host whose remembered policy reply prints no bound.
    pub(super) async fn quota_handoff_ready(&self, rt: &ThreadRt, wall: SpentWall) -> bool {
        let shared = self.sup.shared();
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
        codex_alab_aterm::account_recovery::leaves_wall(&remote, wall, asked_at)
            && rotation_target_in_sight(&client).await
            && self.sup.acting()
            && standing()
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
    fn carry_asked_wall(&self, rt: &ThreadRt, owed: &OwedAct, generation: u64) -> bool {
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
        let epoch = now_ms();
        let snapshot = lock(&rt.state).clone();
        let Some(mut recovery) =
            crate::recovery::capture_asked_wall(&rt.key, turn, &snapshot, now, epoch)
        else {
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

    /// Returns true while this path owns the pending point. Failures retain its
    /// durable intent and a bounded wake; no peer text is logged or displayed.
    pub(super) async fn follow_renewal(self: &Arc<Self>, rt: &Arc<ThreadRt>) -> bool {
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
                    self.schedule(rt, clock() + RECHECK);
                    return true;
                }
            }
        }
        if !self.eligible_renewal_scope(rt) {
            return false;
        }
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
        // Quota preparation runs asynchronously in aterm and its observation
        // expires after a minute. Consume a ready result promptly instead of
        // making every native transition wait the ordinary policy-recheck period.
        let recheck = match purpose {
            Purpose::WeeklyQuota => Duration::from_secs(5),
            Purpose::Login => RECHECK,
        };
        if purpose == Purpose::WeeklyQuota
            && existing.is_none()
            && rt.quota_rotation_declined.load(Ordering::SeqCst)
                == rt.accepted_turns.load(Ordering::SeqCst)
        {
            return false;
        }
        let (client, policy) = match self.recovery_host(purpose).await {
            RecoveryHost::Permits(permitted) => {
                if rt.bound_host_unread.swap(false, Ordering::SeqCst) {
                    self.sup.note(format!(
                        "{}: the host reads again; the bound handoff is asked about",
                        rt.key
                    ));
                }
                let (client, policy, _) = *permitted;
                (client, policy)
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
            RecoveryHost::Unread
                if purpose == Purpose::WeeklyQuota
                    && existing.as_ref().is_some_and(|owed| {
                        owed.renewal
                            .as_ref()
                            .is_some_and(|renewal| renewal.receipt.is_some())
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
            // A reply that was read and forbids this recovery still releases the
            // point, receipt or not: a host with rotation (or renewal) switched off
            // will not finish the move, and the wall's own resume is what such a
            // host leaves the session.
            RecoveryHost::Unread | RecoveryHost::Refuses => {
                rt.bound_host_unread.store(false, Ordering::SeqCst);
                return false;
            }
        };
        let generation = shared.upgrade_generation();
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
                && shared.upgrade_generation() == generation
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
        }) && purpose == Purpose::Login
            && self.login_for(rt).1.back
        {
            return false;
        }
        if owed.is_none() {
            let state = lock(&rt.state).clone();
            // A file renewed in place retains the existing, identity-checked watcher.
            if purpose == Purpose::Login && self.login_for(rt).1.back {
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
        // readiness call after Ready would mutate the very claim it sealed.
        let readiness = match self.sup.ready_for_admission().await {
            Ok(readiness) => readiness,
            Err(_) => {
                self.schedule(rt, clock() + recheck);
                return true;
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
        if !self.sup.admission_fence_current(&readiness) || !current() {
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
            account_broker::prepare(
                &client,
                renewal.purpose,
                &owed.thread,
                &wall,
                &Identity {
                    account: renewal.account.clone(),
                    user: renewal.user.clone(),
                },
            )
            .await
        };
        if !self.sup.admission_fence_current(&readiness) || !current() {
            self.schedule(rt, clock() + recheck);
            return true;
        }
        let already_consumed = matches!(&outcome, Ok(Outcome::Consumed(_)));
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
                // The bound intent is written without the wall: `commit` follows
                // only a record the other account's process can start from.
                if shared.publish_renewal(generation, owed.clone())
                    && self.carry_asked_wall(rt, &owed, generation)
                    && write_through(shared).await
                    && self.sup.admission_fence_current(&readiness)
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
                {
                    self.recover_renewal(rt, thread, owed, Some(receipt)).await;
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
            self.recover_renewal(rt, thread, owed, None).await;
        }
        self.schedule(rt, clock() + RECHECK);
        true
    }

    async fn recover_renewal(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        thread: Arc<CodexThread>,
        owed: OwedAct,
        receipt: Option<Receipt>,
    ) {
        let Some(wall) = owed
            .native_origin
            .as_ref()
            .map(|origin| origin.turn_id.clone())
        else {
            return;
        };
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
                let generation = shared.upgrade_generation();
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
                    || shared.upgrade_generation() != generation
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
                    || shared.upgrade_generation() != generation
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
                    || shared.upgrade_generation() != generation
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
                    || me.sup.policy() != policy || shared.upgrade_generation() != generation
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
        let request = codex_protocol::turn_input::RecoverTurnRequest {
            prompt_cache_ttl: thread.recovery_prompt_cache_ttl(&wall),
            turn_id: wall,
            thread_settings: Default::default(),
            trace: None,
            cyber_access_program: *lock(&rt.cyber_access_program),
        };
        // Native refusal returns here before consuming input or starting work.
        // Keep the same pending ID for the next bounded attempt.
        let _ = thread
            .recover_turn_if_idle(codex_core::WithTurnExtensionData::new(request, init))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
