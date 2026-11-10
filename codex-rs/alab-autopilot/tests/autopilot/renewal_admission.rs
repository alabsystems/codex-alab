// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_alab_autopilot::words;
use codex_alab_core::session::AccountHandoff;
use codex_alab_core::session::NativeOrigin;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_core::StartIfIdleSubmission;
use codex_core::WithTurnExtensionData;
use codex_extension_api::ExtensionDataInit;
use codex_protocol::turn_input::RecoverTurnRequest;

use super::login_fixtures as fixtures;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[path = "renewal_repeat.rs"]
mod renewal_repeat;

#[path = "renewal_consume.rs"]
mod renewal_consume;

/// File-backed subscription identity for durable ChatGPT renewal intent.
async fn subscription_fixture(
    label: &str,
) -> (
    TestLogin,
    codex_alab_aterm::renewal::Identity,
    wiremock::MockServer,
) {
    let refresh = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(
            wiremock::ResponseTemplate::new(400)
                .set_body_json(serde_json::json!({"error":"invalid_grant"})),
        )
        .mount(&refresh)
        .await;
    let home = Arc::new(fixtures::home());
    let user = format!("{label}-user");
    let account = format!("{label}-account");
    fixtures::chatgpt(
        home.path(),
        &user,
        &account,
        "fixture-refresh",
        Some(&refresh.uri()),
    );
    let manager = fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
    let identity = codex_alab_aterm::renewal::Identity::from_native(&account, &user)
        .unwrap_or_else(|| panic!("fixture identity"));
    (TestLogin::Manager { manager, home }, identity, refresh)
}

#[test]
fn unrelated_and_env_override_401_do_not_renew_stored_chatgpt_login() {
    big_stack(|| async {
        for env_override in [false, true] {
            let home = Arc::new(fixtures::home());
            fixtures::chatgpt(
                home.path(),
                "scope-user",
                "scope-account",
                "scope-refresh",
                None,
            );
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let refusal = wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error":{"type":"invalid_request_error","code":"invalid_api_key","message":"fixture provider denied access"}
            }));
            let h = harness_login(
                Mode::Current,
                vec![refusal],
                &[
                    ("continue", "false"),
                    ("human_grace_s", "0"),
                    ("logins", "renew"),
                ],
                &[],
                Some(0),
                Some(TestLogin::Manager {
                    manager,
                    home: Arc::clone(&home),
                }),
                Timing {
                    min_work: Duration::ZERO,
                    login_look: Duration::from_millis(25),
                    ..Timing::default()
                },
                move |config| {
                    if env_override {
                        // A present, nonsecret environment value lets the real
                        // request reach the fixture with an explicit env override.
                        config.model_provider.env_key = Some("PATH".into());
                    } else {
                        config.model_provider.name = "Unrelated fixture provider".into();
                        config.model_provider.requires_openai_auth = false;
                        config.model_provider.experimental_bearer_token =
                            Some("fixture-provider-token".into());
                    }
                },
                Some(Arc::new(codex_goal_extension::GoalService::new())),
            )
            .await;
            h.test
                .submit_turn("An unrelated provider must keep its own authentication wall.")
                .await
                .expect("provider auth wall");
            let wall = h.shared.latest_turn(&h.thread).expect("original wall");
            assert_eq!(h.mock.requests().len(), 1);
            fixtures::chatgpt(
                home.path(),
                "scope-user",
                "scope-account",
                "later-refresh",
                None,
            );
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert!(h.shared.owed_act().and_then(|owed| owed.renewal).is_none());
            assert!(
                h.fake
                    .requests()
                    .iter()
                    .all(|request| !request.contains("codex-renewal"))
            );
            assert_eq!(
                h.autopilot.login_looks(),
                0,
                "a different provider cannot watch/reuse ChatGPT login"
            );
            assert!(
                h.fake
                    .acts()
                    .iter()
                    .all(|act| act != words::RULE_LOGIN_BACK)
            );
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("goal runtime");
            assert_eq!(
                runtime
                    .alab_auth_goal_context(&wall)
                    .await
                    .expect("scope proof"),
                None
            );
            let db = h.test.codex.state_db().expect("DB");
            assert!(
                !db.thread_goals()
                    .has_thread_goal_continuation_deferral(h.test.session_configured.thread_id)
                    .await
                    .expect("deferral")
            );
            assert_eq!(
                h.mock.requests().len(),
                1,
                "no OpenAI sign-in continuation is submitted"
            );
        }
    });
}

#[test]
fn refused_durable_admission_preserves_input_then_recovers_the_same_native_id() {
    big_stack(|| async {
        let (login, identity, _refresh) = subscription_fixture("durable").await;
        let plan = responses::sse(vec![
            responses::ev_response_created("plan"),
            responses::ev_function_call(
                "plan-call",
                "update_plan",
                r#"{"plan":[{"step":"kept work","status":"completed"}]}"#,
            ),
            responses::ev_completed("plan"),
        ]);
        let refused = wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error":{"message":"fixture sign-in expired","type":"invalid_request_error","code":"invalid_api_key"}
        }));
        let h = harness_login(
            Mode::Current,
            vec![
                responses::sse_response(plan),
                refused.clone(),
                refused,
                responses::sse_response(reply("recovered", "Recovered original work.")),
                responses::sse_response(reply("ordinary", "Ordinary input.")),
            ],
            &[("continue", "false"), ("human_grace_s", "0")],
            &[],
            Some(0),
            Some(login),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |config| config.update_plan_enabled = true,
            None,
        )
        .await;
        h.test
            .submit_turn("Keep the original request and completed plan.")
            .await
            .expect("failed auth turn recorded");
        let wall = h
            .shared
            .latest_turn(&h.thread)
            .expect("original native turn");
        assert_eq!(h.mock.requests().len(), 3);
        assert!(
            h.test
                .codex
                .state_db()
                .expect("DB")
                .thread_goals()
                .alab_capture_auth_standalone(h.test.session_configured.thread_id, &wall)
                .await
                .expect("passive original proof")
        );
        let durable = Arc::new(AtomicBool::new(false));
        let can_write = Arc::clone(&durable);
        assert!(
            h.shared
                .set_flush(Box::new(move || can_write.load(Ordering::SeqCst)))
        );
        let marker = OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall.clone(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: false,
                account: identity.account.clone(),
                user: identity.user.clone(),
                receipt: Some("c".repeat(32)),
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: None,
            }),
            rule: Some("login-back@v1".into()),
            text: Some("retained intent".into()),
            mode: Some("cruise".into()),
            ..Default::default()
        };
        h.shared.set_owed_act(Some(marker));
        let shared = h.shared;
        let expected = wall.clone();
        let mut init = ExtensionDataInit::new();
        init.insert(codex_alab_core::renewal::TurnAdmission::new(move |turn| {
            let expected = expected.clone();
            Box::pin(async move {
                if turn != expected {
                    return Err("different native turn".into());
                }
                let mut owed = shared.owed_act().ok_or("missing pending work")?;
                let thread = owed.thread.clone();
                owed.renewal.as_mut().ok_or("missing receipt")?.turn = Some(turn);
                if !shared.publish_renewal(shared.upgrade_generation(), owed)
                    || !shared.flush_record()
                {
                    return Err("disk unavailable".into());
                }
                if !shared.admit_recovered_turn(&thread, &expected) {
                    return Err("native point changed".into());
                }
                Ok(())
            })
        }));
        let request = || RecoverTurnRequest {
            turn_id: wall.clone(),
            thread_settings: Default::default(),
            trace: None,
            prompt_cache_ttl: None,
            cyber_access_program: None,
        };
        assert!(
            h.test
                .codex
                .recover_turn_if_idle(WithTurnExtensionData::new(request(), init.clone()))
                .await
                .is_err()
        );
        assert_eq!(
            h.mock.requests().len(),
            3,
            "failed durability sent no model request"
        );
        assert!(
            h.shared.owed_act().is_some(),
            "refusal retains pending original work"
        );
        durable.store(true, Ordering::SeqCst);
        let accepted = h
            .test
            .codex
            .recover_turn_if_idle(WithTurnExtensionData::new(request(), init))
            .await
            .expect("retry admitted");
        assert!(
            matches!(accepted, StartIfIdleSubmission::Started { turn_id, .. } if turn_id == wall)
        );
        wait_for_event(&h.test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
        assert_eq!(h.mock.requests().len(), 4);
        assert_eq!(
            h.mock.requests()[1].body_json()["input"],
            h.mock.requests()[3].body_json()["input"],
            "recovery keeps original prompt and completed tool output"
        );
        assert!(
            h.shared
                .owed_act()
                .and_then(|owed| owed.renewal)
                .is_some_and(|r| r.accepted)
        );
        // The startup-only callback must not gate unrelated future native input.
        h.test
            .submit_turn("A fresh ordinary turn.")
            .await
            .expect("ordinary turn has no renewal marker");
        assert_eq!(h.mock.requests().len(), 5);
    });
}

#[test]
fn same_file_login_restores_only_the_unedited_auth_blocked_goal() {
    for edit in [false, true] {
        big_stack(move || async move {
            let home = Arc::new(fixtures::home());
            fixtures::api_key(home.path(), "sk-test-old-goal");
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let mut replies = vec![wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))];
            if !edit {
                replies.push(
                    responses::sse_response(reply("restored-goal", "Continuing the goal."))
                        .set_delay(Duration::from_secs(10)),
                );
            }
            let h = harness_login(
                Mode::Current,
                replies,
                &[("continue", "false"), ("human_grace_s", "0")],
                &[],
                Some(0),
                Some(TestLogin::Manager {
                    manager: Arc::clone(&manager),
                    home: Arc::clone(&home),
                }),
                Timing {
                    min_work: Duration::ZERO,
                    login_look: Duration::from_millis(25),
                    ..Timing::default()
                },
                |_| {},
                Some(Arc::new(codex_goal_extension::GoalService::new())),
            )
            .await;
            let db = h.test.codex.state_db().unwrap_or_else(|| panic!("goal DB"));
            let id = h.test.session_configured.thread_id;
            let goal = db
                .thread_goals()
                .replace_thread_goal(
                    id,
                    "original goal",
                    codex_state::ThreadGoalStatus::Active,
                    Some(10000),
                )
                .await
                .expect("active goal");
            h.test
                .submit_turn("Work on the original goal.")
                .await
                .expect("auth wall");
            assert_eq!(
                db.thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("goal")
                    .expect("present")
                    .status,
                codex_state::ThreadGoalStatus::Blocked
            );
            if edit {
                db.thread_goals()
                    .update_thread_goal(
                        id,
                        codex_state::GoalUpdate {
                            objective: None,
                            status: Some(codex_state::ThreadGoalStatus::Blocked),
                            token_budget: None,
                            expected_goal_id: Some(goal.goal_id),
                        },
                    )
                    .await
                    .expect("same-value manual block");
            }
            fixtures::api_key(home.path(), "sk-test-new-goal");
            if edit {
                tokio::time::sleep(Duration::from_millis(300)).await;
                assert_eq!(
                    h.mock.requests().len(),
                    1,
                    "manual block must prevent recovery sampling"
                );
                assert_eq!(
                    db.thread_goals()
                        .get_thread_goal(id)
                        .await
                        .expect("goal")
                        .expect("present")
                        .status,
                    codex_state::ThreadGoalStatus::Blocked
                );
            } else {
                wait_until("goal recovery request", || h.mock.requests().len() == 2).await;
                assert_eq!(
                    db.thread_goals()
                        .get_thread_goal(id)
                        .await
                        .expect("goal")
                        .expect("present")
                        .status,
                    codex_state::ThreadGoalStatus::Active
                );
            }
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("shutdown fixture");
        });
    }
}

