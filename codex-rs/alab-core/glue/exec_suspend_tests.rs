// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_core::StartThreadOptions;
use codex_core::TurnInputRequest;
use codex_core::TurnInputSubmission;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutItem;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::run_test_with_large_stack;
use core_test_support::test_codex::test_codex;
use std::time::Duration;

#[path = "exec_test_process.rs"]
mod test_process;

#[test]
fn exec_quota_observation_tracks_a_real_authenticated_request() {
    check_quota_observation(false);
}

#[test]
fn exec_quota_observation_tracks_child_without_replacing_root_settings() {
    check_quota_observation(true);
}

fn check_quota_observation(tree: bool) {
    let (_, module) = module_path!().split_once("::").unwrap();
    let exact = format!("{module}::exec_quota_observation_child");
    let root = tempfile::tempdir().unwrap();
    let guard =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../alab/command_guard.py");
    let output = std::process::Command::new("/usr/bin/python3")
        .arg(&guard)
        .args(["--", "/usr/bin/python3"])
        .arg(&guard)
        .args(["--inherited-group-timeout", "60", "--"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", &exact, "--nocapture"])
        .env_clear()
        .env("ALAB_EXEC_QUOTA_OBSERVER_CHILD", "1")
        .env(
            "ALAB_EXEC_QUOTA_OBSERVER_TREE",
            if tree { "1" } else { "0" },
        )
        .env(
            "ALAB_EXEC_QUOTA_OBSERVER_EVIDENCE",
            root.path().join("observed"),
        )
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("PATH", "/usr/bin:/bin")
        .env("RUST_MIN_STACK", "16777216")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "real quota observation failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(root.path().join("observed")).unwrap(),
        b"observed"
    );
}

#[test]
fn exec_quota_observation_child() -> anyhow::Result<()> {
    if std::env::var("ALAB_EXEC_QUOTA_OBSERVER_CHILD").as_deref() != Ok("1") {
        return Ok(());
    }
    run_test_with_large_stack("exec-quota-observation", || async {
        codex_alab_core::activate();
        let mock = responses::start_mock_server().await;
        let reset = chrono::Utc::now().timestamp() + 86400;
        responses::mount_response_sequence(
            &mock,
            vec![
                wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({"error": {
                    "type": "usage_limit_reached", "resets_at": reset,
                    "message": "fixture weekly quota exhausted"
                }})),
            ],
        )
        .await;
        let auth = codex_login::CodexAuth::from_external_chatgpt_tokens(
            "e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture",
            "account",
            Some("plus"),
        )?;
        let base = mock.uri();
        let fixture = test_codex()
            .with_auth(auth)
            .with_config(move |config| {
                config.model_provider.name = "OpenAI".into();
                config.model_provider.requires_openai_auth = true;
                config.model_provider.supports_websockets = false;
                config.chatgpt_base_url = base;
            })
            .build_with_auto_env(&mock)
            .await?;
        let tree = std::env::var("ALAB_EXEC_QUOTA_OBSERVER_TREE").as_deref() == Ok("1");
        let (_release, gate) = oneshot::channel();
        let (source, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse_completed("not-delivered"),
        }]])
        .await;
        let mut options = fixture.start_thread_options().await;
        if tree {
            options.config.model_provider.base_url = Some(format!("{}/v1", source.uri()));
        }
        options.thread_extension_init.insert(HeadlessExec);
        options.turn_extension_init.insert(HeadlessExec);
        let started = fixture.thread_manager.start_thread(options).await?;
        let mut watch = started.thread.watch_exec_quota().expect("exec observer");
        let TurnInputSubmission::Started { turn_id, .. } = started
            .thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "retain this authenticated quota wait".into(),
                text_elements: Vec::new(),
            }]))
            .await?
        else {
            anyhow::bail!("original turn did not start")
        };
        let child = if tree {
            tokio::time::timeout(Duration::from_secs(10), source.wait_for_request_count(1)).await?;
            let mut options = fixture.start_thread_options().await;
            options.session_source = Some(codex_protocol::protocol::SessionSource::Internal(
                codex_protocol::protocol::InternalSessionSource::MemoryConsolidation,
            ));
            options
                .thread_extension_init
                .insert(codex_alab_core::quota::ExecQuotaMember);
            options
                .turn_extension_init
                .insert(codex_alab_core::quota::ExecQuotaMember);
            let child = fixture
                .thread_manager
                .spawn_internal_session(started.thread_id, options)
                .await?;
            let TurnInputSubmission::Started {
                turn_id: child_turn,
                ..
            } = child
                .thread
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "child authenticated quota wait".into(),
                    text_elements: Vec::new(),
                }]))
                .await?
            else {
                anyhow::bail!("child did not start")
            };
            Some((child, child_turn))
        } else {
            None
        };
        let observed = tokio::time::timeout(
            Duration::from_secs(20),
            watch.wait_for(|wall| wall.as_ref().is_some_and(|wall| wall.current())),
        )
        .await??
        .clone()
        .expect("live quota wall");
        if let Some((child, child_turn)) = &child {
            assert_eq!(observed.options().thread(), child.thread_id);
            assert_eq!(observed.options().turn(), child_turn);
            assert_ne!(observed.options().thread(), started.thread_id);
        } else {
            assert_eq!(observed.options().turn(), turn_id);
        }
        let root_options = watch.root_options().await.expect("original root context");
        assert_eq!(root_options.thread(), started.thread_id);
        assert_eq!(root_options.turn(), turn_id);
        assert!(
            matches!(observed.source(), Some(codex_alab_core::renewal::RequestAuth::ChatGpt { account, user })
            if account == "account" && user == "user")
        );
        if let Some((child, _)) = child {
            child.thread.shutdown_and_wait().await?;
        }
        started.thread.shutdown_and_wait().await?;
        assert!(watch.root_options().await.is_none());
        assert!(
            !observed.current(),
            "shutdown ends the authenticated quota wait"
        );
        assert!(
            watch.borrow().is_none(),
            "shutdown removes the quota observation"
        );
        std::fs::write(
            std::env::var_os("ALAB_EXEC_QUOTA_OBSERVER_EVIDENCE").unwrap(),
            b"observed",
        )?;
        Ok(())
    })
}

