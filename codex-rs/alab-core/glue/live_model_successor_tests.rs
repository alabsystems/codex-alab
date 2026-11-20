// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::session::tests::make_session_and_context_with_auth_and_config_and_rx;
use codex_login::CodexAuth;
use codex_models_manager::manager::RefreshStrategy;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test]
async fn consecutive_unpinned_roots_discover_the_new_sol_default_in_one_server() {
    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::consecutive_unpinned_roots_discover_the_new_sol_default_in_one_server"
    )) {
        return;
    }
    codex_alab_core::activate();
    let server = MockServer::start().await;
    let (session, turn, _) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        vec![],
        |config| {
            config.model = None;
            config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
            config.model_provider.base_url = Some(format!("{}/backend-api/codex", server.uri()));
        },
    )
    .await;
    let mut row = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .find(|row| row.slug == "gpt-6.2-sol")
        .unwrap();
    for slug in ["gpt-88-sol", "gpt-89-sol"] {
        row.slug = slug.into();
        let _catalog = Mock::given(method("GET"))
            .and(path("/backend-api/codex/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ModelsResponse {
                models: vec![row.clone()],
            }))
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        let (selected, _) = crate::session::Session::resolve_startup_model(
            session.services.models_manager.as_ref(),
            &turn.config,
            &codex_protocol::protocol::SessionSource::Cli,
            /*allow_provider_model_fallback*/ false,
        )
        .await
        .unwrap();
        assert_eq!(selected, slug);
    }
}

#[tokio::test]
async fn received_catalog_promotes_running_conversations_and_publishes_the_next_turn_model() {
    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::received_catalog_promotes_running_conversations_and_publishes_the_next_turn_model"
    )) {
        return;
    }
    check_pending_resume_application(false).await;
}

#[tokio::test]
async fn pending_model_resume_observes_the_actor_finishing_in_vanilla() {
    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::pending_model_resume_observes_the_actor_finishing_in_vanilla"
    )) {
        return;
    }
    check_pending_resume_application(true).await;
}

async fn check_pending_resume_application(switch_to_vanilla: bool) {
    codex_alab_core::activate();
    let server = MockServer::start().await;
    let (session, previous, events) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        vec![],
        |config| {
            config.model = Some("gpt-6.1-sol".to_string());
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
        .find(|model| model.slug == "gpt-6.1-sol")
        .unwrap();
    let mut new = old.clone();
    new.slug = "gpt-6.2-sol".into();
    let catalog = || {
        Mock::given(method("GET"))
            .and(path("/backend-api/codex/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ModelsResponse {
                models: vec![old.clone(), new.clone()],
            }))
            .expect(1)
    };
    let enhanced = catalog().mount_as_scoped(&server).await;
    let mut applied = session
        .services
        .session_extension_data
        .get_or_init(super::CatalogApplications::new)
        .0
        .subscribe();
    session
        .services
        .models_manager
        .list_models(
            RefreshStrategy::Online,
            previous.config.http_client_factory(),
        )
        .await;
    let published = session
        .services
        .models_manager
        .model_catalog_updates()
        .unwrap();
    let waiting = super::wait_for_catalog_application(&mut applied, || published.borrow().clone());
    tokio::pin!(waiting);
    assert!(futures::poll!(&mut waiting).is_pending());
    let _catalog_guard = if switch_to_vanilla {
        drop(enhanced);
        assert!(!codex_alab_core::set_enhanced(false));
        // A vanilla turn has its own catalog identity. It must confirm the
        // serving catalog again even when both modes use the same endpoint.
        assert!(
            session
                .services
                .models_manager
                .refreshed_model_catalog()
                .is_none()
        );
        catalog().mount_as_scoped(&server).await
    } else {
        enhanced
    };
    super::super::apply(&session).await;
    assert!(futures::poll!(&mut waiting).is_ready());
    if switch_to_vanilla {
        assert_eq!(
            session.thread_settings_snapshot().await.model,
            "gpt-6.1-sol"
        );
        let (next, _) = session
            .new_turn_with_sub_id("next".into(), Default::default(), Default::default())
            .await
            .unwrap();
        assert_eq!(next.model_info().slug, "gpt-6.1-sol");
        return;
    }
    let announced = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event.msg {
            EventMsg::ThreadSettingsApplied(event) => Some(event.thread_settings),
            _ => None,
        })
        .expect("the model change reaches the TUI via the settings event");
    assert_eq!(announced.model, "gpt-6.2-sol");
    assert_eq!(announced.reasoning_effort, Some(ReasoningEffort::Ultra));
    let (next, _) = session
        .new_turn_with_sub_id("next".into(), Default::default(), Default::default())
        .await
        .unwrap();
    assert_eq!(previous.model_info().slug, "gpt-6.1-sol");
    assert_eq!(next.model_info().slug, "gpt-6.2-sol");
    assert_eq!(next.reasoning_effort(), Some(&ReasoningEffort::Ultra));
}

