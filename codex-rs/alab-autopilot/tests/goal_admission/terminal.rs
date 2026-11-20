// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[path = "child_results.rs"]
mod child_results;

use super::*;
use codex_alab_core::assignment::Intent;
use codex_alab_core::assignment::Selection;
use codex_core::StartThreadOptions;
use codex_core::SuspendTurnOutcome;
use codex_core::TurnInputRequest;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::user_input::UserInput;
use codex_thread_store::LoadThreadHistoryParams;
use std::future::Future;
use std::future::poll_fn;
use std::task::Poll;
use tokio::time::timeout;

const DEADLINE: Duration = Duration::from_secs(10);
const PENDING_ASSIGNMENT: &str = "0123456789abcdef0123456789abcdef";

impl Fixture {
    async fn queue_automatic_goal(&self) {
        let thread = &self.test.codex;
        let id = self.test.session_configured.thread_id;
        let goal = thread
            .state_db()
            .expect("goal DB")
            .thread_goals()
            .get_thread_goal(id)
            .await
            .expect("read goal")
            .expect("standing goal");
        assert_eq!(goal.status, codex_state::ThreadGoalStatus::Active);

        // Defer model input in the real dispatcher while leaving control operations serviceable.
        let domain = self
            .test
            .thread_manager
            .auth_manager()
            .alab_assignment_domain();
        domain
            .request(Intent {
                operation: PENDING_ASSIGNMENT.into(),
                thread: id.to_string(),
                selection: Selection::Auto {
                    revision: "ab".repeat(32),
                },
            })
            .expect("fixture assignment entered Pending");

        // Only this goal submits between taking the uncommitted inventory and its invalidation.
        // On the fixture's single-thread executor the sender takes manager admission and puts
        // the op on the empty native channel in the same poll, then awaits its routing reply.
        let inventory = thread
            .prepare_upgrade_manager()
            .await
            .expect("quiescent submissions");
        assert!(inventory.valid());
        // Schedule the same native recovery that a transient automatic start error arms.
        self.fail_admission();
        assert!(self.runtime.alab_goal_admission_recovery_pending());
        timeout(DEADLINE, async {
            while inventory.valid() {
                assert!(
                    self.runtime.alab_goal_admission_recovery_pending(),
                    "goal recovery retired before queue acceptance"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("automatic goal did not enter Core's queue");
        drop(inventory);

        // This later FIFO acknowledgement proves the dispatcher already consumed the goal op.
        timeout(
            DEADLINE,
            thread.update_thread_settings(ThreadSettingsOverrides::default()),
        )
        .await
        .expect("settings acknowledgement timed out")
        .expect("settings acknowledged");
        assert!(
            self.runtime.alab_goal_admission_recovery_pending(),
            "queued goal already received a routing decision"
        );
        let mut probe = Box::pin(self.runtime.alab_automatic_input_permit());
        poll_fn(|cx| {
            assert!(
                probe.as_mut().poll(cx).is_pending(),
                "queued goal must retain the native goal permit"
            );
            Poll::Ready(())
        })
        .await;
    }

    fn reopen_assignment(&self) {
        self.test
            .thread_manager
            .auth_manager()
            .alab_assignment_domain()
            .cancel_pending(PENDING_ASSIGNMENT)
            .expect("reopen the fixture's account domain");
    }

    async fn assert_queued_goal_retired(&self) {
        let id = self.test.session_configured.thread_id;
        self.reopen_assignment();

        assert!(
            self.goals.alab_goal_runtime(id).is_none(),
            "original runtime still registered"
        );
        assert!(
            timeout(DEADLINE, self.runtime.alab_automatic_input_permit())
                .await
                .expect("retirement left a held goal permit")
                .is_err(),
            "retired runtime admitted work"
        );
        assert!(
            !self.runtime.alab_goal_admission_recovery_pending(),
            "retired goal scheduled another attempt"
        );
    }
}

#[test]
fn reopening_assignment_starts_the_queued_goal_once() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let mock = responses::mount_response_sequence(
            &server,
            vec![
                responses::sse_response(responses::sse_completed("goal"))
                    .set_delay(Duration::from_secs(120)),
            ],
        )
        .await;
        let fixture = Fixture::new(&server).await;
        let id = fixture.test.session_configured.thread_id;
        fixture.queue_automatic_goal().await;
        assert!(mock.requests().is_empty(), "held goal reached model");
        fixture.reopen_assignment();
        timeout(DEADLINE, async {
            while mock.requests().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            fixture.wait_until_retired().await;
        })
        .await
        .expect("released queue did not start the goal");
        let request = mock.single_request();
        assert_eq!(request.header("thread-id"), Some(id.to_string()));
        assert!(
            serde_json::to_string(&request.input())
                .expect("request input")
                .contains("Finish the standing goal once.")
        );
        fixture.set_status(ThreadGoalStatus::Paused, true).await;
        fixture.shutdown().await;
        assert_eq!(
            responses::received_responses_requests(&server).await.len(),
            1
        );
    });
}

#[test]
fn successful_suspension_discards_queued_goal_and_retires_its_permit() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let prompt = "Hold this original turn for suspension.";
        let mock = responses::mount_response_sequence(
            &server,
            vec![
                responses::sse_response(responses::sse_completed("original"))
                    .set_delay(Duration::from_secs(120)),
            ],
        )
        .await;
        let fixture = Fixture::new(&server).await;
        let thread = &fixture.test.codex;
        let id = fixture.test.session_configured.thread_id;
        let start = thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: prompt.into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("original turn");
        let TurnInputSubmission::Started { turn_id, .. } = start else {
            panic!("original turn did not start: {start:?}");
        };
        while mock.requests().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        fixture.queue_automatic_goal().await;
        let suspended = timeout(DEADLINE, thread.suspend_turn_and_shutdown())
            .await
            .expect("ordinary suspension blocked on goal retirement")
            .expect("ordinary suspension");
        assert_eq!(
            suspended,
            SuspendTurnOutcome::Suspended {
                turn_id: turn_id.clone()
            }
        );
        fixture.assert_queued_goal_retired().await;

