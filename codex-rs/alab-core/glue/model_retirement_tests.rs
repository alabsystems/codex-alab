// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::session::tests::HeldStepTask;
use crate::session::tests::make_session_and_context_with_auth_config_home_and_rx;
use crate::state::TaskKind;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[path = "model_retirement_shell_tests.rs"]
mod shell;

enum Case {
    Follow,
    Superseded,
    ChangedOwner,
    Continuation,
    CompletedTool,
    UnresolvedTool,
    ModelRequestHook,
    AlteredInput,
    YieldedShell,
    ProviderStream,
}

#[tokio::test]
async fn first_http_refusal_requires_one_current_account_and_one_original_model_step() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::first_http_refusal_requires_one_current_account_and_one_original_model_step"
    )) {
        exercise(Case::Follow).await;
    }
}

#[tokio::test]
async fn settings_accepted_during_retirement_lookup_keep_their_step() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::settings_accepted_during_retirement_lookup_keep_their_step"
    )) {
        exercise(Case::Superseded).await;
    }
}

#[tokio::test]
async fn a_different_catalog_owner_cannot_recover_an_earlier_refusal() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::a_different_catalog_owner_cannot_recover_an_earlier_refusal"
    )) {
        exercise(Case::ChangedOwner).await;
    }
}

#[tokio::test]
async fn a_rejected_continuation_updates_only_future_turns() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::a_rejected_continuation_updates_only_future_turns"
    )) {
        exercise(Case::Continuation).await;
    }
}

#[tokio::test]
async fn a_completed_tool_result_reaches_the_successor_on_the_same_turn() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::a_completed_tool_result_reaches_the_successor_on_the_same_turn"
    )) {
        exercise(Case::CompletedTool).await;
    }
}

#[tokio::test]
async fn an_outstanding_tool_response_blocks_retirement_replay() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::an_outstanding_tool_response_blocks_retirement_replay"
    )) {
        exercise(Case::UnresolvedTool).await;
    }
}

#[tokio::test]
async fn a_matched_model_request_hook_prevents_retirement_replay() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::a_matched_model_request_hook_prevents_retirement_replay"
    )) {
        exercise(Case::ModelRequestHook).await;
    }
}

#[tokio::test]
async fn a_changed_tool_result_refuses_the_successor_request() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::a_changed_tool_result_refuses_the_successor_request"
    )) {
        exercise(Case::AlteredInput).await;
    }
}

#[tokio::test]
#[cfg_attr(windows, ignore = "uses an interactive POSIX shell")]
async fn a_yielded_terminal_survives_a_retry_but_an_active_poll_blocks_it() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::a_yielded_terminal_survives_a_retry_but_an_active_poll_blocks_it"
    )) {
        exercise(Case::YieldedShell).await;
    }
}

#[tokio::test]
async fn only_the_original_untransformed_provider_stream_can_replay() {
    if crate::alab_test_process::child(concat!(
        module_path!(),
        "::only_the_original_untransformed_provider_stream_can_replay"
    )) {
        exercise(Case::ProviderStream).await;
    }
}

struct ReplacementApiKey;

impl codex_login::ExternalAuth for ReplacementApiKey {
    fn resolve(&self) -> codex_login::ExternalAuthFuture<'_, CodexAuth> {
        Box::pin(async { Ok(CodexAuth::from_api_key("sk-test-local-retirement")) })
    }

    fn refresh(
        &self,
        _context: codex_login::ExternalAuthRefreshContext,
    ) -> codex_login::ExternalAuthFuture<'_, CodexAuth> {
        self.resolve()
    }
}

fn completed_goal(result: &str) -> Vec<ResponseItem> {
    [
        serde_json::json!({"type":"function_call","name":"get_goal","arguments":"{}","call_id":"goal-result"}),
        serde_json::json!({"type":"function_call_output","call_id":"goal-result","output":result}),
    ]
    .into_iter()
    .map(|item| serde_json::from_value(item).unwrap())
    .collect()
}