#[tokio::test]
async fn resume_observer_waits_for_the_superseding_response_or_accepts_no_refresh() {
    use std::sync::Arc;

    let first = Arc::new(());
    let second = Arc::new(());
    let current = std::cell::RefCell::new(Some(Arc::clone(&first)));
    let (sender, mut receiver) = tokio::sync::watch::channel(None);
    {
        let waiting =
            super::wait_for_catalog_application(&mut receiver, || current.borrow().clone());
        tokio::pin!(waiting);
        assert!(futures::poll!(&mut waiting).is_pending());
        current.replace(Some(Arc::clone(&second)));
        sender.send_replace(Some(Arc::downgrade(&first)));
        assert!(futures::poll!(&mut waiting).is_pending());
        sender.send_replace(Some(Arc::downgrade(&second)));
        assert!(futures::poll!(&mut waiting).is_ready());
    }
    // A failed re-fetch leaves the previous confirmed response in place. Whether
    // the caller subscribed before or after application, its receipt is available.
    let unchanged = super::wait_for_catalog_application(&mut receiver, || current.borrow().clone());
    assert!(futures::poll!(Box::pin(unchanged)).is_ready());
    sender.send_replace(None);
    let invalidated = super::wait_for_catalog_application(&mut receiver, || None);
    assert!(futures::poll!(Box::pin(invalidated)).is_ready());
}

#[tokio::test]
async fn idle_handoff_promotes_using_the_api_key_catalog_without_a_refresh() {
    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::idle_handoff_promotes_using_the_api_key_catalog_without_a_refresh"
    )) {
        return;
    }
    codex_alab_core::activate();
    let (session, previous, events) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::from_api_key("fixture-key"),
        vec![],
        |config| {
            config.model = Some("gpt-6.1-sol".to_owned());
            config.model_reasoning_effort = Some(ReasoningEffort::High);
            assert!(config.alab_enable_recovered_model_following());
        },
    )
    .await;
    assert!(
        session
            .services
            .models_manager
            .refreshed_model_catalog()
            .is_none()
    );
    let guard = super::thread_settings::acquire_persistence_lock(&session).await;
    assert!(super::promote_from_catalog(&session, true).await);
    drop(guard);
    let announced = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event.msg {
            EventMsg::ThreadSettingsApplied(event) => Some(event.thread_settings),
            _ => None,
        })
        .expect("post-transfer selection is published");
    assert_eq!(announced.model, "gpt-6.2-sol");
    assert_eq!(announced.reasoning_effort, Some(ReasoningEffort::High));
    assert_eq!(previous.model_info().slug, "gpt-6.1-sol");
}