#[test]
fn exec_suspension_keeps_original_turn_and_closes_admission() -> anyhow::Result<()> {
    check_original_turn_suspension(false)
}

#[test]
fn ephemeral_exec_suspension_retains_history_without_a_rollout() -> anyhow::Result<()> {
    check_original_turn_suspension(true)
}

fn check_original_turn_suspension(ephemeral: bool) -> anyhow::Result<()> {
    run_test_with_large_stack("exec-suspension", move || async move {
        let (_release, gate) = oneshot::channel();
        let (server, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse_completed("not-delivered"),
        }]])
        .await;
        let mock = responses::start_mock_server().await;
        let fixture = test_codex().build_with_auto_env(&mock).await?;
        let mut options = fixture.start_thread_options().await;
        options.config.ephemeral = ephemeral;
        options.config.model_provider.base_url = Some(format!("{}/v1", server.uri()));
        options.thread_extension_init.insert(HeadlessExec);
        options.turn_extension_init.insert(HeadlessExec);
        let invocation = ExecConfiguration::capture(&options.config, &Default::default())?;
        let started = fixture.thread_manager.start_thread(options).await?;
        let thread_id = started.thread_id;
        let thread = started.thread;
        assert!(
            thread
                .watch_exec_quota()
                .expect("exec quota watch")
                .borrow()
                .is_none()
        );
        let TurnInputSubmission::Started { turn_id, .. } = thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "preserve this original request".into(),
                text_elements: Vec::new(),
            }]))
            .await?
        else {
            anyhow::bail!("expected the original turn to start");
        };
        tokio::time::timeout(Duration::from_secs(10), server.wait_for_request_count(1)).await?;
        let admission = crate::turn_admission::TurnAdmission::default();
        let mut state = State::default();
        // A stale host observation must not stop the current turn or close admission.
        assert!(
            state
                .suspend(
                    &fixture.thread_manager,
                    &admission,
                    thread_id,
                    "stale-turn",
                    &invocation
                )
                .await
                .is_err()
        );
        drop(
            admission
                .admit()
                .expect("rejected observation leaves admission open"),
        );
        assert!(
            thread
                .current_turn_extension_data::<HeadlessExec>(&turn_id)
                .await
                .is_some()
        );
        let suspended = tokio::time::timeout(
            Duration::from_secs(10),
            state.suspend(
                &fixture.thread_manager,
                &admission,
                thread_id,
                &turn_id,
                &invocation,
            ),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        assert_eq!(suspended.thread(), thread_id);
        assert_eq!(suspended.turn(), turn_id);
        if ephemeral {
            let snapshot = suspended.ephemeral_history().expect("memory history");
            let encoded = serde_json::to_vec(snapshot)?;
            let decoded: codex_core::ExecEphemeralHistory = serde_json::from_slice(&encoded)?;
            assert!(decoded.clone().restore(thread_id, "wrong-turn").is_err());
            let restored = decoded
                .restore(thread_id, &turn_id)
                .map_err(anyhow::Error::msg)?;
            assert!(codex_alab_autopilot::unfinished_recovery_turn(
                &restored, &turn_id
            ));
        } else {
            assert!(suspended.ephemeral_history().is_none());
        }
        assert!(admission.admit().is_err());
        // Retry after a lost reply reconciles this exact origin despite closed
        // admission. It cannot lend the completed shutdown to another turn.
        let reconciled = state
            .suspend(
                &fixture.thread_manager,
                &admission,
                thread_id,
                &turn_id,
                &invocation,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(reconciled.thread(), thread_id);
        assert_eq!(reconciled.turn(), turn_id);
        assert!(
            state
                .suspend(
                    &fixture.thread_manager,
                    &admission,
                    thread_id,
                    "other",
                    &invocation
                )
                .await
                .is_err()
        );
        if ephemeral {
            assert!(thread.rollout_path().is_none());
        } else {
            let rollout = std::fs::read_to_string(thread.rollout_path().expect("durable rollout"))?;
            assert!(rollout.contains("preserve this original request"));
        }
        let history = thread.exec_recovery_history().await?;
        assert_eq!(history.thread_id, thread_id);
        assert!(codex_alab_autopilot::unfinished_recovery_turn(
            &history.items,
            &turn_id
        ));
        let mut starts = 0;
        for item in history.items {
            match item {
                RolloutItem::EventMsg(EventMsg::TurnStarted(event)) if event.turn_id == turn_id => {
                    starts += 1;
                }
                RolloutItem::EventMsg(EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_)) => {
                    panic!("suspension must leave the original turn unfinished");
                }
                _ => {}
            }
        }
        assert_eq!(starts, 1, "the original turn remains unfinished");
        server.shutdown().await;
        Ok(())
    })
}

