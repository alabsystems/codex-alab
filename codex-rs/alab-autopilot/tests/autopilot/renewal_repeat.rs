// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// A successful standalone recovery must not leave custody that prevents the
/// next independent failed turn from recovering in the same live session.
#[test]
fn consecutive_standalone_login_walls_recover_without_enabled_goals() {
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
                "repeat-user",
                "repeat-account",
                "initial",
                Some(&refresh.uri()),
            );
            let manager =
                fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
            let refusal = || {
                wiremock::ResponseTemplate::new(401).set_body_json(
                    serde_json::json!({"error": {"message":"fixture auth expired",
                    "type":"invalid_request_error", "code":"invalid_api_key"}}),
                )
            };
            let h = harness_login(
                Mode::Current,
                vec![
                    refusal(),
                    refusal(),
                    responses::sse_response(reply("first-recovered", "First task done.")),
                    refusal(),
                    refusal(),
                    responses::sse_response(reply("second-recovered", "Second task done.")),
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
            let mut previous = None;
            for (index, (prompt, credential)) in [
                ("Complete the first independent task.", "first-renewal"),
                ("Complete the second independent task.", "second-renewal"),
            ]
            .into_iter()
            .enumerate()
            {
                let before_looks = h.autopilot.login_looks();
                h.test.submit_turn(prompt).await.expect("actual auth wall");
                let wall = h.shared.latest_turn(&h.thread).expect("failed native turn");
                if let Some(previous) = &previous {
                    assert_ne!(&wall, previous, "the second task is a new native turn");
                }
                assert_eq!(
                    h.mock.requests().len(),
                    index * 3 + 2,
                    "core exhausted this credential"
                );
                wait_until("current wall is watched", || {
                    h.autopilot.login_looks() > before_looks
                })
                .await;
                fixtures::chatgpt(
                    home.path(),
                    "repeat-user",
                    "repeat-account",
                    credential,
                    Some(&refresh.uri()),
                );
                wait_until("current original request recovered", || {
                    h.mock.requests().len() == (index + 1) * 3
                })
                .await;
                wait_for_event(&h.test.codex, |event| {
                    matches!(event, EventMsg::TurnComplete(_))
                })
                .await;
                wait_until("recovered native task completed", || {
                    !h.shared.turn_running_on(&h.thread)
                })
                .await;
                assert_eq!(
                    h.shared.latest_turn(&h.thread).as_deref(),
                    Some(wall.as_str())
                );
                let requests = h.mock.requests();
                assert_eq!(
                    requests[index * 3].body_json()["input"],
                    requests[index * 3 + 2].body_json()["input"],
                    "recovery keeps exactly the failed request's history and input"
                );
                assert!(
                    !requests[index * 3 + 2].body_json()["input"]
                        .to_string()
                        .contains(words::LOGIN_BACK),
                    "no replacement prompt"
                );
                let db = h.test.codex.state_db().expect("state database");
                let id = h.test.session_configured.thread_id;
                assert!(
                    db.thread_goals()
                        .get_thread_goal(id)
                        .await
                        .expect("goal read")
                        .is_none(),
                    "standalone recovery creates no goal"
                );
                assert!(
                    !db.thread_goals()
                        .has_thread_goal_continuation_deferral(id)
                        .await
                        .expect("deferral read"),
                    "completed recovery leaves no future goal hold"
                );
                previous = Some(wall);
            }
            h.test.codex.shutdown_and_wait().await.expect("shutdown");
        }
    });
}
