// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Headless exec retains its active request across account-quota walls.
//! Interactive consumers retain their existing policy and account recovery path.
//!
//! An ordinary wall, or a model's own limit, is waited out to its reset. A workspace's wall is
//! kept only while the run's host says a move may come (`codex_alab_core::exec_wall`, gap 4 of
//! `alab/PARALLEL-LOGINS.md`, step S3), from the exec tree's state its turn carries.

use super::*;
use crate::session::alab_exec_recovery_options::quota_observation;
use chrono::Utc;
use codex_alab_core::exec_wall::UNKNOWN_RESET_WAIT;
use std::time::Duration;

pub(super) fn eligible(err: &CodexErr, turn: &TurnContext) -> bool {
    reset_delay(
        codex_alab_core::active(),
        turn.extension_data
            .get::<codex_alab_core::quota::HeadlessExec>()
            .is_some()
            || turn
                .extension_data
                .get::<codex_alab_core::quota::ExecQuotaMember>()
                .is_some(),
        err,
        tree(turn).as_deref(),
        Utc::now(),
        std::time::Instant::now(),
    )
    .is_some()
}

/// The exec tree's observations and host-kept walls, as the turn carries them
/// (`quota_observation::bind`).
fn tree(turn: &TurnContext) -> Option<std::sync::Arc<quota_observation::Events>> {
    turn.extension_data.get::<quota_observation::Events>()
}

/// How long the refusal `err` waits before its retry, or `None` when it ends the request. Reads
/// only: the wait records its first wait itself.
fn reset_delay(
    active: bool,
    runtime_exec: bool,
    err: &CodexErr,
    tree: Option<&quota_observation::Events>,
    now: chrono::DateTime<Utc>,
    clock: std::time::Instant,
) -> Option<Duration> {
    if !active
        || !runtime_exec
        || err
            .http_status_code_value()
            .is_some_and(permanent_client_status)
    {
        return None;
    }
    let CodexErrorDetails::UsageLimitReached(quota) = err.details() else {
        return None;
    };
    let kind = codex_alab_quota_recovery::wall_kind(quota);
    if kind.host_kept() {
        // No reset of this account ends it, a named one included: only a move does.
        return tree?.keep(&kind, clock);
    }
    let Some(reset) = quota.resets_at else {
        return Some(UNKNOWN_RESET_WAIT);
    };
    Some(
        reset
            .signed_duration_since(now)
            .to_std()
            .unwrap_or_default(),
    )
}

