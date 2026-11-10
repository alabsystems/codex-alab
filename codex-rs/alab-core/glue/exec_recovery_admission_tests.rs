// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_core::RecoverTurnRequest;
use codex_core::StartIfIdleSubmission;
use codex_core::TurnInputRequest;
use codex_core::TurnInputSubmission;
use codex_core::TurnStartOptions;
use codex_core::WithTurnExtensionData;
use codex_extension_api::ExtensionDataInit;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::user_input::UserInput;
use codex_rollout::InitialHistory;
use codex_rollout::ResumedHistory;
use codex_thread_store::LoadThreadHistoryParams;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::run_test_with_large_stack;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn exec_recovery_preserves_original_settings_and_requires_live_admission() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-recovery-settings", || {
        recovery_case(false, false, false)
    })
}

#[test]
fn exec_recovery_preserves_original_turn_across_compacted_history() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-recovery-compacted", || {
        recovery_case(true, false, false)
    })
}

#[test]
fn ephemeral_exec_recovery_restores_original_turn_and_settings_without_storage()
-> anyhow::Result<()> {
    run_test_with_large_stack("exec-recovery-ephemeral", || {
        recovery_case(false, true, false)
    })
}

#[test]
fn exec_recovery_waits_for_explicit_tree_release_after_native_admission() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-recovery-launch", || recovery_case(false, false, true))
}