#[tokio::test]
async fn outstanding_mail_keeps_native_admission_during_handoff_model_selection() {
    use codex_alab_core::upgrade_barrier::Refusal;
    use codex_alab_core::upgrade_startup_hold::Owner;
    use codex_protocol::AgentPath;
    use codex_protocol::protocol::InterAgentCommunication;
    use futures::FutureExt;

    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::outstanding_mail_keeps_native_admission_during_handoff_model_selection"
    )) {
        return;
    }
    codex_alab_core::activate();
    let (session, _) = crate::session::tests::make_session_and_context().await;
    let owner = Owner::default();
    session.services.thread_extension_data.insert(owner.fence());
    let (release, pending) = tokio::sync::oneshot::channel::<()>();
    session.services.session_extension_data.insert(super::HandoffCatalogRefresh(
        async move { pending.await.unwrap() }.boxed().shared(),
    ));
    session.input_queue.enqueue_mailbox_communication(
        crate::PendingAgentMessage::from(InterAgentCommunication::new(
            AgentPath::root(),
            AgentPath::root(),
            Vec::new(),
            "accepted queued work".into(),
            true,
        )),
        Default::default(),
    ).await;
    let barrier = &session.services.local_agent_runtime.alab_upgrade;
    assert!(session.admit_exec_pending_work().await.is_none());
    drop(barrier.prepare().expect("held destination has not started its mail"));

    owner.release_if(|| Ok(())).unwrap();
    let mut admission = Box::pin(session.admit_exec_pending_work());
    assert!(futures::poll!(admission.as_mut()).is_pending());
    assert!(matches!(barrier.prepare(), Err(Refusal::PendingAdmission)));
    assert!(session.input_queue.has_pending_mailbox_items().await);

    release.send(()).unwrap();
    let admitted = admission.await.expect("the original queued work can start");
    assert!(matches!(barrier.prepare(), Err(Refusal::PendingAdmission)));
    drop(admitted);
    assert!(barrier.prepare().is_ok());
}

#[test]
fn first_turn_after_idle_handoff_discovers_a_release_with_a_warm_catalog() -> anyhow::Result<()> {
    const NAME: &str = concat!(
        module_path!(),
        "::first_turn_after_idle_handoff_discovers_a_release_with_a_warm_catalog"
    );
    if !crate::alab_test_process::child(NAME) {
        return Ok(());
    }
    core_test_support::test_codex::run_test_with_large_stack(NAME, || async {
        codex_alab_core::activate();
        check_first_handoff_request(true, HandoffAction::User(None), "gpt-88-astra").await?;
        check_first_handoff_request(false, HandoffAction::User(None), "gpt-6.2-astra").await?;
        check_first_handoff_request(
            true,
            HandoffAction::User(Some("gpt-6.2-astra")),
            "gpt-6.2-astra",
        )
        .await
    })
}