async fn exercise(case: Case) {
    codex_alab_core::activate();
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    let (session, initial, _events) = make_session_and_context_with_auth_config_home_and_rx(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        vec![],
        home.path(),
        |config| {
            if matches!(&case, Case::YieldedShell) {
                // The protected test process already supplies the filesystem and network sandbox.
                config
                    .permissions
                    .set_permission_profile(codex_protocol::models::PermissionProfile::External {
                        network: codex_protocol::permissions::NetworkSandboxPolicy::Restricted,
                    })
                    .unwrap();
            }
            config.model = Some("gpt-6.1-sol".into());
            config.alab_saved_model_can_follow.clone_from(&config.model);
            config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
            config.model_provider.base_url = Some(format!("{}/backend-api/codex", server.uri()));
        },
    )
    .await;
    let old = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .find(|row| row.slug == "gpt-6.1-sol")
        .unwrap();
    let mut next = old.clone();
    next.slug = "gpt-6.2-sol".into();
    let initial_rows = Mock::given(method("GET"))
        .and(path("/backend-api/codex/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ModelsResponse {
            models: vec![old.clone()],
        }))
        .expect(1)
        .mount_as_scoped(&server)
        .await;
    session
        .services
        .models_manager
        .list_models(
            RefreshStrategy::Online,
            initial.config.http_client_factory(),
        )
        .await;
    drop(initial_rows);
    let turn = session
        .new_turn_with_default_settings("retired-model".to_owned(), Default::default())
        .await;
    let finish = Arc::new(Notify::new());
    session
        .spawn_task(
            Arc::clone(&turn),
            Vec::new(),
            HeldStepTask {
                kind: TaskKind::Regular,
                finish: Arc::clone(&finish),
            },
        )
        .await;
    let step = session
        .capture_step_context(Arc::clone(&turn), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(step.settings.model_info.slug, old.slug);
    if matches!(case, Case::ProviderStream) {
        stream::tests::exercise(&session, &step).await;
        finish.notify_one();
        return;
    }
    assert!(!session.features.enabled(Feature::StepModelSwitching));
    assert!(matches!(
        session
            .apply_turn_settings(
                &turn.sub_id,
                TurnSettingsUpdate {
                    model: Some(next.slug.clone()),
                    ..Default::default()
                }
            )
            .await,
        TurnSettingsUpdateOutcome::Rejected { .. }
    ));
    let error = || {
        CodexErr::new(CodexErrorDetails::ModelNotFound {
            message: "retired".into(),
            status: Some(http::StatusCode::NOT_FOUND),
        })
    };
    // A later open and an error reported by an already-open stream cannot arm the retry.
    let _ = step
        .alab_observe_model_open(
            &session,
            std::future::ready(Err::<(), _>(error())),
            false,
            true,
        )
        .await;
    assert!(
        !session
            .alab_recover_retired_model(&step, &error(), true)
            .await
    );
    let _ = step
        .alab_observe_model_open(&session, std::future::ready(Ok(())), true, true)
        .await;
    assert!(
        !session
            .alab_recover_retired_model(&step, &error(), true)
            .await
    );
    turn.alab_keep_requested_model(true);
    let _ = step
        .alab_observe_model_open(
            &session,
            std::future::ready(Err::<(), _>(error())),
            true,
            true,
        )
        .await;
    assert!(
        !session
            .alab_recover_retired_model(&step, &error(), true)
            .await
    );
    turn.extension_data.remove::<RequestedModel>();

    let initial_step = matches!(case, Case::Follow | Case::Superseded | Case::ChangedOwner);
    let replayable_hooks = !matches!(case, Case::ModelRequestHook);
    let input = if matches!(case, Case::YieldedShell) {
        shell::launch(&session, &step).await
    } else {
        completed_goal("original tool result")
    };
    if !initial_step && !matches!(case, Case::Continuation) {
        session
            .record_conversation_items(&turn, &step.settings.model_info, &input)
            .await;
    }
    let turn_state = Arc::clone(
        &session
            .active_turn
            .lock()
            .await
            .as_ref()
            .unwrap()
            .turn_state,
    );
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    if matches!(case, Case::UnresolvedTool) {
        assert!(
            turn_state
                .lock()
                .await
                .insert_pending_dynamic_tool("outstanding".into(), tx)
                .is_none()
        );
    }
    let requests_before_refusal = server.received_requests().await.unwrap().len();
    let refused = step
        .alab_observe_model_open(
            &session,
            std::future::ready(Err::<(), _>(error())),
            true,
            replayable_hooks,
        )
        .await
        .unwrap_err();
    assert!(step.extension_data.get::<RejectedModel>().is_some());
    if !matches!(case, Case::Continuation) {
        step.alab_retain_refused_input(&Arc::new(crate::client_common::Prompt {
            input: input.clone(),
            ..Default::default()
        }));
    }
    if matches!(case, Case::YieldedShell) {
        shell::verify_pending_operations(&session, &step).await;
    }
    if matches!(case, Case::ChangedOwner) {
        session
            .services
            .auth_manager
            .set_external_auth(Arc::new(ReplacementApiKey))
            .await
            .unwrap();
        assert!(
            session
                .services
                .auth_manager
                .auth()
                .await
                .unwrap()
                .is_api_key_auth()
        );
        assert!(codex_alab_core::active());
        assert!(
            session
                .services
                .models_manager
                .refreshed_model_catalog()
                .is_none()
        );
        assert!(
            !session
                .alab_recover_retired_model(&step, &refused, initial_step)
                .await
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            requests_before_refusal
        );
    } else {
        let requested = Arc::new(Notify::new());
        let captured = Arc::clone(&requested);
        let rows = ModelsResponse {
            models: vec![old.clone(), next.clone()],
        };
        let response = ResponseTemplate::new(200).set_body_json(rows);
        let (release, blocked) = std::sync::mpsc::channel();
        let blocked = std::sync::Mutex::new(blocked);
        Mock::given(method("GET"))
            .and(path("/backend-api/codex/models"))
            .respond_with(move |_: &wiremock::Request| {
                captured.notify_one();
                blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .expect("catalog was not released");
                response.clone()
            })
            .expect(1)
            .mount(&server)
            .await;
        let recover = session.alab_recover_retired_model(&step, &refused, initial_step);
        let reviewer = match step.settings.approvals_reviewer() {
            ApprovalsReviewer::User => ApprovalsReviewer::AutoReview,
            ApprovalsReviewer::AutoReview => ApprovalsReviewer::User,
        };
        let supersede = async {
            tokio::time::timeout(Duration::from_secs(5), requested.notified())
                .await
                .expect("the real serving catalog was never requested");
            if matches!(case, Case::Superseded) {
                assert_eq!(
                    session
                        .apply_turn_settings(
                            &turn.sub_id,
                            TurnSettingsUpdate {
                                approvals_reviewer: Some(reviewer),
                                ..Default::default()
                            }
                        )
                        .await,
                    TurnSettingsUpdateOutcome::Applied
                );
            }
            release.send(()).unwrap();
        };
        let (recovered, ()) = tokio::join!(recover, supersede);
        let latest = turn.next_step_settings.load_full();
        if matches!(
            case,
            Case::Follow | Case::CompletedTool | Case::AlteredInput | Case::YieldedShell
        ) {
            assert!(recovered);
            assert_eq!(latest.model_info.slug, next.slug);
            assert_eq!(
                latest.selected_collaboration_mode().reasoning_effort(),
                Some(ReasoningEffort::Ultra)
            );
            step.alab_arm_retirement_retry(refused);
            let successor = session
                .capture_step_context(Arc::clone(&turn), &CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(successor.settings.model_info.slug, next.slug);
            assert!(Arc::ptr_eq(&successor.turn, &turn));
            if matches!(case, Case::AlteredInput) {
                let original_error = successor
                    .alab_verify_retirement_input(&session, &completed_goal("changed"))
                    .await
                    .unwrap_err();
                assert!(
                    matches!(original_error.details(), CodexErrorDetails::ModelNotFound { message, status: Some(http::StatusCode::NOT_FOUND) } if message == "retired")
                );
            } else {
                successor
                    .alab_verify_retirement_input(&session, &input)
                    .await
                    .unwrap();
            }
        } else if matches!(
            case,
            Case::Continuation | Case::UnresolvedTool | Case::ModelRequestHook
        ) {
            assert!(!recovered);
            assert_eq!(latest.model_info.slug, old.slug);
            assert_eq!(session.thread_settings_snapshot().await.model, next.slug);
        } else {
            assert!(!recovered);
            assert_eq!(latest.model_info.slug, old.slug);
            assert_eq!(latest.approvals_reviewer(), reviewer);
        }
        assert_eq!(step.settings.model_info.slug, old.slug);
        assert!(
            !session
                .alab_recover_retired_model(&step, &error(), initial_step)
                .await
        );
    }
    if matches!(case, Case::UnresolvedTool) {
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(
            turn_state
                .lock()
                .await
                .remove_pending_dynamic_tool("outstanding")
                .is_some()
        );
    }
    if matches!(case, Case::YieldedShell) {
        shell::finish(&session, &input).await;
    }
    finish.notify_one();
}

#[tokio::test]
async fn only_the_alab_binary_serializes_with_persistent_thread_settings() {
    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::only_the_alab_binary_serializes_with_persistent_thread_settings"
    )) {
        return;
    }
    let (session, _) = crate::session::tests::make_session_and_context().await;
    assert!(!codex_alab_core::is_alab_binary());
    let _persistent = crate::session::thread_settings::acquire_persistence_lock(&session).await;
    assert!(session.alab_serialize_model_activation().await.is_none());
    codex_alab_core::activate();
    let pending = session.alab_serialize_model_activation();
    tokio::pin!(pending);
    assert!(futures::poll!(&mut pending).is_pending());
    assert!(!codex_alab_core::set_enhanced(false));
    drop(_persistent);
    assert!(pending.await.is_some());
}