async fn recovery_case(compact: bool, ephemeral: bool, launch: bool) -> anyhow::Result<()> {
    let (_release, gate) = oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse_completed("not-delivered"),
    }]])
    .await;
    let mock = responses::start_mock_server().await;
    let fixture = test_codex()
        .with_model("gpt-5.6-sol")
        .with_config(|config| {
            config
                .features
                .enable(codex_features::Feature::FastMode)
                .expect("fast mode");
        })
        .build_with_auto_env(&mock)
        .await?;
    let mut options = fixture.start_thread_options().await;
    options.config.ephemeral = ephemeral;
    options.config.model_provider.base_url = Some(format!("{}/v1", server.uri()));
    options.history_mode = Some(if ephemeral {
        ThreadHistoryMode::Legacy
    } else {
        ThreadHistoryMode::Paginated
    });
    options.thread_extension_init.insert(HeadlessExec);
    options.turn_extension_init.insert(HeadlessExec);
    let invocation = codex_core::config::ExecConfiguration::capture(
        &options.config,
        &codex_core::config::ConfigOverrides::default(),
    )?;
    let started = fixture.thread_manager.start_thread(options).await?;
    let thread_id = started.thread_id;
    let thread = started.thread;
    let schema = serde_json::json!({
        "type": "object", "properties": {"answer": {"type": "string"}},
        "required": ["answer"], "additionalProperties": false
    });
    let TurnInputSubmission::Started { turn_id, .. } = thread
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "retain the original structured response".to_owned(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
                effort: Some(Some(codex_protocol::openai_models::ReasoningEffort::High)),
                summary: Some(codex_protocol::config_types::ReasoningSummary::Detailed),
                ..Default::default()
            })
            .on_start(TurnStartOptions {
                final_output_json_schema: Some(schema.clone()),
                service_tier: Some("priority".to_owned()),
                ..Default::default()
            }),
        )
        .await?
    else {
        anyhow::bail!("original turn was not started");
    };
    tokio::time::timeout(Duration::from_secs(10), server.wait_for_request_count(1)).await?;
    thread
        .inject_if_running(vec![
            responses::user_message_item("queued accepted context first"),
            responses::user_message_item("queued accepted context second"),
        ])
        .await
        .map_err(|_| anyhow::anyhow!("original turn rejected pending context"))?;
    assert!(
        !serde_json::to_string(&thread.exec_recovery_history().await?.items)?
            .contains("queued accepted context"),
        "fixture must leave the accepted items queued, outside recorded history",
    );

    if compact {
        // Persist a compaction checkpoint through the real writer. Its model
        // context intentionally omits the original native start/input events.
        let snapshot = thread.conversation_history_snapshot().await;
        fixture
            .thread_store
            .append_items(codex_thread_store::AppendThreadItemsParams {
                thread_id,
                items: vec![codex_rollout::RolloutItem::Compacted(Box::new(
                    codex_rollout::CompactedItem {
                        message: "compacted original request".into(),
                        replacement_history: Some(
                            snapshot.items().cloned().map(Into::into).collect(),
                        ),
                        guardian_history: None,
                        retained_context: None,
                        mcp_resource_origins: None,
                        window_number: Some(1),
                        first_window_id: None,
                        previous_window_id: None,
                        window_id: None,
                        compaction_response_id: None,
                        latest_token_usage_record: None,
                        resume_metadata: None,
                    },
                ))],
            })
            .await?;
    }
    let suspended = State::default()
        .suspend(
            &fixture.thread_manager,
            &crate::turn_admission::TurnAdmission::default(),
            thread_id,
            &turn_id,
            &invocation,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    let tree: codex_core::ExecTreeSnapshot =
        serde_json::from_slice(&serde_json::to_vec(suspended.tree())?)?;
    assert!(tree.validate(thread_id, "wrong-turn").is_err());
    let saved = tree.threads()[0].recovery().unwrap().clone();
    let memory_history = tree.threads()[0].ephemeral_history().cloned();
    if ephemeral {
        assert!(thread.rollout_path().is_none());
        assert!(
            fixture
                .thread_store
                .load_history(LoadThreadHistoryParams {
                    thread_id,
                    include_archived: false,
                })
                .await
                .is_err()
        );
    }
    fixture
        .thread_manager
        .remove_thread(&thread_id)
        .await
        .expect("unload the suspended root");
    let recovery_server = responses::start_mock_server().await;
    let response =
        responses::mount_sse_once(&recovery_server, responses::sse_completed("recovered")).await;
    let replacement = test_codex()
        .with_model("gpt-5.4")
        .with_home(Arc::clone(&fixture.home))
        .with_thread_store(Arc::clone(&fixture.thread_store))
        .with_config(move |config| {
            config.ephemeral = ephemeral;
            config.model_reasoning_effort =
                Some(codex_protocol::openai_models::ReasoningEffort::Low);
            config.model_reasoning_summary =
                Some(codex_protocol::config_types::ReasoningSummary::None);
            config
                .features
                .enable(codex_features::Feature::FastMode)
                .expect("fast mode");
        })
        .build_with_auto_env(&recovery_server)
        .await?;
    let mut options = replacement.start_thread_options().await;
    if let Some(history) = memory_history {
        options = history
            .prepare_start(options, thread_id, &turn_id)
            .map_err(anyhow::Error::msg)?;
    } else {
        let history = fixture
            .thread_store
            .load_latest_model_context(LoadThreadHistoryParams {
                thread_id,
                include_archived: false,
            })
            .await?;
        if compact {
            assert!(!history.items.iter().any(|item| matches!(
                item,
                codex_rollout::RolloutItem::EventMsg(EventMsg::TurnStarted(_))
            )));
        }
        options.initial_history = InitialHistory::Resumed(ResumedHistory {
            conversation_id: thread_id,
            history: Arc::new(history.items),
            history_revision: history.revision,
            rollout_path: None,
        });
        options.history_mode = Some(ThreadHistoryMode::Paginated);
        options.thread_extension_init.insert(HeadlessExec);
        options.turn_extension_init.insert(HeadlessExec);
    }
    options = tree
        .inputs()
        .clone()
        .prepare(thread_id, &turn_id)
        .map_err(anyhow::Error::msg)?
        .prepare_start(options, thread_id, &turn_id)
        .map_err(anyhow::Error::msg)?;
    let resumed = replacement.thread_manager.start_thread(options).await?;
    assert_eq!(resumed.thread_id, thread_id);
    let resumed = resumed.thread;
    let launch_barrier = if launch {
        Some(
            codex_core::ExecLaunchBarrier::prepare(&tree, &[Arc::clone(&resumed)])
                .map_err(anyhow::Error::msg)?,
        )
    } else {
        None
    };
    if ephemeral {
        assert!(resumed.rollout_path().is_none());
    }
    let request = || RecoverTurnRequest {
        turn_id: turn_id.clone(),
        thread_settings: Default::default(),
        trace: None,
        prompt_cache_ttl: None,
        cyber_access_program: None,
    };
    let mut unadmitted = ExtensionDataInit::new();
    unadmitted.insert(saved.clone());
    assert!(
        resumed
            .recover_turn_if_idle(WithTurnExtensionData::new(request(), unadmitted))
            .await
            .is_err()
    );
    assert!(
        response.requests().is_empty(),
        "serialized settings cannot authorize execution"
    );
    let mut recovery = recovery::State::default();
    let denied = if let Some(barrier) = &launch_barrier {
        ExecRecoveryRequest::new_for_launch(
            saved.clone(),
            barrier.permit(thread_id).map_err(anyhow::Error::msg)?,
            |_| Box::pin(async { Err("revoked".to_owned()) }),
        )
        .map_err(anyhow::Error::msg)?
    } else {
        ExecRecoveryRequest::new(saved.clone(), |_| {
            Box::pin(async { Err("revoked".to_owned()) })
        })
    };
    assert!(
        recovery
            .run(&replacement.thread_manager, denied.clone())
            .await
            .is_err()
    );
    assert!(
        recovery
            .run(&replacement.thread_manager, denied)
            .await
            .is_err()
    );
    assert!(
        response.requests().is_empty(),
        "live refusal must precede sampling"
    );
    let expected = turn_id.clone();
    let admit = move |turn: String| -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
    > {
        let expected = expected.clone();
        Box::pin(async move {
            if turn == expected {
                Ok(())
            } else {
                Err("wrong original turn".to_owned())
            }
        })
    };
    let admitted = if let Some(barrier) = &launch_barrier {
        ExecRecoveryRequest::new_for_launch(
            saved.clone(),
            barrier.permit(thread_id).map_err(anyhow::Error::msg)?,
            admit,
        )
        .map_err(anyhow::Error::msg)?
    } else {
        ExecRecoveryRequest::new(saved.clone(), admit)
    };
    let result = recovery
        .run(&replacement.thread_manager, admitted.clone())
        .await
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        result,
        StartIfIdleSubmission::Started {
            turn_id: turn_id.clone(),
            root_turn_id: turn_id.clone()
        }
    );
    assert_eq!(
        recovery
            .run(&replacement.thread_manager, admitted.clone())
            .await
            .map_err(anyhow::Error::msg)?,
        result,
        "a lost successful reply reconciles the same attempt"
    );
    assert!(
        recovery::State::default()
            .run(&replacement.thread_manager, admitted)
            .await
            .is_err(),
        "the same admission cannot be replayed in another runtime state"
    );
    let different = ExecRecoveryRequest::new(saved.clone(), |_| Box::pin(async { Ok(()) }));
    assert!(
        recovery
            .run(&replacement.thread_manager, different)
            .await
            .is_err(),
        "a new request cannot borrow another attempt's accepted result"
    );
    if let Some(barrier) = &launch_barrier {
        tokio::time::timeout(Duration::from_secs(10), barrier.wait_ready())
            .await?
            .map_err(anyhow::Error::msg)?;
        assert!(
            response.requests().is_empty(),
            "native admission must not bypass launch ordering"
        );
        barrier.release().map_err(anyhow::Error::msg)?;
    }
    tokio::time::timeout(
        Duration::from_secs(10),
        wait_for_event(&resumed, |event| matches!(event, EventMsg::TurnComplete(_))),
    )
    .await?;
    let body = response.single_request().body_json();
    assert_eq!(
        body["model"], "gpt-5.6-sol",
        "replacement defaults must not change the original model"
    );
    assert_eq!(body["text"]["format"]["schema"], schema);
    assert_eq!(body["service_tier"], "priority");
    assert_eq!(body["reasoning"]["effort"], "high");
    assert_eq!(body["reasoning"]["summary"], "detailed");
    assert_eq!(
        body["input"]
            .to_string()
            .matches("retain the original structured response")
            .count(),
        1,
        "recovery must retain the original input exactly once",
    );
    let input_text = body["input"].to_string();
    assert_eq!(
        input_text.matches("queued accepted context first").count(),
        1
    );
    assert_eq!(
        input_text.matches("queued accepted context second").count(),
        1
    );
    assert!(
        input_text.find("queued accepted context first")
            < input_text.find("queued accepted context second")
    );
    assert_eq!(response.requests().len(), 1);
    let completed = ExecRecoveryRequest::new(saved, |_| {
        Box::pin(async { panic!("completed history must be refused before native admission") })
    });
    assert!(
        recovery::State::default()
            .run(&replacement.thread_manager, completed)
            .await
            .is_err()
    );
    assert_eq!(response.requests().len(), 1);
    resumed.shutdown_and_wait().await?;
    server.shutdown().await;
    Ok(())
}