#[derive(Clone, Copy)]
enum HandoffAction {
    User(Option<&'static str>),
    Compact,
    Review(Option<&'static str>),
}

#[test]
fn first_compaction_and_review_after_handoff_use_the_confirmed_model() -> anyhow::Result<()> {
    const NAME: &str = concat!(
        module_path!(),
        "::first_compaction_and_review_after_handoff_use_the_confirmed_model"
    );
    if !crate::alab_test_process::child(NAME) {
        return Ok(());
    }
    core_test_support::test_codex::run_test_with_large_stack(NAME, || async {
        codex_alab_core::activate();
        check_first_handoff_request(true, HandoffAction::Compact, "gpt-88-astra").await?;
        check_first_handoff_request(true, HandoffAction::Review(None), "gpt-88-astra").await?;
        check_first_handoff_request(
            true,
            HandoffAction::Review(Some("gpt-6.2-astra")),
            "gpt-6.2-astra",
        )
        .await
    })
}

async fn check_first_handoff_request(
    catalog_available: bool,
    action: HandoffAction,
    expected: &str,
) -> anyhow::Result<()> {
    use codex_protocol::protocol::Op;
    use codex_protocol::protocol::ReviewRequest;
    use codex_protocol::protocol::ReviewTarget;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    use codex_protocol::turn_input::TurnInputRequest;
    use codex_protocol::turn_input::TurnInputSubmission;
    use codex_protocol::user_input::UserInput;
    use core_test_support::responses;
    use core_test_support::test_codex::test_codex;
    use core_test_support::wait_for_successful_turn;

    let old = codex_models_manager::bundled_models_response()?
        .models
        .into_iter()
        .find(|model| model.slug == "gpt-6.2-astra")
        .unwrap();
    let server = MockServer::start().await;
    let base_url = format!("{}/backend-api/codex", server.uri());
    let route = || Mock::given(method("GET")).and(path("/backend-api/codex/models"));
    let hold = codex_alab_core::upgrade_startup_hold::Owner::default();
    let (fixture, restored) = {
        let _initial = route()
            .respond_with(ResponseTemplate::new(200).set_body_json(ModelsResponse {
                models: vec![old.clone()],
            }))
            .mount_as_scoped(&server)
            .await;
        let test = test_codex()
            .with_model(&old.slug)
            .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
            .with_config(move |config| {
                if let HandoffAction::Review(model) = action {
                    config.review_model = model.map(str::to_owned);
                }
                if matches!(action, HandoffAction::Compact) {
                    config
                        .features
                        .disable(codex_features::Feature::TokenBudget)
                        .unwrap();
                }
                config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
                config.model_provider.base_url = Some(base_url);
                config.model_provider.request_max_retries = Some(0);
            })
            .build(&server)
            .await?;
        let manager = test.thread_manager.models_manager_for_config(&test.config);
        manager
            .list_models(RefreshStrategy::Online, test.config.http_client_factory())
            .await;
        let warm = manager
            .refreshed_model_catalog()
            .expect("confirmed serving catalog");
        assert_eq!(warm.len(), 1);
        assert_eq!(warm[0].slug, old.slug);
        assert_eq!(test.codex.config_snapshot().await.model, old.slug);
        let mut options = test.start_thread_options().await;
        options.thread_extension_init.insert(hold.fence());
        let restored = test.thread_manager.start_thread(options).await?.thread;
        (test, restored)
    };
    let mut released = old.clone();
    released.slug = "gpt-88-astra".into();
    released.multi_agent_reasoning_effort = Some(ReasoningEffort::High);
    let expected_wire_effort = if expected == released.slug {
        "high"
    } else {
        "xhigh"
    };
    let catalog = if catalog_available {
        ResponseTemplate::new(200).set_body_json(ModelsResponse {
            models: vec![old, released],
        })
    } else {
        ResponseTemplate::new(503)
    };
    // The first submission is ready before the new serving catalog is available.
    let _after = route()
        .respond_with(catalog.set_delay(std::time::Duration::from_millis(150)))
        .expect(1)
        .mount_as_scoped(&server)
        .await;
    let mut events = vec![responses::ev_response_created("after-transfer")];
    match action {
        HandoffAction::Review(_) => events.push(responses::ev_assistant_message(
            "review",
            &serde_json::json!({
                "findings": [],
                "overall_correctness": "patch is correct",
                "overall_explanation": "No changes needed.",
                "overall_confidence_score": 1.0
            })
            .to_string(),
        )),
        HandoffAction::Compact => events.push(serde_json::json!({
            "type": "response.output_item.done",
            "item": {"type": "compaction", "encrypted_content": "HANDOFF_SUMMARY"}
        })),
        HandoffAction::User(_) => {}
    }
    events.push(responses::ev_completed("after-transfer"));
    let sent = responses::mount_sse_once(&server, responses::sse(events)).await;
    let mut incoming = Box::pin(async {
        match action {
            HandoffAction::User(model) => {
                let input = TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Continue in this session.".into(),
                    text_elements: vec![],
                }])
                .with_thread_settings(ThreadSettingsOverrides {
                    model: model.map(str::to_owned),
                    ..Default::default()
                });
                assert!(matches!(
                    restored.start_or_steer_turn(input).await?,
                    TurnInputSubmission::Started { .. }
                ));
            }
            HandoffAction::Compact => {
                restored.submit(Op::Compact).await?;
            }
            HandoffAction::Review(_) => {
                restored
                    .submit(Op::Review {
                        review_request: ReviewRequest {
                            target: ReviewTarget::Custom {
                                instructions: "Review this change.".into(),
                            },
                            user_facing_hint: None,
                        },
                    })
                    .await?;
            }
        }
        anyhow::Ok(())
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), &mut incoming)
            .await
            .is_err(),
        "the released turn must not capture settings before model restoration"
    );
    assert!(sent.requests().is_empty());
    restored.alab_restore_model_following().await;
    hold.release_if(|| Ok(())).map_err(anyhow::Error::msg)?;
    incoming.await?;
    wait_for_successful_turn(&restored).await;
    let body = sent.single_request().body_json();
    assert_eq!(body["model"], expected);
    if matches!(action, HandoffAction::User(_)) {
        assert_eq!(body["reasoning"]["effort"], expected_wire_effort);
    }
    if matches!(action, HandoffAction::Review(_) | HandoffAction::Compact) {
        assert_eq!(restored.config_snapshot().await.model, "gpt-88-astra");
    }
    assert_eq!(
        restored.config_snapshot().await.reasoning_effort,
        Some(ReasoningEffort::Ultra)
    );
    restored.shutdown_and_wait().await?;
    fixture.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn resumed_turn_preserves_requested_effort_across_serving_catalog_withdrawals() {
    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::resumed_turn_preserves_requested_effort_across_serving_catalog_withdrawals"
    )) {
        return;
    }
    check_resumed_catalog_withdrawal(false).await;
}