        let requests = responses::received_responses_requests(&server).await;
        assert_eq!(requests.len(), 1, "queued goal reached the model");
        assert_eq!(requests[0].header("thread-id"), Some(id.to_string()));
        assert!(
            requests[0]
                .message_input_texts("user")
                .iter()
                .any(|text| text == prompt)
        );
        let history = fixture
            .test
            .thread_store
            .load_history(LoadThreadHistoryParams {
                thread_id: id,
                include_archived: false,
            })
            .await
            .expect("read original durable history");
        assert_eq!(history.thread_id, id);
        let started: Vec<_> = history
            .items
            .iter()
            .filter_map(|item| match item {
                codex_history::RolloutItem::EventMsg(EventMsg::TurnStarted(event)) => {
                    Some(event.turn_id.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(started, [turn_id.as_str()], "an extra turn was admitted");
    });
}

#[test]
fn tree_shutdown_retires_queued_goal_and_stops_only_its_members() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let fixture = Fixture::new(&server).await;
        let manager = &fixture.test.thread_manager;
        let root = &fixture.test.codex;
        let root_id = fixture.test.session_configured.thread_id;
        let member = manager
            .spawn_internal_session(
                root_id,
                StartThreadOptions {
                    session_source: Some(SessionSource::Internal(InternalSessionSource::Guardian)),
                    ..StartThreadOptions::new(fixture.test.config.clone())
                },
            )
            .await
            .expect("start a member of the same tree");
        assert_eq!(member.session_configured.parent_thread_id, Some(root_id));
        let other = manager
            .start_thread(StartThreadOptions::new(fixture.test.config.clone()))
            .await
            .expect("start an independent root on the same manager");
        assert_ne!(
            other.session_configured.session_id,
            member.session_configured.session_id
        );
        for thread in [root, &member.thread, &other.thread] {
            thread
                .update_thread_settings(ThreadSettingsOverrides::default())
                .await
                .expect("live thread acknowledged settings before the goal was queued");
        }

        fixture.queue_automatic_goal().await;
        manager
            .request_agent_tree_shutdown(root_id)
            .await
            .expect("request root tree shutdown")
            .wait_detailed()
            .await
            .expect("root and its member completed tree shutdown");
        fixture.assert_queued_goal_retired().await;
        for thread in [root, &member.thread] {
            thread.wait_until_terminated().await;
            let error = thread
                .update_thread_settings(ThreadSettingsOverrides::default())
                .await
                .expect_err("a stopped tree member still accepted work");
            assert!(
                matches!(error.details(), CodexErrorDetails::InternalAgentDied),
                "{error}"
            );
        }
        other
            .thread
            .update_thread_settings(ThreadSettingsOverrides::default())
            .await
            .expect("tree shutdown also terminated the independent root");
        other
            .thread
            .shutdown_and_wait()
            .await
            .expect("close independent root");
        assert!(
            responses::received_responses_requests(&server)
                .await
                .is_empty(),
            "a queued automatic goal reached the model"
        );
    });
}