pub(super) async fn wait(
    backoff: &mut CapacityBackoff,
    err: CodexErr,
    sess: &Session,
    turn: &TurnContext,
) {
    let tree = tree(turn);
    let clock = std::time::Instant::now();
    let Some(reset) = reset_delay(true, true, &err, tree.as_deref(), Utc::now(), clock) else {
        return;
    };
    let CodexErrorDetails::UsageLimitReached(details) = err.details() else {
        return;
    };
    let kind = codex_alab_quota_recovery::wall_kind(details);
    // A host-kept wait is the wall's own (`reset_delay` found the tree): its first is now taken.
    let kept = tree.filter(|_| kind.host_kept());
    if let Some(tree) = &kept {
        tree.walls().waited(&kind, clock);
    }
    // The live sampler, rather than a UI error string, identifies the exact
    // attempted account. This lease ends before a retry or on cancellation.
    let _quota_observation = quota_observation::publish(sess, turn, kind);
    let (delay, armed) = if kept.is_some() {
        // The keep's own length, which no provider advice or backoff stretches: a wait past it
        // would outlast the bound, or the first wait's cap.
        let deadline = Instant::now() + reset;
        (reset, codex_alab_core::wake::ArmedWait::new(deadline))
    } else {
        // A stale provider reset still needs positive backoff. Repeated quota errors
        // grow that backoff rather than spinning at the expired reset timestamp.
        let delay = reset.max(backoff.next_delay(rand::random()));
        let local_deadline = Instant::now() + delay;
        let armed = err.retry_after().map_or_else(
            || codex_alab_core::wake::ArmedWait::new(local_deadline),
            |advice| advice.arm_sleep_wait(Some(local_deadline)),
        );
        (delay, armed)
    };
    let observer = turn
        .extension_data
        .get::<InFlightRetryObserver>()
        .map(|observer| observer.waiting(armed.deadline().into_std()));
    tracing::info!(turn_id = %turn.sub_id, ?delay, "account quota exhausted; waiting to retry");
    sess.notify_stream_error(turn, "Waiting for account quota", err)
        .await;
    // The caller retains task cancellation. Reporting does not restart the wait,
    // and host sleep counts toward the provider's real-time reset deadline.
    let timer = armed.wait_observed(|deadline| {
        if let Some(observer) = &observer {
            let _ = observer.update(deadline.into_std());
        }
    });
    match &kept {
        // A grant the runtime holds for the root turn needs this wait's observation: the wait
        // ends when the grant is let go, or when its suspension cancels the turn.
        Some(tree) => {
            tree.walls().kept_wait(timer).await;
        }
        None => {
            timer.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_child_process;
    use super::*;
    use codex_alab_core::exec_wall::HOST_KEPT_BOUND;
    use codex_alab_core::exec_wall::Verdict;
    use codex_alab_core::wall_kind::Workspace;
    use codex_alab_quota_recovery::WallKind;
    use codex_protocol::error::UsageLimitReachedError;
    use codex_protocol::protocol::RateLimitReachedType;
    use codex_protocol::protocol::RateLimitSnapshot;

    fn quota_details(reset: Option<chrono::DateTime<Utc>>) -> UsageLimitReachedError {
        UsageLimitReachedError {
            plan_type: None,
            resets_at: reset,
            limit_window_minutes: None,
            rate_limits: None,
            promo_message: None,
            rate_limit_reached_type: None,
        }
    }

    fn quota(reset: Option<chrono::DateTime<Utc>>) -> CodexErr {
        CodexErr::UsageLimitReached(quota_details(reset))
    }

    /// A refusal at a wall of `kind`, naming `reset`, as codex-api states each kind: a
    /// workspace's word on the refusal and its `codex` bucket, spend control on that bucket,
    /// a model's limit by its limit id.
    fn refusal(kind: &WallKind, reset: Option<chrono::DateTime<Utc>>) -> CodexErr {
        let mut details = quota_details(reset);
        let bucket = |limit_id: &str, reached, spend_control| RateLimitSnapshot {
            limit_id: Some(limit_id.into()),
            limit_name: Some(limit_id.into()),
            normal_model_slug: None,
            primary: None,
            secondary: None,
            credits: None,
            individual_limit: None,
            spend_control_reached: spend_control,
            plan_type: None,
            rate_limit_reached_type: Some(reached),
        };
        let reached = match kind {
            WallKind::Ordinary | WallKind::Limit(_) => RateLimitReachedType::RateLimitReached,
            WallKind::Workspace(Workspace::OwnerCreditsDepleted) => {
                RateLimitReachedType::WorkspaceOwnerCreditsDepleted
            }
            WallKind::Workspace(Workspace::MemberCreditsDepleted) => {
                RateLimitReachedType::WorkspaceMemberCreditsDepleted
            }
            WallKind::Workspace(Workspace::OwnerUsageLimitReached) => {
                RateLimitReachedType::WorkspaceOwnerUsageLimitReached
            }
            WallKind::Workspace(Workspace::MemberUsageLimitReached) => {
                RateLimitReachedType::WorkspaceMemberUsageLimitReached
            }
            WallKind::Workspace(Workspace::SpendControl) => RateLimitReachedType::RateLimitReached,
        };
        details.rate_limit_reached_type = Some(reached);
        details.rate_limits = match kind {
            WallKind::Ordinary => None,
            WallKind::Limit(id) => Some(Box::new(bucket(id, reached, None))),
            WallKind::Workspace(Workspace::SpendControl) => {
                Some(Box::new(bucket("codex", reached, Some(true))))
            }
            WallKind::Workspace(_) => Some(Box::new(bucket("codex", reached, Some(false)))),
        };
        let error = CodexErr::UsageLimitReached(details);
        let CodexErrorDetails::UsageLimitReached(read) = error.details() else {
            unreachable!()
        };
        assert_eq!(&codex_alab_quota_recovery::wall_kind(read), kind);
        error
    }

    /// The exec tree of a run whose root turn is `root-turn`, and the subscription of its
    /// runtime, when one reads it.
    fn tree(
        subscribed: bool,
    ) -> (
        std::sync::Arc<quota_observation::Events>,
        Option<impl Sized>,
    ) {
        let events = crate::agent::LocalAgentControl::default()
            .runtime
            .exec_quota_events();
        events.walls().root_turn("root-turn");
        let runtime = subscribed.then(|| events.subscribe());
        (events, runtime)
    }

    #[test]
    fn only_active_exec_retries_recoverable_account_quota() {
        let now = chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let clock = std::time::Instant::now();
        let future = now + chrono::Duration::seconds(120);
        let error = quota(Some(future));
        let delay =
            |active, exec, error: &CodexErr| reset_delay(active, exec, error, None, now, clock);
        assert_eq!(delay(true, true, &error), Some(Duration::from_secs(120)));
        assert_eq!(delay(false, true, &error), None);
        assert_eq!(delay(true, false, &error), None);
        assert_eq!(delay(true, true, &quota(None)), Some(UNKNOWN_RESET_WAIT));
        for error in [
            CodexErr::QuotaExceeded,
            CodexErr::UsageNotIncluded,
            CodexErr::ServerOverloaded,
        ] {
            assert_eq!(delay(true, true, &error), None);
        }
        assert_eq!(delay(true, true, &quota(Some(now))), Some(Duration::ZERO));
        // A workspace's wall ends a run whose turn carries no exec tree, and one whose tree
        // no runtime reads, or whose host declined it; only a host's keep holds it.
        let (subscribed, _runtime) = tree(true);
        let (unread, _none) = tree(false);
        for kind in [
            RateLimitReachedType::WorkspaceOwnerCreditsDepleted,
            RateLimitReachedType::WorkspaceMemberCreditsDepleted,
            RateLimitReachedType::WorkspaceOwnerUsageLimitReached,
            RateLimitReachedType::WorkspaceMemberUsageLimitReached,
        ] {
            let mut details = quota_details(Some(future));
            details.rate_limit_reached_type = Some(kind);
            let error = CodexErr::UsageLimitReached(details);
            assert_eq!(delay(true, true, &error), None, "{kind:?}");
            assert_eq!(
                reset_delay(true, true, &error, Some(&unread), now, clock),
                None,
                "{kind:?}"
            );
            let CodexErrorDetails::UsageLimitReached(read) = error.details() else {
                unreachable!()
            };
            let wall = codex_alab_quota_recovery::wall_kind(read);
            subscribed.walls().waited(&wall, clock);
            subscribed
                .walls()
                .decide("root-turn", &wall, Verdict::Declined);
            assert_eq!(
                reset_delay(true, true, &error, Some(&subscribed), now, clock),
                None,
                "{kind:?}"
            );
            subscribed.walls().decide("root-turn", &wall, Verdict::Keep);
            assert_eq!(
                reset_delay(true, true, &error, Some(&subscribed), now, clock),
                Some(UNKNOWN_RESET_WAIT),
                "{kind:?}"
            );
        }
    }

    /// EXEC'S WAIT AT EACH KIND OF WALL, by whether a runtime reads the tree and what its host
    /// said: an ordinary wall and a model's own limit wait to their reset as before, whatever
    /// the tree; a workspace's wall waits only with a runtime, its first wait capped at
    /// `UNKNOWN_RESET_WAIT` whatever reset it names, its later waits only under the host's keep.
    #[test]
    fn a_walls_kind_runtime_and_verdict_decide_its_wait() {
        let now = chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let clock = std::time::Instant::now();
        // A spend cap's reset 20 days away, as the fixture's 429 names it.
        let far = now + chrono::Duration::days(20);
        let named = Duration::from_secs(20 * 86_400);
        let kinds = [
            WallKind::Ordinary,
            WallKind::Limit("codex_other".into()),
            WallKind::Workspace(Workspace::OwnerCreditsDepleted),
            WallKind::Workspace(Workspace::MemberCreditsDepleted),
            WallKind::Workspace(Workspace::OwnerUsageLimitReached),
            WallKind::Workspace(Workspace::MemberUsageLimitReached),
            WallKind::Workspace(Workspace::SpendControl),
        ];
        for kind in &kinds {
            for subscribed in [false, true] {
                for waited in [false, true] {
                    for verdict in [Verdict::Unknown, Verdict::Keep, Verdict::Declined] {
                        let (events, _runtime) = tree(subscribed);
                        if waited {
                            events.walls().waited(kind, clock);
                        }
                        events.walls().decide("root-turn", kind, verdict);
                        let case =
                            format!("{kind:?} subscribed={subscribed} waited={waited} {verdict:?}");
                        for (reset, ordinary) in
                            [(Some(far), Some(named)), (None, Some(UNKNOWN_RESET_WAIT))]
                        {
                            let error = refusal(kind, reset);
                            let wait = reset_delay(true, true, &error, Some(&events), now, clock);
                            let expected = if kind.host_kept() {
                                (subscribed
                                    && verdict != Verdict::Declined
                                    && (!waited || verdict == Verdict::Keep))
                                    .then_some(UNKNOWN_RESET_WAIT)
                            } else {
                                ordinary
                            };
                            assert_eq!(wait, expected, "{case} reset={reset:?}");
                            // Judging records nothing: the same refusal is judged the same.
                            assert_eq!(
                                reset_delay(true, true, &error, Some(&events), now, clock),
                                wait,
                                "{case}"
                            );
                        }
                    }
                }
            }
            // Without a tree on the turn, only a host-kept wall changes: it ends.
            let error = refusal(kind, Some(far));
            assert_eq!(
                reset_delay(true, true, &error, None, now, clock),
                (!kind.host_kept()).then_some(named),
                "{kind:?}"
            );
        }
    }

    /// THE CRITIQUE'S SEQUENCES, through the retry's own judgement and the wait's record: three
    /// and more workspace refusals in a row under the host's keep; one no host answers, which
    /// ends at the second; and the whole keep's bound, which a held grant lifts.
    #[test]
    fn consecutive_workspace_refusals_keep_only_while_the_host_does_within_the_bound() {
        let now = chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let start = std::time::Instant::now();
        let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
        let error = refusal(&credits, None);
        // One refusal judged as Core judges it (terminal? then retry?), then its wait taken.
        let refused = |events: &quota_observation::Events, clock| {
            let terminal = reset_delay(true, true, &error, Some(events), now, clock).is_none();
            let wait = reset_delay(true, true, &error, Some(events), now, clock);
            assert_eq!(terminal, wait.is_none());
            if wait.is_some() {
                events.walls().waited(&credits, clock);
            }
            wait
        };

        let (events, _runtime) = tree(true);
        for retry in 0..5u32 {
            let clock = start + UNKNOWN_RESET_WAIT * retry;
            assert_eq!(refused(&events, clock), Some(UNKNOWN_RESET_WAIT), "{retry}");
            events.walls().decide("root-turn", &credits, Verdict::Keep);
        }

        let (events, _runtime) = tree(true);
        assert_eq!(refused(&events, start), Some(UNKNOWN_RESET_WAIT));
        assert_eq!(refused(&events, start + UNKNOWN_RESET_WAIT), None);

        let (events, _runtime) = tree(true);
        assert_eq!(refused(&events, start), Some(UNKNOWN_RESET_WAIT));
        events.walls().decide("root-turn", &credits, Verdict::Keep);
        let last = start + HOST_KEPT_BOUND - Duration::from_secs(5);
        assert_eq!(refused(&events, last), Some(Duration::from_secs(5)));
        assert_eq!(refused(&events, start + HOST_KEPT_BOUND), None);
        let hold = events.walls().hold("root-turn");
        assert_eq!(
            refused(&events, start + HOST_KEPT_BOUND * 2),
            Some(UNKNOWN_RESET_WAIT)
        );
        drop(hold);
        assert_eq!(refused(&events, start + HOST_KEPT_BOUND * 2), None);
    }

    /// THE REAL RETRY AT A WORKSPACE WALL, on a real exec turn: the turn carries its tree's walls
    /// from its binding; the retry is judged and waited through Core's own entry points; the
    /// first wait is 30 s, not the 20 days the refusal names; an unanswered wall then ends; the
    /// host's keep holds it; and a held grant keeps the wait past its timer until it is let go.
    #[test]
    fn the_real_retry_at_a_workspace_wall_keeps_only_while_its_host_may_move_it() {
        if !activated("the_real_retry_at_a_workspace_wall_keeps_only_while_its_host_may_move_it") {
            return;
        }
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let (session, turn) = crate::session::tests::make_session_and_context().await;
                session
                    .services
                    .thread_extension_data
                    .insert(codex_alab_core::quota::HeadlessExec);
                turn.extension_data
                    .insert(codex_alab_core::quota::HeadlessExec);
                quota_observation::bind(&session, &turn);
                let events = super::tree(&turn).expect("an exec turn carries its tree's walls");
                assert!(std::sync::Arc::ptr_eq(
                    &events,
                    &session.services.local_agent_runtime.exec_quota_events()
                ));
                let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
                let error = || refusal(&credits, Some(Utc::now() + chrono::Duration::days(20)));
                assert!(!eligible(&error(), &turn), "no runtime reads the tree");
                let runtime = events.subscribe();
                assert!(eligible(&error(), &turn), "the first wait");

                tokio::time::pause();
                let mut backoff = CapacityBackoff::default();
                let start = Instant::now();
                wait(&mut backoff, error(), &session, &turn).await;
                let waited = start.elapsed();
                assert!(
                    waited >= UNKNOWN_RESET_WAIT
                        && waited < UNKNOWN_RESET_WAIT + Duration::from_secs(1),
                    "{waited:?}"
                );
                assert!(
                    !eligible(&error(), &turn),
                    "unanswered, it ends at its second"
                );
                events.walls().decide(&turn.sub_id, &credits, Verdict::Keep);
                assert!(eligible(&error(), &turn), "the host's keep");

                let hold = events.walls().hold(&turn.sub_id);
                let waiting = wait(&mut backoff, error(), &session, &turn);
                tokio::pin!(waiting);
                assert!(
                    tokio::time::timeout(UNKNOWN_RESET_WAIT * 3, &mut waiting)
                        .await
                        .is_err(),
                    "a held grant outlasts the wait's timer"
                );
                drop(hold);
                tokio::time::timeout(Duration::from_secs(1), waiting)
                    .await
                    .expect("the wait ends with the grant");
                drop(runtime);
                assert!(!eligible(&error(), &turn), "the runtime is gone");
            });
    }

    /// AN EXEC REVIEW'S OWN TURN IS ITS TREE'S ROOT TURN: only its delegate samples, so the
    /// review task's start names it (`exec_review_stream.rs` `begin`). The runtime asks about
    /// the review's turn (`root_options`), so a grant held for it, and the host's keep for it,
    /// keep the delegate's workspace wall past its second refusal.
    #[test]
    fn an_exec_reviews_own_turn_is_the_root_turn_its_walls_are_kept_for() {
        if !activated("an_exec_reviews_own_turn_is_the_root_turn_its_walls_are_kept_for") {
            return;
        }
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let (session, review, _events) =
                    crate::session::tests::make_session_and_context_with_rx().await;
                session
                    .services
                    .thread_extension_data
                    .insert(codex_alab_core::quota::HeadlessExec);
                let cancel = tokio_util::sync::CancellationToken::new();
                let _consumer = crate::tasks::review_recovery::begin(&session, &review, &cancel)
                    .expect("an exec review keeps its consumer");
                let events = session.services.local_agent_runtime.exec_quota_events();
                assert!(std::sync::Arc::ptr_eq(
                    &events,
                    &super::tree(&review).expect("the review's turn carries its tree's walls")
                ));
                let _runtime = events.subscribe();
                let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
                let clock = std::time::Instant::now();
                // The delegate's first refusal is waited; unanswered, its second would end it.
                assert_eq!(events.keep(&credits, clock), Some(UNKNOWN_RESET_WAIT));
                events.walls().waited(&credits, clock);
                assert_eq!(events.keep(&credits, clock), None);
                let hold = events.walls().hold(&review.sub_id);
                assert!(events.walls().granted(), "a grant for the review's turn");
                assert_eq!(events.keep(&credits, clock), Some(UNKNOWN_RESET_WAIT));
                drop(hold);
                events
                    .walls()
                    .decide(&review.sub_id, &credits, Verdict::Keep);
                assert_eq!(
                    events.walls().verdict(&credits),
                    Verdict::Keep,
                    "the host's keep for the review's turn"
                );
                assert_eq!(events.keep(&credits, clock), Some(UNKNOWN_RESET_WAIT));
            });
    }

    /// Whether this is the child process that runs this module's test `test` with codex-alab
    /// active. Otherwise the test is run in such a child and must pass there. Activation lives
    /// in a child so upstream tests retain their own behavior.
    fn activated(test: &str) -> bool {
        const CHILD: &str = "ALAB_EXEC_WORKSPACE_WALL_TEST";
        if std::env::var_os(CHILD).is_some() {
            codex_alab_core::activate();
            return true;
        }
        let name = format!("{}::{test}", module_path!().split_once("::").unwrap().1);
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test child parent runtime")
            .block_on(async {
                test_child_process::run(command, Duration::from_secs(60))
                    .await
                    .unwrap_or_else(|failure| panic!("{name}: {failure}"))
                    .assert_test_success(&name);
            });
        false
    }
}