#[tokio::test]
async fn custom_catalog_preserves_effort_and_recovers_only_along_its_declared_edges() {
    if !crate::alab_test_process::child(concat!(
        module_path!(),
        "::custom_catalog_preserves_effort_and_recovers_only_along_its_declared_edges"
    )) {
        return;
    }
    check_resumed_catalog_withdrawal(true).await;
}

async fn check_resumed_catalog_withdrawal(custom_catalog: bool) {
    codex_alab_core::activate();
    let server = MockServer::start().await;
    let (session, previous, _) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        vec![],
        |config| {
            config.model = Some("gpt-6.2-sol".into());
            config.alab_saved_model_can_follow.clone_from(&config.model);
            config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
            config.model_provider.base_url = Some(format!("{}/backend-api/codex", server.uri()));
            if custom_catalog {
                config.model_provider_id = "gateway".into();
                config.model_provider.model_catalog_url =
                    Some(format!("{}/backend-api/codex/models", server.uri()).into());
            }
        },
    )
    .await;
    let mut sol = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .find(|model| model.slug == "gpt-6.2-sol")
        .unwrap();
    let mut astra = sol.clone();
    astra.slug = "gpt-6.2-astra".into();
    let mut unlinked = sol.clone();
    unlinked.slug = "gpt-99-sol".into();
    unlinked.upgrade = None;
    sol.upgrade = None;
    let mut future = sol.clone();
    future.slug = "gpt-6.3-sol".into();
    sol.supported_reasoning_levels
        .retain(|preset| preset.effort == ReasoningEffort::Low);
    sol.default_reasoning_level = Some(ReasoningEffort::Low);
    let served = std::sync::Arc::new(std::sync::Mutex::new(vec![sol.clone(), astra.clone()]));
    let rows = std::sync::Arc::clone(&served);
    Mock::given(method("GET"))
        .and(path("/backend-api/codex/models"))
        .respond_with(move |_: &wiremock::Request| {
            ResponseTemplate::new(200).set_body_json(ModelsResponse {
                models: rows.lock().unwrap().clone(),
            })
        })
        .mount(&server)
        .await;
    let manager = &session.services.models_manager;
    manager
        .list_models(
            RefreshStrategy::Online,
            previous.config.http_client_factory(),
        )
        .await;
    super::super::apply(&session).await;
    let (temporary, saved) = session
        .new_turn_with_sub_id(
            "temporarily-low".into(),
            Default::default(),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        temporary.to_turn_context_item().effort,
        Some(ReasoningEffort::Ultra)
    );
    assert_eq!(
        session
            .alab_effort_for_request(&temporary.next_step_settings.load_full())
            .await,
        Some(ReasoningEffort::Low)
    );
    assert_eq!(saved.reasoning_effort, Some(ReasoningEffort::Ultra));

    *served.lock().unwrap() = if custom_catalog {
        vec![astra.clone(), unlinked.clone()]
    } else {
        vec![astra.clone()]
    };
    manager
        .list_models(
            RefreshStrategy::Online,
            previous.config.http_client_factory(),
        )
        .await;
    assert!(
        session
            .new_turn_with_sub_id("withdrawn".into(), Default::default(), Default::default())
            .await
            .is_err()
    );

    if custom_catalog {
        sol.upgrade = Some(codex_protocol::openai_models::ModelInfoUpgrade {
            model: future.slug.clone(),
            migration_markdown: String::new(),
            retirement_at: None,
        });
        *served.lock().unwrap() = vec![sol, future.clone(), unlinked, astra];
    } else {
        *served.lock().unwrap() = vec![future.clone(), astra];
    }
    let (restored, _) = session
        .new_turn_with_sub_id(
            "same-thread-retry".into(),
            Default::default(),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(restored.model_info().slug, future.slug);
    assert_eq!(restored.reasoning_effort(), Some(&ReasoningEffort::Ultra));
}
