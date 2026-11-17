// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real manager/Core/GoalRuntime path, using the existing integration
//! fixture. Opening this pure Source inhibitor is not a Native frontier proof.
use super::*;
use codex_alab_core::retained_goal_gate::Gate;
use codex_alab_core::retained_goal_gate::Owner;
use std::future::Future as _;
use std::task::Poll;

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

#[test]
fn actual_runtime_blocks_fresh_goal_input_and_drains_its_automatic_window() {
    big_stack(|| async {
        let (owner, gate, initial) = Owner::new_held();
        let server = responses::start_mock_server().await;
        let mock = responses::mount_response_sequence(
            &server,
            vec![
                responses::sse_response(responses::sse(vec![responses::ev_completed(
                    "retained-goal",
                )]))
                .set_delay(Duration::from_secs(30)),
            ],
        )
        .await;
        // with_thread_manager runs before the fixture's first genuine Core
        // start. LateGoal then consumes that actual thread extension Gate.
        let fixture = Fixture::new_with_gate(&server, Some(gate.clone())).await;
        let result: Result<(), String> = async {
            let actual = fixture
                .test
                .codex
                .thread_extension_data()
                .get::<Gate>()
                .ok_or("actual Core thread has no retained gate")?;
            require(
                actual.same(&gate),
                "Core did not retain the manager's exact gate",
            )?;
            require(
                fixture.runtime.alab_automatic_input_permit().await.is_err(),
                "a held actual GoalRuntime admitted fresh automatic input",
            )?;
            fixture
                .runtime
                .alab_recheck_after_retained_frontier()
                .await?;
            require(
                mock.requests().is_empty(),
                "held native goal continuation reached the model",
            )?;

            owner.release_if(&initial, || Ok(())).await?;
            let permit = fixture.runtime.alab_automatic_input_permit().await?;
            let mut replacement = std::pin::pin!(owner.hold());
            let pending = std::future::poll_fn(|context| {
                Poll::Ready(matches!(replacement.as_mut().poll(context), Poll::Pending))
            })
            .await;
            require(
                pending,
                "replacement hold did not drain the actual automatic permit",
            )?;
            // The writer is queued while the actual goal semaphore is held.
            // Retained admission must refuse before trying that semaphore.
            let later = tokio::time::timeout(
                Duration::from_secs(5),
                fixture.runtime.alab_automatic_input_permit(),
            )
            .await
            .map_err(|_| "queued replacement caused an automatic-window deadlock")?;
            require(
                later.is_err(),
                "a queued replacement admitted another automatic window",
            )?;
            drop(later);
            drop(permit);
            let replacement = replacement.await?;
            require(
                fixture.runtime.alab_automatic_input_permit().await.is_err(),
                "replacement hold was not attached to the actual runtime",
            )?;
            fixture
                .runtime
                .alab_recheck_after_retained_frontier()
                .await?;
            require(
                mock.requests().is_empty(),
                "replacement hold reached the model",
            )?;

            owner.release_if(&replacement, || Ok(())).await?;
            fixture
                .runtime
                .alab_recheck_after_retained_frontier()
                .await?;
            tokio::time::timeout(Duration::from_secs(10), async {
                while mock.requests().is_empty() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .map_err(|_| "released original goal policy never admitted its real Core turn")?;
            require(
                mock.requests().len() == 1,
                "released goal was admitted more than once",
            )?;
            // Preserve the fixture's ordinary paused-goal cleanup before the
            // delayed model response can trigger a second idle continuation.
            fixture.set_status(ThreadGoalStatus::Paused, true).await;
            Ok(())
        }
        .await;
        // All borrowed automatic permits/pending hold observations above have
        // dropped before this actual Core shutdown, including error paths.
        fixture.shutdown().await;
        drop(owner);
        result.unwrap_or_else(|error| panic!("actual retained goal runtime: {error}"));
    });
}
