// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

// Core runtime types form a recursive graph during auto-trait evaluation.
#![recursion_limit = "256"]

//! A local admission failure re-enters the real goal runtime and atomic start path.
//! Provider replies are owned mocks; no autopilot or live model is installed.

#[path = "autopilot/late_goal.rs"]
mod late_goal;

use std::sync::Arc;
use std::time::Duration;

use codex_core::config::Config;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_goal_extension::GoalObjectiveUpdate;
use codex_goal_extension::GoalRuntimeHandle;
use codex_goal_extension::GoalService;
use codex_goal_extension::GoalSetRequest;
use codex_goal_extension::GoalTokenBudgetUpdate;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::ThreadGoalStatus;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;

fn big_stack<F, Fut>(test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let outcome = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|error| panic!("owned runtime: {error}"))
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(60), test())
                        .await
                        .unwrap_or_else(|error| {
                            panic!("goal admission test hang detector: {error}")
                        });
                });
        })
        .unwrap_or_else(|error| panic!("owned test thread: {error}"))
        .join();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

struct Fixture {
    test: TestCodex,
    goals: Arc<GoalService>,
    runtime: Arc<GoalRuntimeHandle>,
}

impl Fixture {
    async fn new(server: &wiremock::MockServer) -> Self {
        codex_alab_core::activate();
        let late = Arc::new(late_goal::LateGoal::default());
        let mut registry = ExtensionRegistryBuilder::<Config>::new();
        late.register(&mut registry);
        let test = test_codex()
            .with_model("gpt-5.5")
            .with_extensions(Arc::new(registry.build()))
            .build(server)
            .await
            .unwrap_or_else(|error| panic!("real core fixture: {error}"));
        let goals = Arc::new(GoalService::new());
        late.bind(&test, Arc::clone(&goals)).await;
        let runtime = test
            .codex
            .thread_extension_data()
            .get::<GoalRuntimeHandle>()
            .unwrap_or_else(|| panic!("real goal runtime"));
        let fixture = Self {
            test,
            goals,
            runtime,
        };
        // Persist the goal without its ordinary immediate continuation. This is the
        // state left when an automatic admission failed before creating a turn.
        fixture.set_status(ThreadGoalStatus::Active, false).await;
        fixture
    }

    async fn set_status(&self, status: ThreadGoalStatus, apply: bool) {
        let db = self
            .test
            .codex
            .state_db()
            .unwrap_or_else(|| panic!("goal database"));
        let outcome = self
            .goals
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: self.test.session_configured.thread_id,
                    objective: GoalObjectiveUpdate::Set("Finish the standing goal once."),
                    status: Some(status),
                    token_budget: GoalTokenBudgetUpdate::Keep,
                    max_goal_token_budget: None,
                },
                async { Ok(()) },
            )
            .await
            .unwrap_or_else(|error| panic!("set goal under its mutation permit: {error}"));
        if apply {
            outcome.apply_runtime_effects(&self.goals).await;
        }
    }

    fn fail_admission(&self) {
        self.runtime
            .alab_record_goal_admission_failure(&CodexErr::Io(std::io::Error::from(
                std::io::ErrorKind::Interrupted,
            )));
    }

    async fn wait_until_retired(&self) {
        while self.runtime.alab_goal_admission_recovery_pending() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    async fn shutdown(&self) {
        self.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|error| panic!("owned core shutdown: {error}"));
    }
}

#[test]
fn transient_failure_coalesces_into_one_real_goal_continuation() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let mock = responses::mount_response_sequence(
            &server,
            vec![
                responses::sse_response(responses::sse(vec![responses::ev_completed("goal")]))
                    .set_delay(Duration::from_secs(30)),
            ],
        )
        .await;
        let fixture = Fixture::new(&server).await;
        for _ in 0..4 {
            fixture.fail_admission();
        }
        assert!(fixture.runtime.alab_goal_admission_recovery_pending());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            mock.requests().is_empty(),
            "retry must have a positive delay"
        );
        while mock.requests().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        fixture.wait_until_retired().await;
        assert_eq!(mock.requests().len(), 1);
        let input = serde_json::to_string(&mock.single_request().input()).expect("request input");
        assert!(input.contains("Finish the standing goal once."), "{input}");
        // Stop before the delayed response completes: ordinary goal turn-stop may
        // otherwise correctly admit another continuation of the still-active goal.
        fixture.set_status(ThreadGoalStatus::Paused, true).await;
        fixture.shutdown().await;
        assert_eq!(
            mock.requests().len(),
            1,
            "only one atomic start was admitted"
        );
    });
}

#[test]
fn a_goal_mutation_retires_the_pending_recheck() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let fixture = Fixture::new(&server).await;
        fixture.fail_admission();
        assert!(fixture.runtime.alab_goal_admission_recovery_pending());
        fixture.set_status(ThreadGoalStatus::Paused, true).await;
        assert!(!fixture.runtime.alab_goal_admission_recovery_pending());
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(
            responses::received_responses_requests(&server)
                .await
                .is_empty()
        );
        let db = fixture.test.codex.state_db().expect("goal database");
        let goal = db
            .thread_goals()
            .get_thread_goal(fixture.test.session_configured.thread_id)
            .await
            .expect("paused goal read")
            .expect("standing goal");
        assert_eq!(goal.status, codex_state::ThreadGoalStatus::Paused);
        fixture.shutdown().await;
    });
}

#[test]
fn intentional_deferral_is_observed_without_starting_or_rearming() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let fixture = Fixture::new(&server).await;
        let db = fixture.test.codex.state_db().expect("goal database");
        let goal = db
            .thread_goals()
            .get_thread_goal(fixture.test.session_configured.thread_id)
            .await
            .expect("active goal read")
            .expect("standing goal");
        db.thread_goals()
            .replace_thread_goal_snapshot(&goal)
            .await
            .expect("persist intentional fork continuation deferral");
        fixture.fail_admission();
        fixture.wait_until_retired().await;
        assert!(
            responses::received_responses_requests(&server)
                .await
                .is_empty()
        );
        assert!(
            db.thread_goals()
                .has_thread_goal_continuation_deferral(fixture.test.session_configured.thread_id)
                .await
                .expect("deferral is preserved")
        );
        fixture.shutdown().await;
    });
}

#[test]
fn a_permanent_admission_failure_never_arms_recovery() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let fixture = Fixture::new(&server).await;
        fixture.fail_admission();
        assert!(fixture.runtime.alab_goal_admission_recovery_pending());
        fixture
            .runtime
            .alab_record_goal_admission_failure(&CodexErr::Io(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied,
            )));
        assert!(!fixture.runtime.alab_goal_admission_recovery_pending());
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(
            responses::received_responses_requests(&server)
                .await
                .is_empty()
        );
        fixture.shutdown().await;
    });
}