#[test]
fn exec_suspension_refuses_an_unmarked_runtime() -> anyhow::Result<()> {
    run_test_with_large_stack("exec-suspension-unmarked", || async {
        let mock = responses::start_mock_server().await;
        let fixture = test_codex().build_with_auto_env(&mock).await?;
        let started = fixture
            .thread_manager
            .start_thread(StartThreadOptions::new(fixture.config.clone()))
            .await?;
        assert!(started.thread.watch_exec_quota().is_none());
        let admission = crate::turn_admission::TurnAdmission::default();
        assert!(
            suspend(
                &fixture.thread_manager,
                &admission,
                started.thread_id,
                "turn",
                &ExecConfiguration::capture(&fixture.config, &Default::default())?
            )
            .await
            .is_err()
        );
        drop(
            admission
                .admit()
                .expect("refusal leaves the original runtime usable"),
        );
        started.thread.shutdown_and_wait().await?;
        Ok(())
    })
}

#[test]
fn exec_suspension_completes_a_real_wait_without_user_abort() -> anyhow::Result<()> {
    if !test_process::child(concat!(
        module_path!(),
        "::exec_suspension_completes_a_real_wait_without_user_abort"
    )) {
        return Ok(());
    }
    run_test_with_large_stack("exec-transfer-wait", || async {
        codex_alab_core::activate();
        let mock = responses::start_mock_server().await;
        let requests = responses::mount_sse_once(
            &mock,
            responses::sse(vec![
                responses::ev_response_created("wait-response"),
                responses::ev_function_call_with_namespace(
                    "original-wait",
                    "collaboration",
                    "wait_agent",
                    r#"{"timeout_ms":180000}"#,
                ),
                responses::ev_completed("wait-response"),
            ]),
        )
        .await;
        let fixture = test_codex()
            .with_model("gpt-5.6-sol")
            .with_config(|config| {
                config
                    .features
                    .enable(codex_features::Feature::MultiAgentV2)
                    .unwrap();
            })
            .build_with_auto_env(&mock)
            .await?;
        let mut options = fixture.start_thread_options().await;
        options.thread_extension_init.insert(HeadlessExec);
        options.turn_extension_init.insert(HeadlessExec);
        let invocation = ExecConfiguration::capture(&options.config, &Default::default())?;
        let started = fixture.thread_manager.start_thread(options).await?;
        let thread = started.thread;
        let TurnInputSubmission::Started { turn_id, .. } = thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "wait for runtime activity".into(),
                text_elements: Vec::new(),
            }]))
            .await?
        else {
            anyhow::bail!("root did not start");
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = thread.next_event().await?;
                if matches!(event.msg, EventMsg::ItemStarted(ref event)
                    if matches!(&event.item, codex_protocol::items::TurnItem::CollabAgentToolCall(item) if item.id == "original-wait")) {
                    return Ok::<_, anyhow::Error>(());
                }
            }
        }).await??;
        let admission = crate::turn_admission::TurnAdmission::default();
        let mut state = State::default();
        let suspended = tokio::time::timeout(
            Duration::from_secs(10),
            state.suspend(
                &fixture.thread_manager,
                &admission,
                started.thread_id,
                &turn_id,
                &invocation,
            ),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        assert_eq!(suspended.turn(), turn_id);
        let mut completed = 0;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match thread.next_event().await?.msg {
                    EventMsg::ItemCompleted(event) => {
                        if let codex_protocol::items::TurnItem::CollabAgentToolCall(item) =
                            event.item
                            && item.id == "original-wait"
                        {
                            assert_eq!(
                                item.status,
                                codex_protocol::items::CollabAgentToolCallStatus::Interrupted
                            );
                            completed += 1;
                        }
                    }
                    EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_) => {
                        anyhow::bail!("handoff ended original turn")
                    }
                    EventMsg::ShutdownComplete => return Ok::<_, anyhow::Error>(()),
                    _ => {}
                }
            }
        })
        .await??;
        assert_eq!(completed, 1);
        let history = thread.exec_recovery_history().await?;
        assert!(codex_alab_autopilot::unfinished_recovery_turn(
            &history.items,
            &turn_id
        ));
        // Recovery evidence contains lifecycle records; model outputs live in
        // the canonical rollout that the replacement worker restores.
        let rollout = std::fs::read_to_string(thread.rollout_path().expect("durable rollout"))?;
        let items = rollout
            .lines()
            .map(codex_rollout::parse_rollout_line)
            .collect::<Result<Vec<_>, _>>()?;
        let output = items
            .iter()
            .find_map(|line| {
                let RolloutItem::ResponseItem(envelope) = &line.item else {
                    return None;
                };
                let value = serde_json::to_value(&envelope.item).ok()?;
                (value.get("type").and_then(serde_json::Value::as_str)
                    == Some("function_call_output")
                    && value.get("call_id").and_then(serde_json::Value::as_str)
                        == Some("original-wait"))
                .then_some(value)
            })
            .expect("wait result retained in original history");
        let text = output.to_string();
        assert!(text.contains("account handoff"), "{text}");
        assert!(!text.contains("aborted by user"));
        assert_eq!(requests.requests().len(), 1);
        Ok(())
    })
}