#[test]
fn goal_completed_during_the_turn_cannot_be_reclassified_as_standalone() {
    big_stack(|| async {
        let home = Arc::new(fixtures::home());
        fixtures::api_key(home.path(), "sk-test-original-goal");
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let h = harness_login(Mode::Current, vec![wiremock::ResponseTemplate::new(401)
            .set_delay(Duration::from_secs(2))
            .set_body_json(serde_json::json!({"error":{"type":"invalid_request_error","code":"invalid_api_key","message":"fixture auth expired"}}))],
            &[("continue","false"),("human_grace_s","0")], &[], Some(0),
            Some(TestLogin::Manager { manager, home: Arc::clone(&home) }),
            Timing { min_work: Duration::ZERO, login_look: Duration::from_millis(25), ..Timing::default() },
            |_| {}, Some(Arc::new(codex_goal_extension::GoalService::new()))).await;
        let db = h.test.codex.state_db().unwrap_or_else(|| panic!("goal DB"));
        let id = h.test.session_configured.thread_id;
        let original = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "original goal",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("goal");
        h.test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Work on this goal.".into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("start associated turn");
        wait_until("model request started under goal", || {
            h.mock.requests().len() == 1
        })
        .await;
        let complete = db
            .thread_goals()
            .update_thread_goal(
                id,
                codex_state::GoalUpdate {
                    objective: None,
                    status: Some(codex_state::ThreadGoalStatus::Complete),
                    token_budget: None,
                    expected_goal_id: Some(original.goal_id.clone()),
                },
            )
            .await
            .expect("manual complete")
            .expect("same goal");
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .expect("goal runtime");
        runtime
            .apply_external_goal_set(
                complete,
                Some(codex_goal_extension::PreviousGoalSnapshot::from(&original)),
            )
            .await
            .expect("native completion effects");
        wait_for_event(&h.test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
        let wall = h.shared.latest_turn(&h.thread).expect("auth wall");
        assert!(
            db.thread_goals()
                .alab_auth_goal_context(id, &wall)
                .await
                .expect("context")
                .is_none()
        );
        fixtures::api_key(home.path(), "sk-test-renewed-goal");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            h.mock.requests().len(),
            1,
            "completed original goal cannot become standalone recovery"
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn standalone_auth_recovery_preserves_an_unrelated_paused_goal() {
    big_stack(|| async {
        let home = Arc::new(fixtures::home());
        fixtures::api_key(home.path(), "sk-test-standalone-old");
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let h = harness_login(Mode::Current, vec![wiremock::ResponseTemplate::new(401)
            .set_body_json(serde_json::json!({"error":{"type":"invalid_request_error","code":"invalid_api_key","message":"fixture auth expired"}})),
            responses::sse_response(reply("standalone", "Standalone task resumed.")).set_delay(Duration::from_secs(10))],
            &[("continue","false"),("human_grace_s","0")], &[], Some(0),
            Some(TestLogin::Manager { manager, home: Arc::clone(&home) }),
            Timing { min_work: Duration::ZERO, login_look: Duration::from_millis(25), ..Timing::default() },
            |_| {}, Some(Arc::new(codex_goal_extension::GoalService::new()))).await;
        let db = h.test.codex.state_db().unwrap_or_else(|| panic!("goal DB"));
        let id = h.test.session_configured.thread_id;
        db.thread_goals()
            .replace_thread_goal(
                id,
                "paused separate goal",
                codex_state::ThreadGoalStatus::Paused,
                None,
            )
            .await
            .expect("paused goal");
        h.test
            .submit_turn("Independent standalone request.")
            .await
            .expect("auth wall");
        let wall = h.shared.latest_turn(&h.thread).expect("original turn");
        assert_eq!(
            db.thread_goals()
                .alab_auth_goal_context(id, &wall)
                .await
                .expect("standalone proof"),
            Some(None)
        );
        fixtures::api_key(home.path(), "sk-test-standalone-new");
        wait_until("standalone recovery request", || {
            h.mock.requests().len() == 2
        })
        .await;
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(id)
                .await
                .expect("goal")
                .expect("present")
                .status,
            codex_state::ThreadGoalStatus::Paused
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn resumed_revoked_goal_intent_releases_deferral_for_the_new_goal() {
    big_stack(|| async {
        let h = harness_login(Mode::Current, vec![
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}})),
            responses::sse_response(reply("new-goal", "Working on the replacement goal.")).set_delay(Duration::from_secs(10)),
        ], &[("continue", "false"), ("human_grace_s", "0")], &[], Some(0), None,
            Timing { min_work: Duration::ZERO, ..Timing::default() }, |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new()))).await;
        let db = h.test.codex.state_db().unwrap_or_else(|| panic!("goal DB"));
        let id = h.test.session_configured.thread_id;
        let old = db
            .thread_goals()
            .replace_thread_goal(id, "old goal", codex_state::ThreadGoalStatus::Active, None)
            .await
            .expect("old goal");
        h.test
            .submit_turn("Original request must never replay.")
            .await
            .expect("auth wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .expect("runtime");
        runtime
            .alab_defer_auth_recovery(&wall)
            .await
            .expect("persisted recovery deferral");
        h.shared.set_owed_act(Some(OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall.clone(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: true,
                account: "a".repeat(64),
                user: "b".repeat(64),
                receipt: None,
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: Some(old.goal_id),
            }),
            rule: Some("login-back@v1".into()),
            ..Default::default()
        }));
        // Model an offline edit: DB mutation only, without live runtime effects.
        db.thread_goals()
            .replace_thread_goal(
                id,
                "Replacement goal only",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("offline replacement");
        let session = codex_extension_api::ExtensionData::new(h.thread.clone());
        codex_extension_api::ThreadLifecycleContributor::on_thread_resume(
            h.autopilot.as_ref(),
            codex_extension_api::ThreadResumeInput {
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            },
        )
        .await;
        wait_until("replacement goal continues", || {
            h.mock.requests().len() == 2
        })
        .await;
        assert!(
            h.shared.owed_act().is_none(),
            "revoked recovery custody is released"
        );
        let input = h.mock.requests()[1].body_json()["input"].to_string();
        assert!(
            input.contains("Replacement goal only"),
            "new goal owns the next request"
        );
        assert_ne!(
            h.shared.latest_turn(&h.thread).as_deref(),
            Some(wall.as_str()),
            "old native turn is not recovered"
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

async fn set_goal_runtime_enabled(h: &Harness, enabled: bool) {
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
    codex_goal_extension::install_with_backend(
        &mut builder,
        h.test.codex.state_db().unwrap_or_else(|| panic!("goal DB")),
        codex_analytics::AnalyticsEventsClient::disabled(),
        None,
        Arc::downgrade(&h.test.thread_manager),
        Arc::new(codex_goal_extension::GoalService::new()),
        move |config: &Config| codex_goal_extension::GoalExtensionConfig {
            enabled,
            max_goal_token_budget: config.max_goal_token_budget,
        },
    );
    let registry = builder.build();
    let session = codex_extension_api::ExtensionData::new(h.thread.clone());
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_start(codex_extension_api::ThreadStartInput {
                config: &h.test.config,
                session_source: &codex_protocol::protocol::SessionSource::Exec,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                extension_metrics: None,
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            })
            .await;
    }
}

#[test]
fn disabled_goals_keep_standalone_custody_without_installing_a_future_deferral() {
    big_stack(|| async {
        let (login, identity, _refresh) = subscription_fixture("disabled").await;
        let refused = || {
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
            "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
        };
        let h = harness_login(
            Mode::Current,
            vec![
                refused(),
                refused(),
                responses::sse_response(reply("enabled-goal", "New goal continues."))
                    .set_delay(Duration::from_secs(10)),
            ],
            &[("continue", "false"), ("human_grace_s", "0")],
            &[],
            Some(0),
            Some(login),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        set_goal_runtime_enabled(&h, false).await;
        h.test
            .submit_turn("Standalone with goals disabled.")
            .await
            .expect("auth wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        assert!(
            h.test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("disabled runtime")
                .alab_capture_auth_standalone(&wall)
                .await
                .expect("passive standalone capture")
        );
        h.shared.set_owed_act(Some(OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall,
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: true,
                account: identity.account.clone(),
                user: identity.user.clone(),
                receipt: None,
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: None,
            }),
            rule: Some("login-back@v1".into()),
            ..Default::default()
        }));
        let session = codex_extension_api::ExtensionData::new(h.thread.clone());
        codex_extension_api::ThreadLifecycleContributor::on_thread_resume(
            h.autopilot.as_ref(),
            codex_extension_api::ThreadResumeInput {
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            },
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            h.shared.owed_act().is_some(),
            "disabled goals do not revoke standalone custody"
        );
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        assert!(
            !db.thread_goals()
                .has_thread_goal_continuation_deferral(id)
                .await
                .expect("deferral")
        );
        assert_eq!(h.mock.requests().len(), 2);
        set_goal_runtime_enabled(&h, true).await;
        let goal = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "Enabled later goal",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("new goal");
        h.test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .expect("runtime")
            .apply_external_goal_set(goal, None)
            .await
            .expect("enable goal continuation");
        wait_until("later enabled goal is not stranded", || {
            h.mock.requests().len() == 3
        })
        .await;
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn disabling_goals_does_not_downgrade_an_associated_recovery_to_standalone() {
    big_stack(|| async {
        let (login, identity, _refresh) = subscription_fixture("disabled").await;
        let refused = || {
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
            "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
        };
        let h = harness_login(
            Mode::Current,
            vec![refused(), refused()],
            &[("continue", "false"), ("human_grace_s", "0")],
            &[],
            Some(0),
            Some(login),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        let goal = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "Associated goal",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("goal");
        h.test
            .submit_turn("Associated original work.")
            .await
            .expect("auth wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if db
                    .thread_goals()
                    .alab_auth_goal_context(id, &wall)
                    .await
                    .expect("original auth proof")
                    == Some(Some(goal.goal_id.clone()))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("original associated witness before disabling goals");
        h.shared.set_owed_act(Some(OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall,
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: true,
                account: identity.account.clone(),
                user: identity.user.clone(),
                receipt: None,
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: Some(goal.goal_id.clone()),
            }),
            rule: Some("login-back@v1".into()),
            ..Default::default()
        }));
        set_goal_runtime_enabled(&h, false).await;
        let session = codex_extension_api::ExtensionData::new(h.thread.clone());
        codex_extension_api::ThreadLifecycleContributor::on_thread_resume(
            h.autopilot.as_ref(),
            codex_extension_api::ThreadResumeInput {
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            },
        )
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            h.mock.requests().len(),
            2,
            "disabled associated recovery cannot sample"
        );
        assert_eq!(
            h.shared
                .owed_act()
                .and_then(|owed| owed.renewal)
                .and_then(|renewal| renewal.goal_id),
            Some(goal.goal_id)
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn transient_api_key_goal_admission_blocks_duplicate_then_retries_without_durable_deferral() {
    big_stack(|| async {
        let home = Arc::new(fixtures::home());
        fixtures::api_key(home.path(), "sk-test-transient-old");
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let h = harness_login(Mode::Current, vec![
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}})),
            responses::sse_response(reply("transient", "Resumed original goal.")).set_delay(Duration::from_secs(10)),
        ], &[("continue", "false"), ("human_grace_s", "0")], &[], Some(0),
            Some(TestLogin::Manager { manager, home: Arc::clone(&home) }),
            Timing { min_work: Duration::ZERO, login_look: Duration::from_millis(25), ..Timing::default() },
            |_| {}, Some(Arc::new(codex_goal_extension::GoalService::new()))).await;
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        let goal = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "Transient goal",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("goal");
        h.test
            .submit_turn("Keep this original API-key goal turn.")
            .await
            .expect("auth wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .expect("runtime");
        let _transient = runtime
            .alab_begin_transient_auth_recovery()
            .expect("live recovery token");
        let permit = runtime
            .alab_auth_recovery_permit_transient(&wall, Some(&goal.goal_id), &wall)
            .await
            .expect("transient reservation");
        drop(permit);
        assert!(
            !db.thread_goals()
                .has_thread_goal_continuation_deferral(id)
                .await
                .expect("deferral")
        );
        let restored = db
            .thread_goals()
            .get_thread_goal(id)
            .await
            .expect("read")
            .expect("goal");
        runtime
            .apply_external_goal_set(restored, None)
            .await
            .expect("idle goal look");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            h.mock.requests().len(),
            1,
            "live transient latch prevents duplicate goal work"
        );
        fixtures::api_key(home.path(), "sk-test-transient-new");
        wait_until("intended same-ID retry", || h.mock.requests().len() == 2).await;
        assert_eq!(
            h.shared.latest_turn(&h.thread).as_deref(),
            Some(wall.as_str())
        );
        assert!(
            !db.thread_goals()
                .has_thread_goal_continuation_deferral(id)
                .await
                .expect("accepted deferral")
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn queued_generic_goal_rechecks_transient_custody_and_stale_tokens_cannot_clear_it() {
    big_stack(|| async {
        let h = harness_login(Mode::Current, vec![wiremock::ResponseTemplate::new(401)
            .set_body_json(serde_json::json!({"error":{"type":"invalid_request_error","code":"invalid_api_key","message":"fixture auth expired"}}))],
            &[("continue","false"),("human_grace_s","0")], &[], Some(0), None,
            Timing { min_work: Duration::ZERO, ..Timing::default() }, |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new()))).await;
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        db.thread_goals()
            .replace_thread_goal(
                id,
                "Queued goal",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("goal");
        h.test
            .submit_turn("Original auth wall.")
            .await
            .expect("wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall ID");
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .expect("runtime");
        let held = runtime
            .alab_automatic_input_permit()
            .await
            .expect("hold native mutation boundary");
        let mut proposed = db
            .thread_goals()
            .get_thread_goal(id)
            .await
            .expect("read")
            .expect("goal");
        proposed.status = codex_state::ThreadGoalStatus::Active;
        let mut queued = Box::pin(runtime.apply_external_goal_set(proposed, None));
        // Its first suspension is the held mutation permit: the optimistic
        // latch read has already happened, but no native admission can occur.
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(queued.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let old = runtime
            .alab_begin_transient_auth_recovery()
            .expect("first transient token");
        db.thread_goals()
            .alab_restore_auth_goal_with_deferral(id, &wall, false)
            .await
            .expect("restore")
            .expect("same goal");
        drop(held);
        queued
            .await
            .expect("queued continuation was safely suppressed");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            h.mock.requests().len(),
            1,
            "queued generic continuation may not pass the new latch"
        );
        let new = runtime
            .alab_begin_transient_auth_recovery()
            .expect("replacement token");
        assert!(
            !runtime.alab_finish_transient_auth_recovery(old),
            "old guard cannot clear a later recovery"
        );
        assert!(runtime.alab_auth_recovery_deferred());
        assert!(runtime.alab_finish_transient_auth_recovery(new));
        assert!(!runtime.alab_auth_recovery_deferred());
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn same_file_recovery_rechecks_draft_after_suspended_persistence() {
    big_stack(|| async {
        let refresh = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error":"invalid_grant"})),
            )
            .mount(&refresh)
            .await;
        let home = Arc::new(fixtures::home());
        fixtures::chatgpt(
            home.path(),
            "writer-user",
            "writer-account",
            "old-refresh",
            Some(&refresh.uri()),
        );
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let refusal = || {
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
            "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
        };
        let h = harness_login(
            Mode::Current,
            vec![
                refusal(),
                refusal(),
                responses::sse_response(reply("renewed", "Original task resumed."))
                    .set_delay(Duration::from_secs(10)),
            ],
            &[("continue", "false"), ("human_grace_s", "0")],
            &[],
            Some(0),
            Some(TestLogin::Manager {
                manager,
                home: Arc::clone(&home),
            }),
            Timing {
                min_work: Duration::ZERO,
                login_look: Duration::from_millis(25),
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        h.test
            .submit_turn("Retain the original ChatGPT task.")
            .await
            .expect("auth wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        assert_eq!(h.mock.requests().len(), 2);
        // Native completion can precede the extension's wall/baseline publish.
        // Renew only after the original request has an armed login watch.
        wait_until("original ChatGPT wall's login baseline", || {
            h.autopilot.login_looks() > 0
        })
        .await;
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Arc::new(std::sync::Mutex::new(Some(entered_tx)));
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let release_writer = Arc::clone(&release);
        let shared = h.shared;
        assert!(shared.set_flush(Box::new(move || {
            if shared
                .owed_act()
                .and_then(|owed| owed.renewal)
                .is_some_and(|renewal| renewal.local)
                && let Some(entered) = entered.lock().expect("fixture sender").take()
            {
                let _ = entered.send(());
                let (ready, wake) = &*release_writer;
                let ready = ready.lock().expect("fixture writer gate");
                let _ = wake
                    .wait_timeout_while(ready, Duration::from_secs(2), |ready| !*ready)
                    .expect("fixture writer wake");
            }
            true
        })));
        fixtures::chatgpt(
            home.path(),
            "writer-user",
            "writer-account",
            "new-refresh",
            Some(&refresh.uri()),
        );
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .expect("writer reached")
            .expect("writer entry");
        let frame = |draft_empty| {
            shared.observe(
                &Frame {
                    primary_thread: Some(&h.thread),
                    displayed_thread: Some(&h.thread),
                    rollout_path: None,
                    cwd: h.test.cwd_path(),
                    model: "gpt-5",
                    codex_home: h.test.codex_home_path(),
                    embedded: true,
                    state: ProgramState::Idle,
                    draft_empty,
                    queued_input: false,
                    popup_open: false,
                    editor_open: false,
                    pending_user_turn: false,
                },
                now_ms(),
            )
        };
        frame(false);
        {
            let (ready, wake) = &*release;
            *ready.lock().expect("release writer") = true;
            wake.notify_all();
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            h.mock.requests().len(),
            2,
            "draft added while persisting must prevent native work"
        );
        assert!(
            shared.owed_act().and_then(|owed| owed.renewal).is_some(),
            "refusal keeps durable original work"
        );
        frame(true);
        tokio::time::timeout(Duration::from_secs(90), async {
            while h.mock.requests().len() != 3 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("one recovery after the native 60-second recheck");
        assert_eq!(
            shared.latest_turn(&h.thread).as_deref(),
            Some(wall.as_str())
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn failed_external_edit_cannot_leave_the_next_verified_attempt_without_a_latch() {
    big_stack(|| async {
        let home = Arc::new(fixtures::home());
        fixtures::api_key(home.path(), "sk-test-failed-edit-old");
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let h = harness_login(Mode::Current, vec![wiremock::ResponseTemplate::new(401)
            .set_body_json(serde_json::json!({"error":{"type":"invalid_request_error","code":"invalid_api_key","message":"fixture auth expired"}})),
            responses::sse_response(reply("retry", "Original work resumed.")).set_delay(Duration::from_secs(10))],
            &[("continue","false"),("human_grace_s","0")], &[], Some(0),
            Some(TestLogin::Manager { manager, home: Arc::clone(&home) }),
            Timing { min_work: Duration::ZERO, login_look: Duration::from_millis(25), ..Timing::default() },
            |_| {}, Some(Arc::new(codex_goal_extension::GoalService::new()))).await;
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        let goal = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "Original goal",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("goal");
        h.test
            .submit_turn("Original request.")
            .await
            .expect("auth wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .expect("runtime");
        let held = runtime
            .alab_automatic_input_permit()
            .await
            .expect("hold first admission");
        fixtures::api_key(home.path(), "sk-test-failed-edit-new");
        wait_until("first transient latch", || {
            runtime.alab_auth_recovery_deferred()
        })
        .await;
        let frame = |draft_empty| {
            h.shared.observe(
                &Frame {
                    primary_thread: Some(&h.thread),
                    displayed_thread: Some(&h.thread),
                    rollout_path: None,
                    cwd: h.test.cwd_path(),
                    model: "gpt-5",
                    codex_home: h.test.codex_home_path(),
                    embedded: true,
                    state: ProgramState::Idle,
                    draft_empty,
                    queued_input: false,
                    popup_open: false,
                    editor_open: false,
                    pending_user_turn: false,
                },
                now_ms(),
            )
        };
        frame(false);
        drop(held);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if db
                    .thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("goal")
                    .expect("present")
                    .status
                    == codex_state::ThreadGoalStatus::Active
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("first admission restored goal before its refusal");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(h.mock.requests().len(), 1, "draft refused first admission");
        // Prepare an edit but perform no write: the original proof must remain,
        // while the old transient token was explicitly revoked by preparation.
        runtime
            .prepare_external_goal_mutation()
            .await
            .expect("prepare failed edit");
        assert!(!runtime.alab_auth_recovery_deferred());
        assert_eq!(
            runtime.alab_auth_goal_context(&wall).await.expect("proof"),
            Some(Some(goal.goal_id))
        );
        let held = runtime
            .alab_automatic_input_permit()
            .await
            .expect("hold retried admission");
        frame(true);
        let session = codex_extension_api::ExtensionData::new(h.thread.clone());
        codex_extension_api::ThreadLifecycleContributor::on_thread_idle(
            h.autopilot.as_ref(),
            codex_extension_api::ThreadIdleInput {
                cause: codex_extension_api::ThreadIdleCause::Failed,
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            },
        )
        .await;
        wait_until("retry owns a fresh transient latch", || {
            runtime.alab_auth_recovery_deferred()
        })
        .await;
        assert_eq!(h.mock.requests().len(), 1);
        drop(held);
        wait_until("retried original native turn", || {
            h.mock.requests().len() == 2
        })
        .await;
        assert_eq!(
            h.shared.latest_turn(&h.thread).as_deref(),
            Some(wall.as_str())
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn in_flight_goal_replacement_runs_its_own_goal_turn_after_auth_recovery() {
    big_stack(|| async {
        let home = Arc::new(fixtures::home());
        fixtures::api_key(home.path(), "sk-test-replacement-old");
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let refusal = || {
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
            "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
        };
        let h = harness_login(
            Mode::Current,
            vec![
                refusal().set_delay(Duration::from_secs(2)),
                refusal(),
                responses::sse_response(reply("replacement", "Continuing replacement goal."))
                    .set_delay(Duration::from_secs(10)),
            ],
            &[("continue", "false"), ("human_grace_s", "0")],
            &[],
            Some(0),
            Some(TestLogin::Manager {
                manager,
                home: Arc::clone(&home),
            }),
            Timing {
                min_work: Duration::ZERO,
                login_look: Duration::from_millis(25),
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        let old = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "Original goal A",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("goal A");
        h.test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Original A request must not be recovered under B.".into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("start A");
        wait_until("A model request", || h.mock.requests().len() == 1).await;
        let original_turn = h.shared.latest_turn(&h.thread).expect("A native ID");
        let replacement = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "Replacement goal B owns new work",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("goal B");
        let replacement_id = replacement.goal_id.clone();
        let runtime = h
            .test
            .codex
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>()
            .expect("runtime");
        runtime
            .apply_external_goal_set(
                replacement,
                Some(codex_goal_extension::PreviousGoalSnapshot::from(&old)),
            )
            .await
            .expect("native replacement effects");
        wait_until("B starts its own native goal request", || {
            h.mock.requests().len() == 2
        })
        .await;
        wait_for_event(&h.test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
        wait_for_event(&h.test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
        let replacement_turn = h.shared.latest_turn(&h.thread).expect("B native ID");
        assert_ne!(
            replacement_turn, original_turn,
            "old A input is not recovered as B"
        );
        assert!(
            runtime
                .alab_auth_goal_context(&original_turn)
                .await
                .expect("A proof")
                .is_none()
        );
        assert_eq!(
            runtime
                .alab_auth_goal_context(&replacement_turn)
                .await
                .expect("B proof"),
            Some(Some(replacement_id))
        );
        assert!(
            h.mock.requests()[1].body_json()["input"]
                .to_string()
                .contains("Replacement goal B owns new work")
        );
        fixtures::api_key(home.path(), "sk-test-replacement-new");
        wait_until("B's legitimate auth recovery", || {
            h.mock.requests().len() == 3
        })
        .await;
        assert_eq!(
            h.shared.latest_turn(&h.thread).as_deref(),
            Some(replacement_turn.as_str())
        );
        assert_eq!(
            h.mock.requests()[1].body_json()["input"],
            h.mock.requests()[2].body_json()["input"],
            "only B's own native input is recovered"
        );
        assert_eq!(
            db.thread_goals()
                .get_thread_goal(id)
                .await
                .expect("goal")
                .expect("B remains")
                .status,
            codex_state::ThreadGoalStatus::Active
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

/// The old request is still in flight when its replacement is written. No
/// post-wall watch event exists, so recovery must reconcile the initial file.
#[test]
fn credential_saved_before_the_final_401_resumes_the_original_native_turn() {
    big_stack(|| async {
        for before_request in [false, true] {
            let home = Arc::new(fixtures::home());
            fixtures::api_key(home.path(), "sk-test-early-old");
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let h = harness_login(
                Mode::Current,
                vec![wiremock::ResponseTemplate::new(500)],
                &[("continue", "false"), ("human_grace_s", "0")],
                &[],
                Some(0),
                Some(TestLogin::Manager {
                    manager,
                    home: Arc::clone(&home),
                }),
                Timing {
                    min_work: Duration::ZERO,
                    login_look: Duration::from_secs(3600),
                    ..Timing::default()
                },
                |_| {},
                Some(Arc::new(codex_goal_extension::GoalService::new())),
            )
            .await;
            h._server.reset().await;
            let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
            let observed = Arc::clone(&requests);
            let renew_home = Arc::clone(&home);
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path_regex(".*/responses"))
                .respond_with(move |request: &wiremock::Request| {
                    let mut calls = observed.lock().expect("requests");
                    calls.push(request.headers.get("authorization").expect("bearer").to_str().expect("header").to_owned());
                    if calls.len() == 1 {
                        if !before_request { fixtures::api_key(renew_home.path(), "sk-test-early-new"); }
                        wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                            "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
                    } else {
                        assert_eq!(calls.len(), 2, "one original request retry");
                        responses::sse_response(reply("early-renewed", "Original request completed."))
                    }
                }).expect(2).mount(&h._server).await;
            if before_request {
                fixtures::api_key(home.path(), "sk-test-early-new");
            }
            h.test
                .submit_turn("Retain this original native request and its input.")
                .await
                .expect("first completion");
            let original = h.shared.latest_turn(&h.thread).expect("original turn");
            wait_until("early credential recovery", || {
                requests.lock().expect("requests").len() == 2
            })
            .await;
            assert_eq!(
                h.shared.latest_turn(&h.thread).as_deref(),
                Some(original.as_str()),
                "recover exact failed native turn"
            );
            assert_eq!(
                *requests.lock().expect("requests"),
                ["Bearer sk-test-early-old", "Bearer sk-test-early-new"]
            );
            assert!(
                h.fake
                    .requests()
                    .iter()
                    .all(|request| !request.contains("codex-renewal")),
                "same-file API key recovery needs no email renewal"
            );
        }
    });
}

/// Core reloads the file between unauthorized attempts. That credential was
/// then refused too, so merely reading it at the wall cannot recover the turn.
#[test]
fn internally_reloaded_credential_refused_by_the_model_is_not_login_back() {
    big_stack(|| async {
        let refresh = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error":"invalid_grant"})),
            )
            .mount(&refresh)
            .await;
        let home = Arc::new(fixtures::home());
        fixtures::chatgpt(
            home.path(),
            "attempt-user",
            "attempt-account",
            "old-refresh",
            Some(&refresh.uri()),
        );
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let h = harness_login(
            Mode::Current,
            vec![wiremock::ResponseTemplate::new(500)],
            &[("continue", "false"), ("human_grace_s", "0")],
            &[],
            Some(0),
            Some(TestLogin::Manager {
                manager,
                home: Arc::clone(&home),
            }),
            Timing {
                min_work: Duration::ZERO,
                login_look: Duration::from_millis(25),
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        h._server.reset().await;
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let renew_home = Arc::clone(&home);
        let issuer = refresh.uri();
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path_regex(".*/responses"))
            .respond_with(move |request: &wiremock::Request| {
                let mut calls = observed.lock().expect("requests");
                calls.push(request.headers.get("authorization").expect("bearer").to_str().expect("header").to_owned());
                if calls.len() == 1 {
                    fixtures::chatgpt(renew_home.path(), "attempt-user", "attempt-account", "already-tried-refresh", Some(&issuer));
                }
                assert!(calls.len() <= 2, "already refused credential must not replay");
                wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                    "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
            }).expect(2).mount(&h._server).await;
        h.test
            .submit_turn("The internally retried credential is refused too.")
            .await
            .expect("auth wall");
        wait_until("file reconciliation and subsequent look", || {
            h.autopilot.login_looks() >= 2
        })
        .await;
        assert_eq!(
            *requests.lock().expect("requests"),
            [
                "Bearer access-for-old-refresh",
                "Bearer access-for-already-tried-refresh"
            ]
        );
        assert!(
            h.shared.owed_act().and_then(|owed| owed.renewal).is_none(),
            "already tried credential grants no local recovery"
        );
        assert!(
            h.fake
                .acts()
                .iter()
                .all(|act| act != words::RULE_LOGIN_BACK),
            "no false login back"
        );
    });
}

/// A predecessor may have restored its unchanged goal while retaining auth
/// custody. Switching provider offline must release only that old custody so
/// the same active goal proceeds through normal goal policy on the new route.
#[test]
fn resumed_provider_change_retires_old_auth_witness_and_deferral() {
    big_stack(|| async {
        let h = harness_login(Mode::Current, vec![
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture provider expired", "type":"invalid_request_error", "code":"invalid_api_key"}})),
            responses::sse_response(reply("changed-provider", "Working through current provider.")).set_delay(Duration::from_secs(10)),
        ], &[("continue", "false"), ("human_grace_s", "0")], &[], Some(0), None,
            Timing { min_work: Duration::ZERO, ..Timing::default() }, |config| {
                config.model_provider.name = "Explicit replacement provider".into();
                config.model_provider.requires_openai_auth = false;
                config.model_provider.experimental_bearer_token = Some("fixture-provider".into());
            }, Some(Arc::new(codex_goal_extension::GoalService::new()))).await;
        h.test
            .submit_turn("Original subscription request.")
            .await
            .expect("provider wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        let db = h.test.codex.state_db().unwrap_or_else(|| panic!("goal DB"));
        let id = h.test.session_configured.thread_id;
        let goal = db
            .thread_goals()
            .replace_thread_goal(
                id,
                "Continue this unchanged goal normally",
                codex_state::ThreadGoalStatus::Active,
                None,
            )
            .await
            .expect("predecessor goal");
        assert!(
            db.thread_goals()
                .alab_update_for_auth_wall(
                    id,
                    codex_state::GoalUpdate {
                        objective: None,
                        status: Some(codex_state::ThreadGoalStatus::Blocked),
                        token_budget: None,
                        expected_goal_id: Some(goal.goal_id.clone()),
                    },
                    Some(&wall)
                )
                .await
                .expect("predecessor auth wall")
                .is_some()
        );
        assert!(
            db.thread_goals()
                .alab_restore_auth_goal(id, &wall)
                .await
                .expect("predecessor restored goal")
                .is_some()
        );
        assert!(
            db.thread_goals()
                .has_thread_goal_continuation_deferral(id)
                .await
                .expect("old deferral")
        );
        h.shared.set_owed_act(Some(OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall.clone(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: true,
                account: "a".repeat(64),
                user: "b".repeat(64),
                receipt: None,
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: Some(goal.goal_id.clone()),
            }),
            rule: Some("login-back@v1".into()),
            ..Default::default()
        }));
        let session = codex_extension_api::ExtensionData::new(h.thread.clone());
        codex_extension_api::ThreadLifecycleContributor::on_thread_resume(
            h.autopilot.as_ref(),
            codex_extension_api::ThreadResumeInput {
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            },
        )
        .await;
        wait_until("current provider runs normal active goal", || {
            h.mock.requests().len() == 2
        })
        .await;
        assert!(
            h.shared.owed_act().is_none(),
            "obsolete subscription intent retired"
        );
        assert!(
            db.thread_goals()
                .alab_auth_goal_context(id, &wall)
                .await
                .expect("old proof")
                .is_none()
        );
        assert!(
            !db.thread_goals()
                .has_thread_goal_continuation_deferral(id)
                .await
                .expect("deferral released")
        );
        let current = db
            .thread_goals()
            .get_thread_goal(id)
            .await
            .expect("current goal")
            .expect("goal");
        assert_eq!(
            current.goal_id, goal.goal_id,
            "retirement cannot replace the human goal"
        );
        assert!(
            h.mock.requests()[1].body_json()["input"]
                .to_string()
                .contains("Continue this unchanged goal normally")
        );
        assert_ne!(
            h.shared.latest_turn(&h.thread).as_deref(),
            Some(wall.as_str()),
            "normal new turn, never old provider replay"
        );
        assert!(
            h.fake
                .requests()
                .iter()
                .all(|request| !request.contains("codex-renewal"))
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn provider_change_retires_standalone_custody_with_disabled_or_absent_goals() {
    big_stack(|| async {
        for (disabled, goal_bound) in [(false, false), (true, false), (false, true), (true, true)] {
            let goals = disabled.then(|| Arc::new(codex_goal_extension::GoalService::new()));
            let h = harness_login(Mode::Current, vec![
                wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                    "message":"fixture provider expired", "type":"invalid_request_error", "code":"invalid_api_key"}})),
                responses::sse_response(reply("ordinary-work", "The new work completes.")),
            ], &[("continue", "false"), ("human_grace_s", "0")], &[], Some(0), None,
                Timing { min_work: Duration::ZERO, ..Timing::default() }, |config| {
                    config.model_provider.name = "Replacement standalone provider".into();
                    config.model_provider.requires_openai_auth = false;
                    config.model_provider.experimental_bearer_token = Some("fixture-provider".into());
                }, goals).await;
            h.test
                .submit_turn("Old standalone request.")
                .await
                .expect("provider wall");
            let wall = h.shared.latest_turn(&h.thread).expect("wall");
            if disabled {
                set_goal_runtime_enabled(&h, false).await;
            }
            let mut expected_goal = None;
            if let Some(db) = h.test.codex.state_db() {
                let store = db.thread_goals();
                let id = h.test.session_configured.thread_id;
                if goal_bound {
                    let goal = store
                        .replace_thread_goal(
                            id,
                            "Keep the offline human goal",
                            codex_state::ThreadGoalStatus::Active,
                            None,
                        )
                        .await
                        .expect("offline goal");
                    store
                        .alab_update_for_auth_wall(
                            id,
                            codex_state::GoalUpdate {
                                objective: None,
                                status: Some(codex_state::ThreadGoalStatus::Blocked),
                                token_budget: None,
                                expected_goal_id: Some(goal.goal_id.clone()),
                            },
                            Some(&wall),
                        )
                        .await
                        .expect("old auth witness");
                    store
                        .alab_restore_auth_goal(id, &wall)
                        .await
                        .expect("unchanged goal restored");
                    expected_goal = Some(goal.goal_id);
                } else {
                    store
                        .replace_thread_goal(
                            id,
                            "Unrelated paused human goal",
                            codex_state::ThreadGoalStatus::Paused,
                            None,
                        )
                        .await
                        .expect("existing paused goal for native hold");
                    store
                        .alab_record_auth_standalone(id, &wall)
                        .await
                        .expect("old standalone proof");
                }
                store
                    .alab_defer_auth_recovery(id, &wall)
                    .await
                    .expect("old owned deferral");
            }
            h.shared.set_owed_act(Some(OwedAct {
                thread: h.thread.clone(),
                native_origin: Some(NativeOrigin {
                    turn_id: wall.clone(),
                    phase: NativePhase::Complete,
                }),
                renewal: Some(AccountHandoff {
                    purpose: codex_alab_core::account_handoff::Purpose::Login,
                    destination: None,
                    openai_scope: true,
                    local: true,
                    account: "a".repeat(64),
                    user: "b".repeat(64),
                    receipt: None,
                    turn: None,
                    accepted: false,
                    starts: 1,
                    goal_id: expected_goal.clone(),
                }),
                rule: Some("login-back@v1".into()),
                ..Default::default()
            }));
            let session = codex_extension_api::ExtensionData::new(h.thread.clone());
            codex_extension_api::ThreadLifecycleContributor::on_thread_resume(
                h.autopilot.as_ref(),
                codex_extension_api::ThreadResumeInput {
                    session_store: &session,
                    thread_store: h.test.codex.thread_extension_data(),
                },
            )
            .await;
            assert!(
                h.shared.owed_act().is_none(),
                "obsolete standalone custody is retired even without active goals"
            );
            if let Some(db) = h.test.codex.state_db() {
                let id = h.test.session_configured.thread_id;
                assert!(
                    db.thread_goals()
                        .alab_auth_goal_context(id, &wall)
                        .await
                        .expect("old witness")
                        .is_none()
                );
                assert!(
                    !db.thread_goals()
                        .has_thread_goal_continuation_deferral(id)
                        .await
                        .expect("old hold")
                );
            }
            assert_eq!(
                h.mock.requests().len(),
                1,
                "provider change cannot replay old standalone request"
            );
            if let Some(expected) = expected_goal.as_ref() {
                let goal = h
                    .test
                    .codex
                    .state_db()
                    .expect("DB")
                    .thread_goals()
                    .get_thread_goal(h.test.session_configured.thread_id)
                    .await
                    .expect("human goal")
                    .expect("unchanged goal");
                assert_eq!(&goal.goal_id, expected);
                assert_eq!(
                    goal.status,
                    codex_state::ThreadGoalStatus::Active,
                    "retirement does not edit human status"
                );
            }

            h.test
                .submit_turn("New normal work on the replacement provider.")
                .await
                .expect("new task");
            assert_eq!(h.mock.requests().len(), 2);
            assert_ne!(
                h.shared.latest_turn(&h.thread).as_deref(),
                Some(wall.as_str())
            );
            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}

#[test]
fn missing_current_auth_is_unknown_and_keeps_existing_recovery_custody() {
    big_stack(|| async {
        let home = Arc::new(fixtures::home());
        fixtures::api_key(home.path(), "sk-test-unknown-old");
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let h = harness_login(Mode::Current, vec![wiremock::ResponseTemplate::new(401)
            .set_body_json(serde_json::json!({"error": {"message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))],
            &[("continue", "false"), ("human_grace_s", "0")], &[], Some(0),
            Some(TestLogin::Manager { manager: Arc::clone(&manager), home: Arc::clone(&home) }),
            Timing { min_work: Duration::ZERO, ..Timing::default() }, |_| {}, None).await;
        h.test
            .submit_turn("An unavailable login is not a provider change.")
            .await
            .expect("wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        std::fs::remove_file(home.path().join("auth.json")).expect("scratch credential removed");
        manager.reload().await;
        assert!(manager.auth_cached().is_none());
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        db.thread_goals()
            .replace_thread_goal(
                id,
                "Unrelated paused human goal",
                codex_state::ThreadGoalStatus::Paused,
                None,
            )
            .await
            .expect("existing paused goal for native hold");
        db.thread_goals()
            .alab_record_auth_standalone(id, &wall)
            .await
            .expect("predecessor proof");
        db.thread_goals()
            .alab_defer_auth_recovery(id, &wall)
            .await
            .expect("old auth deferral");
        let owed = OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall.clone(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: true,
                account: "a".repeat(64),
                user: "b".repeat(64),
                receipt: None,
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: None,
            }),
            rule: Some("login-back@v1".into()),
            ..Default::default()
        };
        h.shared.set_owed_act(Some(owed.clone()));
        let session = codex_extension_api::ExtensionData::new(h.thread.clone());
        codex_extension_api::ThreadLifecycleContributor::on_thread_resume(
            h.autopilot.as_ref(),
            codex_extension_api::ThreadResumeInput {
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            },
        )
        .await;
        assert_eq!(
            h.shared.owed_act(),
            Some(owed),
            "missing auth cannot revoke known custody"
        );
        assert!(
            db.thread_goals()
                .alab_auth_goal_context(id, &wall)
                .await
                .expect("old witness")
                .is_some()
        );
        assert!(
            db.thread_goals()
                .has_thread_goal_continuation_deferral(id)
                .await
                .expect("old hold")
        );
        assert_eq!(h.mock.requests().len(), 1);
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn missing_custody_database_keeps_intent_even_after_proven_provider_change() {
    big_stack(|| async {
        for goal_bound in [false, true] {
            let broken = fixtures::home();
            let unavailable = broken.path().join("not-a-database-directory");
            std::fs::write(&unavailable, b"fixture").expect("block DB directory");
            let h = harness_login(Mode::Current, vec![wiremock::ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"error": {"message":"fixture provider expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))],
                &[("continue", "false"), ("human_grace_s", "0")], &[], Some(0), None,
                Timing { min_work: Duration::ZERO, ..Timing::default() }, move |config| {
                    config.sqlite = codex_state::SqliteConfig::new_for_testing(unavailable.try_into().expect("absolute scratch DB path"));
                    config.model_provider.name = "Explicit replacement provider".into();
                    config.model_provider.requires_openai_auth = false;
                    config.model_provider.experimental_bearer_token = Some("fixture-provider".into());
                }, None).await;
            assert!(
                h.test.codex.state_db().is_none(),
                "actual DB unavailable fixture"
            );
            h.test
                .submit_turn("The predecessor's recovery DB is unavailable.")
                .await
                .expect("wall");
            let wall = h.shared.latest_turn(&h.thread).expect("wall");
            let owed = OwedAct {
                thread: h.thread.clone(),
                native_origin: Some(NativeOrigin {
                    turn_id: wall,
                    phase: NativePhase::Complete,
                }),
                renewal: Some(AccountHandoff {
                    purpose: codex_alab_core::account_handoff::Purpose::Login,
                    destination: None,
                    openai_scope: true,
                    local: true,
                    account: "a".repeat(64),
                    user: "b".repeat(64),
                    receipt: None,
                    turn: None,
                    accepted: false,
                    starts: 1,
                    goal_id: goal_bound.then(|| "prior-goal".into()),
                }),
                rule: Some("login-back@v1".into()),
                ..Default::default()
            };
            h.shared.set_owed_act(Some(owed.clone()));
            let session = codex_extension_api::ExtensionData::new(h.thread.clone());
            codex_extension_api::ThreadLifecycleContributor::on_thread_resume(
                h.autopilot.as_ref(),
                codex_extension_api::ThreadResumeInput {
                    session_store: &session,
                    thread_store: h.test.codex.thread_extension_data(),
                },
            )
            .await;
            assert_eq!(
                h.shared.owed_act(),
                Some(owed),
                "cannot forget potentially durable custody until DB comparison is possible"
            );
            assert_eq!(h.mock.requests().len(), 1);
            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}

#[test]
fn live_auth_mode_change_retires_existing_intent_before_new_recovery_gates() {
    big_stack(|| async {
        let refresh = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error":"invalid_grant"})),
            )
            .mount(&refresh)
            .await;
        let home = Arc::new(fixtures::home());
        fixtures::chatgpt(
            home.path(),
            "live-user",
            "live-account",
            "live-old",
            Some(&refresh.uri()),
        );
        let manager =
            fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
        let refusal = || {
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
            "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
        };
        let h = harness_login(
            Mode::Current,
            vec![refusal(), refusal()],
            &[
                ("continue", "false"),
                ("human_grace_s", "0"),
                ("logins", "off"),
            ],
            &[],
            Some(0),
            Some(TestLogin::Manager {
                manager: Arc::clone(&manager),
                home: Arc::clone(&home),
            }),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |_| {},
            Some(Arc::new(codex_goal_extension::GoalService::new())),
        )
        .await;
        h.test
            .submit_turn("Original subscription request.")
            .await
            .expect("wall");
        let wall = h.shared.latest_turn(&h.thread).expect("wall");
        let db = h.test.codex.state_db().expect("DB");
        let id = h.test.session_configured.thread_id;
        db.thread_goals()
            .replace_thread_goal(
                id,
                "Unrelated paused human goal",
                codex_state::ThreadGoalStatus::Paused,
                None,
            )
            .await
            .expect("existing paused goal for native hold");
        db.thread_goals()
            .alab_record_auth_standalone(id, &wall)
            .await
            .expect("original proof");
        db.thread_goals()
            .alab_defer_auth_recovery(id, &wall)
            .await
            .expect("owned old hold");
        h.shared.set_owed_act(Some(OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall.clone(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: true,
                account: "a".repeat(64),
                user: "b".repeat(64),
                receipt: None,
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: None,
            }),
            rule: Some("login-back@v1".into()),
            ..Default::default()
        }));
        fixtures::api_key(home.path(), "sk-test-explicit-mode-change");
        manager.reload().await;
        let session = codex_extension_api::ExtensionData::new(h.thread.clone());
        codex_extension_api::ThreadLifecycleContributor::on_thread_idle(
            h.autopilot.as_ref(),
            codex_extension_api::ThreadIdleInput {
                cause: codex_extension_api::ThreadIdleCause::Failed,
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
            },
        )
        .await;
        wait_until("live mode change releases obsolete intent", || {
            h.shared.owed_act().is_none()
        })
        .await;
        assert!(
            db.thread_goals()
                .alab_auth_goal_context(id, &wall)
                .await
                .expect("old witness")
                .is_none()
        );
        assert!(
            !db.thread_goals()
                .has_thread_goal_continuation_deferral(id)
                .await
                .expect("old hold")
        );
        assert_eq!(
            h.mock.requests().len(),
            2,
            "mode change does not replay old credential work"
        );
        assert!(
            h.fake
                .requests()
                .iter()
                .all(|request| !request.contains("codex-renewal")),
            "cleanup precedes disabled email renewal policy"
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

#[test]
fn same_file_subscription_recovery_without_enabled_goals_keeps_original_input_and_tools() {
    big_stack(|| async {
        for disabled in [false, true] {
            let refresh = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .respond_with(
                    wiremock::ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({"error":"invalid_grant"})),
                )
                .mount(&refresh)
                .await;
            let home = Arc::new(fixtures::home());
            fixtures::chatgpt(
                home.path(),
                "no-goal-user",
                "no-goal-account",
                "before-renewal",
                Some(&refresh.uri()),
            );
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let plan = responses::sse(vec![
                responses::ev_response_created("kept-plan"),
                responses::ev_function_call(
                    "kept-plan-call",
                    "update_plan",
                    r#"{"plan":[{"step":"completed once","status":"completed"}]}"#,
                ),
                responses::ev_completed("kept-plan"),
            ]);
            let refusal = || {
                wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
            };
            let h = harness_login(
                Mode::Current,
                vec![
                    responses::sse_response(plan),
                    refusal(),
                    refusal(),
                    responses::sse_response(reply(
                        "renewed-with-tools",
                        "The original task completes.",
                    )),
                ],
                &[("continue", "false"), ("human_grace_s", "0")],
                &[],
                Some(0),
                Some(TestLogin::Manager {
                    manager,
                    home: Arc::clone(&home),
                }),
                Timing {
                    min_work: Duration::ZERO,
                    login_look: Duration::from_millis(25),
                    ..Timing::default()
                },
                |config| config.update_plan_enabled = true,
                disabled.then(|| Arc::new(codex_goal_extension::GoalService::new())),
            )
            .await;
            if disabled {
                set_goal_runtime_enabled(&h, false).await;
            }
            h.test
                .submit_turn("Keep this original task and the completed plan result.")
                .await
                .expect("original login wall");
            wait_until("original login baseline is observed", || {
                h.autopilot.login_looks() > 0
            })
            .await;
            let wall = h.shared.latest_turn(&h.thread).expect("original turn");
            assert_eq!(h.mock.requests().len(), 3);
            fixtures::chatgpt(
                home.path(),
                "no-goal-user",
                "no-goal-account",
                "after-renewal",
                Some(&refresh.uri()),
            );
            wait_until("renewed original request without enabled goals", || {
                h.mock.requests().len() == 4
            })
            .await;
            wait_for_event(&h.test.codex, |event| {
                matches!(event, EventMsg::TurnComplete(_))
            })
            .await;
            assert_eq!(
                h.shared.latest_turn(&h.thread).as_deref(),
                Some(wall.as_str()),
                "same native ID"
            );
            let before = &h.mock.requests()[1].body_json()["input"];
            let after = &h.mock.requests()[3].body_json()["input"];
            assert_eq!(
                before, after,
                "original prompt and completed tool results are unchanged"
            );
            assert!(
                after.to_string().contains("kept-plan-call"),
                "completed tool output retained"
            );
            assert!(
                !after.to_string().contains(words::LOGIN_BACK),
                "no replacement continuation prompt"
            );
            let db = h.test.codex.state_db().expect("DB");
            let id = h.test.session_configured.thread_id;
            assert!(
                db.thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("goal read")
                    .is_none(),
                "no goal fabricated or activated"
            );
            assert!(
                !db.thread_goals()
                    .has_thread_goal_continuation_deferral(id)
                    .await
                    .expect("deferral"),
                "no future goal hold"
            );
            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}

/// A real ordered lifecycle contributor publishes the cache after core has
/// exhausted its own 401 handling and before Autopilot observes the wall.
struct CacheBeforeWall {
    manager: Arc<codex_login::AuthManager>,
    home: Arc<tempfile::TempDir>,
    issuer: String,
    kind: u8,
    published: AtomicBool,
}

impl codex_extension_api::TurnLifecycleContributor for CacheBeforeWall {
    fn on_turn_error<'a>(
        &'a self,
        _: codex_extension_api::TurnErrorInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            match self.kind {
                0 => fixtures::api_key(self.home.path(), "sk-test-cached-new"),
                1 => fixtures::chatgpt(
                    self.home.path(),
                    "cached-user",
                    "cached-account",
                    "cached-new",
                    Some(&self.issuer),
                ),
                2 => fixtures::chatgpt(
                    self.home.path(),
                    "cached-user",
                    "different-account",
                    "cached-new",
                    Some(&self.issuer),
                ),
                3 => fixtures::chatgpt(
                    self.home.path(),
                    "different-user",
                    "cached-account",
                    "cached-new",
                    Some(&self.issuer),
                ),
                _ => fixtures::api_key(self.home.path(), "sk-test-different-mode"),
            }
            self.manager.reload().await;
            self.published.store(true, Ordering::SeqCst);
        })
    }
}

#[test]
fn fresh_cache_before_wall_recovers_only_the_same_request_identity() {
    big_stack(|| async {
        for (kind, after_autopilot) in (0..5).flat_map(|kind| [(kind, false), (kind, true)]) {
            let refresh = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .respond_with(
                    wiremock::ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({"error":"invalid_grant"})),
                )
                .mount(&refresh)
                .await;
            let home = Arc::new(fixtures::home());
            if kind == 0 {
                fixtures::api_key(home.path(), "sk-test-cached-old");
            } else {
                fixtures::chatgpt(
                    home.path(),
                    "cached-user",
                    "cached-account",
                    "cached-old",
                    Some(&refresh.uri()),
                );
            }
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let hook = Arc::new(CacheBeforeWall {
                manager: Arc::clone(&manager),
                home: Arc::clone(&home),
                issuer: refresh.uri(),
                kind,
                published: AtomicBool::new(false),
            });
            let refusal = || {
                wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
            };
            let attempts = if kind == 0 { 1 } else { 2 };
            let mut replies: Vec<_> = (0..attempts).map(|_| refusal()).collect();
            if kind < 2 {
                replies.push(
                    responses::sse_response(reply(
                        "cached-recovered",
                        "The original request resumes.",
                    ))
                    .set_delay(Duration::from_secs(10)),
                );
            }
            let h = harness_login_with_error_hook(
                Mode::Current,
                replies,
                &[
                    ("continue", "false"),
                    ("human_grace_s", "0"),
                    ("logins", "renew"),
                ],
                &[],
                Some(0),
                Some(TestLogin::Manager { manager, home }),
                Timing {
                    min_work: Duration::ZERO,
                    login_look: Duration::from_millis(25),
                    ..Timing::default()
                },
                |_| {},
                Some(Arc::new(codex_goal_extension::GoalService::new())),
                if after_autopilot {
                    ErrorHooks {
                        after: Some(hook.clone()),
                        ..Default::default()
                    }
                } else {
                    ErrorHooks {
                        before: Some(hook.clone()),
                        ..Default::default()
                    }
                },
            )
            .await;
            let db = h.test.codex.state_db().expect("DB");
            let id = h.test.session_configured.thread_id;
            let goal = db
                .thread_goals()
                .replace_thread_goal(
                    id,
                    "Keep the original cached-auth goal",
                    codex_state::ThreadGoalStatus::Active,
                    None,
                )
                .await
                .expect("original goal");
            h.test
                .submit_turn("Keep this original native input when its cache arrives early.")
                .await
                .expect("original error");
            wait_until(
                "cache publication in the actual ordered error lifecycle",
                || hook.published.load(Ordering::SeqCst),
            )
            .await;
            let wall = h.shared.latest_turn(&h.thread).expect("wall");
            if kind < 2 {
                wait_until("fresh cached same-identity original recovery", || {
                    h.mock.requests().len() == attempts + 1
                })
                .await;
                assert_eq!(
                    h.shared.latest_turn(&h.thread).as_deref(),
                    Some(wall.as_str())
                );
                assert_eq!(
                    h.mock.requests()[0].body_json()["input"],
                    h.mock.requests()[attempts].body_json()["input"],
                    "original request preserved"
                );
                let after = db
                    .thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("restored goal")
                    .expect("goal");
                assert_eq!(
                    after.goal_id, goal.goal_id,
                    "local proof cannot cross goal ownership"
                );
                assert_eq!(after.status, codex_state::ThreadGoalStatus::Active);
            } else {
                assert!(
                    h.fake
                        .wait_for(|fake| fake
                            .attention()
                            .values()
                            .any(|text| text.contains("sign-in was refused or expired")))
                        .await,
                    "mismatched credential remains at the actual login wall"
                );
                assert_eq!(
                    h.mock.requests().len(),
                    attempts,
                    "different account/user/mode cannot recover this request"
                );
                assert!(h.shared.owed_act().and_then(|owed| owed.renewal).is_none());
                let stopped = db
                    .thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("ordinary auth stop")
                    .expect("original goal");
                assert_eq!(stopped.goal_id, goal.goal_id);
                assert_eq!(
                    stopped.status,
                    codex_state::ThreadGoalStatus::Blocked,
                    "ineligible auth still stops its own goal ordinarily"
                );
            }
            assert!(
                h.fake
                    .requests()
                    .iter()
                    .all(|request| !request.contains("codex-renewal prepare")),
                "fresh local recovery never spends email renewal authority"
            );
            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}

struct GoalAndCacheBeforeWall {
    manager: Arc<codex_login::AuthManager>,
    home: Arc<tempfile::TempDir>,
    issuer: String,
    mode_change: bool,
    apply_effects: bool,
    target: std::sync::OnceLock<(
        Arc<codex_state::StateRuntime>,
        ThreadId,
        Arc<codex_goal_extension::GoalRuntimeHandle>,
    )>,
    original: std::sync::OnceLock<String>,
    replacement: std::sync::OnceLock<String>,
}

impl codex_extension_api::TurnLifecycleContributor for GoalAndCacheBeforeWall {
    fn on_turn_error<'a>(
        &'a self,
        input: codex_extension_api::TurnErrorInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if self.original.set(input.turn_id.into()).is_err() {
                return;
            }
            let (db, id, runtime) = self
                .target
                .get()
                .unwrap_or_else(|| panic!("fixture bound before work"));
            let replacement = db
                .thread_goals()
                .replace_thread_goal(
                    *id,
                    "Replacement goal owns its own request",
                    codex_state::ThreadGoalStatus::Active,
                    None,
                )
                .await
                .unwrap_or_else(|error| panic!("human replacement: {error}"));
            self.replacement
                .set(replacement.goal_id.clone())
                .unwrap_or_else(|_| panic!("one replacement"));
            if self.apply_effects {
                runtime
                    .apply_external_goal_set(replacement, None)
                    .await
                    .unwrap_or_else(|error| panic!("actual goal runtime effects: {error}"));
            }
            if self.mode_change {
                fixtures::api_key(self.home.path(), "sk-test-replacement-mode");
            } else {
                fixtures::chatgpt(
                    self.home.path(),
                    "ownership-user",
                    "ownership-account",
                    "ownership-new",
                    Some(&self.issuer),
                );
            }
            self.manager.reload().await;
        })
    }
}

#[test]
fn old_auth_error_cannot_block_replacement_goal_after_cache_ownership_changes() {
    big_stack(|| async {
        for (associated, mode_change, apply_effects) in [
            (false, false, true),
            (false, true, true),
            (true, false, true),
            (true, true, true),
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, true, false),
        ] {
            let refresh = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .respond_with(
                    wiremock::ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({"error":"invalid_grant"})),
                )
                .mount(&refresh)
                .await;
            let home = Arc::new(fixtures::home());
            fixtures::chatgpt(
                home.path(),
                "ownership-user",
                "ownership-account",
                "ownership-old",
                Some(&refresh.uri()),
            );
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let hook = Arc::new(GoalAndCacheBeforeWall {
                manager: Arc::clone(&manager),
                home: Arc::clone(&home),
                issuer: refresh.uri(),
                mode_change,
                apply_effects,
                target: std::sync::OnceLock::new(),
                original: std::sync::OnceLock::new(),
                replacement: std::sync::OnceLock::new(),
            });
            let refusal = || {
                wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
            };
            let h = harness_login_with_error_hook(
                Mode::Current,
                vec![
                    refusal(),
                    refusal(),
                    responses::sse_response(reply("replacement-owned", "Working on the new goal."))
                        .set_delay(Duration::from_secs(10)),
                ],
                &[
                    ("continue", "false"),
                    ("human_grace_s", "0"),
                    ("logins", "renew"),
                ],
                &[],
                Some(0),
                Some(TestLogin::Manager { manager, home }),
                Timing {
                    min_work: Duration::ZERO,
                    ..Timing::default()
                },
                |_| {},
                Some(Arc::new(codex_goal_extension::GoalService::new())),
                ErrorHooks {
                    before: Some(hook.clone()),
                    ..Default::default()
                },
            )
            .await;
            let db = h.test.codex.state_db().expect("DB");
            let id = h.test.session_configured.thread_id;
            if associated {
                db.thread_goals()
                    .replace_thread_goal(
                        id,
                        "Original goal A",
                        codex_state::ThreadGoalStatus::Active,
                        None,
                    )
                    .await
                    .expect("goal A");
            }
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("runtime");
            assert!(hook.target.set((Arc::clone(&db), id, runtime)).is_ok());
            h.test
                .submit_turn("The old request must not stop a later human goal.")
                .await
                .expect("original terminal error");
            wait_until("replacement goal starts its own request", || {
                h.mock.requests().len() == 3
            })
            .await;
            let current = db
                .thread_goals()
                .get_thread_goal(id)
                .await
                .expect("goal B")
                .expect("present");
            assert_eq!(Some(&current.goal_id), hook.replacement.get());
            assert_eq!(
                current.status,
                codex_state::ThreadGoalStatus::Active,
                "old typed401 cannot stop a different or later goal"
            );
            assert_ne!(
                h.shared.latest_turn(&h.thread).as_ref(),
                hook.original.get(),
                "replacement owns a fresh native request"
            );
            let texts = h.mock.requests()[2].message_input_texts("user");
            assert!(
                texts
                    .last()
                    .expect("goal input")
                    .contains("Replacement goal owns its own request")
            );
            assert!(
                h.fake
                    .requests()
                    .iter()
                    .all(|request| !request.contains("codex-renewal prepare"))
            );
            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}

struct ClearCacheBeforeWall {
    manager: Arc<codex_login::AuthManager>,
    home: Arc<tempfile::TempDir>,
    cleared: AtomicBool,
}

impl codex_extension_api::TurnLifecycleContributor for ClearCacheBeforeWall {
    fn on_turn_error<'a>(
        &'a self,
        _: codex_extension_api::TurnErrorInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            std::fs::remove_file(self.home.path().join("auth.json")).unwrap_or_else(|error| {
                panic!("fixture transiently unavailable auth file: {error}")
            });
            self.manager.reload().await;
            assert!(self.manager.auth_cached().is_none());
            self.cleared.store(true, Ordering::SeqCst);
        })
    }
}

/// Missing cached auth retains only observation and original causal custody.
/// Recovery works across each real lifecycle boundary; unchanged bearers,
/// identity changes and human edits never acquire execution authority.
#[test]
fn fresh_cache_after_temporary_none_between_error_hooks_recovers_original_goal() {
    big_stack(|| async {
        // Before Goal, after Goal, after completion; then old-bearer metadata,
        // other account, other mode, human goal replacement, and still missing.
        // Finally disappear after Autopilot, then restore after Goal / after finish.
        for phase in 0..12 {
            let refresh = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .respond_with(
                    wiremock::ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({"error":"invalid_grant"})),
                )
                .mount(&refresh)
                .await;
            let home = Arc::new(fixtures::home());
            fixtures::chatgpt(
                home.path(),
                "cached-user",
                "cached-account",
                "cached-old",
                Some(&refresh.uri()),
            );
            let original_auth: codex_login::AuthDotJson = serde_json::from_slice(
                &std::fs::read(home.path().join("auth.json")).expect("fixture auth"),
            )
            .expect("original auth snapshot");
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let clear = Arc::new(ClearCacheBeforeWall {
                manager: Arc::clone(&manager),
                home: Arc::clone(&home),
                cleared: AtomicBool::new(false),
            });
            let publish = Arc::new(CacheBeforeWall {
                manager: Arc::clone(&manager),
                home: Arc::clone(&home),
                issuer: refresh.uri(),
                kind: 1,
                published: AtomicBool::new(false),
            });
            let refusal = || {
                wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
            "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
            };
            let mut replies = vec![refusal(), refusal()];
            if !(3..9).contains(&phase) {
                replies.push(
                    responses::sse_response(reply(
                        "none-cache-restored",
                        "The original work resumes.",
                    ))
                    .set_delay(Duration::from_secs(10)),
                );
            }
            let h =
                harness_login_with_error_hook(
                    Mode::Current,
                    replies,
                    &[
                        ("continue", "false"),
                        ("human_grace_s", "0"),
                        ("logins", if phase == 8 { "renew" } else { "ask" }),
                    ],
                    &[],
                    Some(0),
                    Some(TestLogin::Manager {
                        manager: Arc::clone(&manager),
                        home: Arc::clone(&home),
                    }),
                    Timing {
                        min_work: Duration::ZERO,
                        login_look: Duration::from_millis(25),
                        ..Timing::default()
                    },
                    |_| {},
                    Some(Arc::new(codex_goal_extension::GoalService::new())),
                    ErrorHooks {
                        before: (phase < 10).then(|| {
                            clear.clone() as Arc<dyn codex_extension_api::TurnLifecycleContributor>
                        }),
                        after: if phase >= 10 {
                            Some(clear.clone()
                                as Arc<dyn codex_extension_api::TurnLifecycleContributor>)
                        } else {
                            (phase == 0).then(|| {
                                publish.clone()
                                    as Arc<dyn codex_extension_api::TurnLifecycleContributor>
                            })
                        },
                        after_goal: (phase == 1 || phase == 10).then(|| {
                            publish.clone()
                                as Arc<dyn codex_extension_api::TurnLifecycleContributor>
                        }),
                    },
                )
                .await;
            let db = h.test.codex.state_db().expect("DB");
            let id = h.test.session_configured.thread_id;
            let goal = db
                .thread_goals()
                .replace_thread_goal(
                    id,
                    "Keep this original goal through transiently unavailable auth",
                    codex_state::ThreadGoalStatus::Active,
                    None,
                )
                .await
                .expect("original goal");
            h.test
                .submit_turn("Keep the original task while the same identity's cache is restored.")
                .await
                .expect("first error");
            assert!(clear.cleared.load(Ordering::SeqCst));
            if phase >= 2 && phase != 10 {
                wait_until("observation-only watch after native completion", || {
                    h.autopilot.login_looks() > 0
                })
                .await;
                assert!(manager.auth_cached().is_none());
                assert_eq!(h.mock.requests().len(), 2, "unknown auth cannot execute");
                assert!(
                    h.fake
                        .requests()
                        .iter()
                        .all(|request| !request.contains("codex-renewal prepare")),
                    "unknown auth cannot email"
                );
                if phase == 6 {
                    db.thread_goals()
                        .replace_thread_goal(
                            id,
                            "Human replacement stays paused",
                            codex_state::ThreadGoalStatus::Paused,
                            None,
                        )
                        .await
                        .expect("human edit");
                }
                if phase == 8 {
                    let wall = h.shared.latest_turn(&h.thread).expect("wall");
                    let identity = codex_alab_aterm::renewal::Identity::from_native(
                        "cached-account",
                        "cached-user",
                    )
                    .expect("identity");
                    h.fake.reply_once(
                        &format!(
                            "@{} codex-renewal prepare thread={} wall={wall} account={} user={}",
                            h.fake.sid(),
                            h.thread,
                            identity.account,
                            identity.user
                        ),
                        &format!("OK codex-renewal state=ready id=0123456789abcdef0123456789abcdef thread={} wall={wall} account={} user={}\n", h.thread, identity.account, identity.user),
                    );
                    h.fake.reply_once(&format!("@{} codex-renewal commit id=0123456789abcdef0123456789abcdef", h.fake.sid()),
                        &format!("OK codex-renewal state=waiting id=0123456789abcdef0123456789abcdef thread={} wall={wall} account={} user={}\n", h.thread, identity.account, identity.user));
                }
                match phase {
                    3 | 8 => {
                        let mut metadata_only = original_auth.clone();
                        let tokens = metadata_only.tokens.as_mut().expect("tokens");
                        tokens.refresh_token =
                            "different-refresh-with-the-SAME-refused-bearer".into();
                        tokens.login_owner_id =
                            Some("different-owner-with-the-SAME-refused-bearer".into());
                        metadata_only.last_refresh = Some(chrono::Utc::now());
                        codex_login::save_auth(
                            home.path(),
                            &metadata_only,
                            codex_login::AuthCredentialsStoreMode::File,
                            codex_login::AuthKeyringBackendKind::default(),
                        )
                        .expect("metadata only");
                    }
                    4 => fixtures::chatgpt(
                        home.path(),
                        "other-user",
                        "other-account",
                        "other-fresh",
                        Some(&refresh.uri()),
                    ),
                    5 => fixtures::api_key(home.path(), "sk-test-other-mode"),
                    7 => {}
                    9 => fixtures::chatgpt(
                        home.path(),
                        "",
                        "",
                        "unknown-profile",
                        Some(&refresh.uri()),
                    ),
                    _ => fixtures::chatgpt(
                        home.path(),
                        "cached-user",
                        "cached-account",
                        "cached-new",
                        Some(&refresh.uri()),
                    ),
                }
                manager.reload().await;
                if phase == 9 {
                    assert!(
                        manager.auth_cached().is_some(),
                        "missing identity is distinct from missing cached auth"
                    );
                    let looks = h.autopilot.login_looks();
                    wait_until("unknown identity remains observational", || {
                        h.autopilot.login_looks() >= looks + 3
                    })
                    .await;
                    assert_eq!(h.mock.requests().len(), 2);
                    assert!(
                        h.fake
                            .requests()
                            .iter()
                            .all(|request| !request.contains("codex-renewal prepare"))
                    );
                    fixtures::chatgpt(
                        home.path(),
                        "cached-user",
                        "cached-account",
                        "known-after-unknown-profile",
                        Some(&refresh.uri()),
                    );
                    manager.reload().await;
                }
            } else {
                assert!(publish.published.load(Ordering::SeqCst));
            }
            let wall = h.shared.latest_turn(&h.thread).expect("original wall");
            if !(3..9).contains(&phase) {
                wait_until(
                    "same-identity restored cache resumes original work after temporary None",
                    || h.mock.requests().len() == 3,
                )
                .await;
                assert_eq!(
                    h.shared.latest_turn(&h.thread).as_deref(),
                    Some(wall.as_str())
                );
                let current = db
                    .thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("goal")
                    .expect("present");
                assert_eq!(current.goal_id, goal.goal_id);
                assert_eq!(current.status, codex_state::ThreadGoalStatus::Active);
                assert_eq!(
                    h.mock.requests()[0].body_json()["input"],
                    h.mock.requests()[2].body_json()["input"]
                );
            } else {
                if phase == 8 {
                    assert!(
                        h.fake
                            .wait_for(|fake| fake
                                .requests()
                                .iter()
                                .any(|request| request.contains("codex-renewal prepare")))
                            .await,
                        "known original profile can request native renewal"
                    );
                    wait_until(
                        "bound native ready receipt retained while commit is waiting",
                        || {
                            h.shared.owed_act().is_some_and(|owed| {
                                owed.native_origin
                                    .as_ref()
                                    .is_some_and(|origin| origin.turn_id == wall)
                                    && owed.renewal.is_some_and(|renewal| {
                                        !renewal.local
                                            && !renewal.accepted
                                            && renewal.receipt.as_deref()
                                                == Some("0123456789abcdef0123456789abcdef")
                                    })
                            })
                        },
                    )
                    .await;
                    assert_eq!(
                        h.fake
                            .requests()
                            .iter()
                            .filter(|request| request.contains("codex-renewal prepare"))
                            .count(),
                        1
                    );
                    assert!(
                        h.fake
                            .wait_for(|fake| fake
                                .requests()
                                .iter()
                                .any(|request| request.contains("codex-renewal commit")))
                            .await,
                        "bound Ready receipt reaches native commit"
                    );
                    assert_eq!(
                        h.fake
                            .requests()
                            .iter()
                            .filter(|request| request.contains("codex-renewal commit"))
                            .count(),
                        1
                    );
                    let native_requests = h.fake.requests();
                    let prepare = native_requests
                        .iter()
                        .position(|request| request.contains("codex-renewal prepare"))
                        .expect("prepare");
                    let commit = native_requests
                        .iter()
                        .position(|request| request.contains("codex-renewal commit"))
                        .expect("commit");
                    assert!(
                        native_requests[prepare + 1..commit]
                            .iter()
                            .all(|request| !request.contains("meta set supervisor")),
                        "Ready's sealed native claim cannot be reconfirmed before commit"
                    );
                } else if phase == 3 {
                    wait_until(
                        "unchanged bearer has completed its known-profile evaluation",
                        || {
                            h.autopilot.supervisor().journal().iter().any(|line| {
                                line.contains("checked the original profile's native renewal")
                            })
                        },
                    )
                    .await;
                } else if phase == 6 {
                    wait_until("fresh cache has rechecked the human-edited goal", || {
                        h.autopilot
                            .supervisor()
                            .journal()
                            .iter()
                            .any(|line| line.contains("looked again at the lost login's return"))
                    })
                    .await;
                } else if phase == 7 {
                    let looks = h.autopilot.login_looks();
                    wait_until("missing cache stays observed without authority", || {
                        h.autopilot.login_looks() >= looks + 3
                    })
                    .await;
                } else {
                    // Mismatch stops observation; bounded negative after the
                    // actual account/mode publication and folder event.
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
                assert_eq!(
                    h.mock.requests().len(),
                    2,
                    "phase {phase}: no fresh authorized bearer/original goal"
                );
                let actual_model_requests = h
                    ._server
                    .received_requests()
                    .await
                    .expect("server requests")
                    .into_iter()
                    .filter(|request| {
                        request.method.as_str() == "POST" && request.url.path() == "/v1/responses"
                    })
                    .count();
                assert_eq!(
                    actual_model_requests, 2,
                    "phase {phase}: no unmatched third request beyond the mock response cap"
                );
                let current = db
                    .thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("goal")
                    .expect("present");
                if phase == 6 {
                    assert_ne!(current.goal_id, goal.goal_id);
                    assert_eq!(current.status, codex_state::ThreadGoalStatus::Paused);
                } else {
                    assert_eq!(current.goal_id, goal.goal_id);
                    assert_eq!(current.status, codex_state::ThreadGoalStatus::Blocked);
                }
            }
            if phase != 8 {
                assert!(
                    h.fake
                        .requests()
                        .iter()
                        .all(|request| !request.contains("codex-renewal prepare"))
                );
            }

            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}

#[test]
fn new_native_work_retires_a_passive_capture_left_before_intent_persistence() {
    big_stack(|| async {
        for disabled in [false, true] {
            let refresh = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .respond_with(
                    wiremock::ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({"error":"invalid_grant"})),
                )
                .mount(&refresh)
                .await;
            let home = Arc::new(fixtures::home());
            fixtures::chatgpt(
                home.path(),
                "orphan-user",
                "orphan-account",
                "orphan-old",
                Some(&refresh.uri()),
            );
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let refusal = || {
                wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {
                "message":"fixture auth expired", "type":"invalid_request_error", "code":"invalid_api_key"}}))
            };
            let h = harness_login(
                Mode::Current,
                vec![
                    refusal(),
                    refusal(),
                    refusal(),
                    refusal(),
                    responses::sse_response(reply(
                        "orphan-successor",
                        "The new native task completes.",
                    )),
                ],
                &[("continue", "false"), ("human_grace_s", "0")],
                &[],
                Some(0),
                Some(TestLogin::Manager {
                    manager,
                    home: Arc::clone(&home),
                }),
                Timing {
                    min_work: Duration::ZERO,
                    login_look: Duration::from_millis(25),
                    ..Timing::default()
                },
                |_| {},
                disabled.then(|| Arc::new(codex_goal_extension::GoalService::new())),
            )
            .await;
            if disabled {
                set_goal_runtime_enabled(&h, false).await;
            }
            h.test
                .submit_turn("The first native work leaves an unwritten intent.")
                .await
                .expect("wall A");
            wait_until("A wall is observed", || h.autopilot.login_looks() > 0).await;
            let old = h.shared.latest_turn(&h.thread).expect("A");
            let db = h.test.codex.state_db().expect("DB");
            let id = h.test.session_configured.thread_id;
            assert!(
                db.thread_goals()
                    .alab_capture_auth_standalone(id, &old)
                    .await
                    .expect("captured before intent persistence")
            );
            assert!(
                h.shared.owed_act().and_then(|owed| owed.renewal).is_none(),
                "no intent was persisted"
            );
            h.test
                .submit_turn("New authorized native work supersedes the unwritten intent.")
                .await
                .expect("wall B");
            let new = h.shared.latest_turn(&h.thread).expect("B");
            assert_ne!(old, new);
            assert!(
                db.thread_goals()
                    .alab_auth_goal_context(id, &old)
                    .await
                    .expect("old proof")
                    .is_none()
            );
            assert!(
                !db.thread_goals()
                    .alab_capture_auth_standalone(id, &old)
                    .await
                    .expect("old capture cannot return")
            );
            fixtures::chatgpt(
                home.path(),
                "orphan-user",
                "orphan-account",
                "orphan-new",
                Some(&refresh.uri()),
            );
            wait_until("new original native B recovery", || {
                h.mock.requests().len() == 5
            })
            .await;
            wait_for_event(&h.test.codex, |event| {
                matches!(event, EventMsg::TurnComplete(_))
            })
            .await;
            assert_eq!(
                h.shared.latest_turn(&h.thread).as_deref(),
                Some(new.as_str())
            );
            assert_eq!(
                h.mock.requests()[2].body_json()["input"],
                h.mock.requests()[4].body_json()["input"]
            );
            assert!(
                db.thread_goals()
                    .get_thread_goal(id)
                    .await
                    .expect("goal read")
                    .is_none()
            );
            assert!(
                !db.thread_goals()
                    .has_thread_goal_continuation_deferral(id)
                    .await
                    .expect("deferral")
            );
            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}
