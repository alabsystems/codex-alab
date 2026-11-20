// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::renewal_admission::subscription_fixture_with_refresh;
use super::*;
use codex_alab_tui::record;
use serde_json::Value;

const LIMIT: &str = "codex_other";
const MODEL: &str = "gpt-5.5";
const PROMPT: &str = "Make the plan and complete the original task.";
const CALL: &str = "named-limit-plan-call";

/// Serve the same 429 as the provider fixture, with each window moved to this run's clock.
fn model_refusal() -> wiremock::ResponseTemplate {
    let mut refusal: Value = serde_json::from_str(include_str!(
        "../../../../alab/smoke/fixtures/provider/refusal-model-limit.json"
    ))
    .expect("named model refusal");
    let headers = refusal["headers"].as_object().unwrap();
    assert_eq!(headers["x-codex-active-limit"], LIMIT);
    assert_eq!(headers["x-codex-primary-used-percent"], "20");
    assert_eq!(headers["x-codex-secondary-used-percent"], "10");
    assert_eq!(headers["x-codex-other-primary-used-percent"], "100");
    let reset = refusal["body"]["error"]["resets_at"].as_i64().unwrap();
    let shift = (now_ms() / 1000) as i64 + 7200 - reset;
    let mut response = wiremock::ResponseTemplate::new(429);
    for (key, value) in headers {
        let value = value.as_str().unwrap();
        let value = if key.ends_with("-reset-at") {
            (value.parse::<i64>().unwrap() + shift).to_string()
        } else {
            value.to_owned()
        };
        response = response.insert_header(key.as_str(), value);
    }
    refusal["body"]["error"]["resets_at"] = (reset + shift).into();
    response.set_body_json(&refusal["body"])
}

struct Refused {
    h: Harness,
    source: Identity,
    turn: String,
    record: std::path::PathBuf,
    _refresh: wiremock::MockServer,
}

async fn refused(named_capability: bool) -> Refused {
    let (login, source, refresh) =
        subscription_fixture_with_refresh("named-source", "named-source").await;
    let plan = responses::sse(vec![
        responses::ev_response_created("plan"),
        responses::ev_function_call(
            CALL,
            "update_plan",
            r#"{"plan":[{"step":"finish once","status":"completed"}]}"#,
        ),
        responses::ev_completed("plan"),
    ]);
    let mut policy = ROTATION.to_vec();
    if named_capability {
        policy.push(("codex_rotation_limits", "1"));
    }
    let mut replies = vec![responses::sse_response(plan), model_refusal()];
    if named_capability {
        replies.push(responses::sse_response(reply(
            "finished",
            "The original task completed.",
        )));
    }
    let h = harness_login(
        Mode::Current,
        replies,
        &policy,
        &[],
        Some(0),
        Some(login),
        quick(),
        |config| config.model = Some(MODEL.to_owned()),
        None,
    )
    .await;
    let record = install_writer(h.shared, h._dir.path().join("records"));
    h.fake.gate("codex-rotation prepare");
    assert!(
        h.fake
            .wait_for(|fake| fake.claim().is_some_and(|claim| claim.proven_pid.is_some()))
            .await
    );
    h.test.submit_text_turn(PROMPT).await.expect("refused turn");
    let turn = h.shared.latest_turn(&h.thread).expect("original turn id");
    assert_eq!(h.mock.requests().len(), 2);
    Refused {
        h,
        source,
        turn,
        record,
        _refresh: refresh,
    }
}

#[test]
fn named_limit_is_carried_from_http_refusal_to_the_original_turn_on_the_destination() {
    big_stack(|| async {
        let refused = refused(true).await;
        let h = &refused.h;
        let turn = &refused.turn;
        let thread = &h.thread;
        let prepare = format!(
            "@{} {VERB} prepare thread={thread} turn={turn} source_account={} source_user={} limit={LIMIT}",
            h.fake.sid(),
            refused.source.account,
            refused.source.user
        );
        wait_until("named rotation prepare", || {
            !h.fake.gated_requests().is_empty()
        })
        .await;
        assert_eq!(h.fake.gated_requests(), [prepare.clone()]);
        let on_disk = || record::read(&refused.record).expect("flushed source record");
        let unbound = on_disk().owed.unwrap().rotation.unwrap();
        assert_eq!(unbound.limit.as_deref(), Some(LIMIT));
        assert!(unbound.receipt.is_none());

        let (login, destination, _refresh) =
            subscription_fixture_with_refresh("named-target", "named-target").await;
        let TestLogin::Manager {
            manager,
            home: _target_home,
        } = login
        else {
            panic!("file-backed target");
        };
        let id = receipt_id();
        let receipt = format!(
            "id={id} thread={thread} turn={turn} source_account={} source_user={} target_account={} target_user={}",
            refused.source.account, refused.source.user, destination.account, destination.user
        );
        let commit = format!("@{} {VERB} commit id={id}", h.fake.sid());
        h.fake.gate("codex-rotation commit");
        h.fake
            .reply_once(&prepare, &format!("OK {VERB} state=ready {receipt}\n"));
        h.fake.open_gate("codex-rotation prepare");
        wait_until("receipt bound before native commit", || {
            h.fake.gated_requests().contains(&commit)
        })
        .await;
        let source_record = on_disk();
        let owed = source_record.owed.expect("durable handoff");
        let rotation = owed.rotation.as_ref().expect("rotation");
        assert_eq!(rotation.limit.as_deref(), Some(LIMIT));
        assert_eq!(rotation.receipt.as_deref(), Some(id.as_str()));
        assert_eq!(
            rotation
                .destination
                .as_ref()
                .map(|target| target.account.as_str()),
            Some(destination.account.as_str())
        );
        assert!(source_record.recovery.is_none());
        h.fake
            .reply_once(&commit, &format!("OK {VERB} state=committed {receipt}\n"));
        let status = format!("@{} {VERB} status id={id}", h.fake.sid());
        h.fake
            .reply_always(&status, &format!("OK {VERB} state=committed {receipt}\n"));
        h.fake.open_gate("codex-rotation commit");
        wait_until("the source checked its committed receipt", || {
            h.fake.requests().contains(&status)
        })
        .await;

        let resumed = restore_rotation(h, owed.into(), turn, &receipt, manager, |host| {
            host.set_policy_key("codex_rotation_limits", "1");
            host.gate("codex-rotation consume");
        })
        .await;
        let consume = format!("@{} {VERB} consume id={id} turn={turn}", resumed.fake.sid());
        wait_until(
            "the destination accepted the original turn before consume",
            || resumed.fake.gated_requests().contains(&consume),
        )
        .await;
        let accepted = record::read(&resumed.record)
            .unwrap()
            .owed
            .unwrap()
            .rotation
            .unwrap();
        assert!(accepted.accepted);
        assert_eq!(accepted.turn.as_ref(), Some(turn));
        assert_eq!(accepted.limit.as_deref(), Some(LIMIT));
        assert_eq!(accepted.receipt.as_deref(), Some(id.as_str()));
        resumed.fake.open_gate("codex-rotation consume");
        let complete = format!(
            "@{} {VERB} complete id={id} turn={turn}",
            resumed.fake.sid()
        );
        wait_until(
            "the resumed original turn was consumed and completed",
            || resumed.fake.requests().contains(&complete),
        )
        .await;
        let host = resumed.fake.requests();
        assert!(
            host.iter().position(|r| r == &consume).unwrap()
                < host.iter().position(|r| r == &complete).unwrap()
        );
        assert_eq!(resumed.shared.latest_turn(thread).as_ref(), Some(turn));
        let requests = h.mock.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1].input(), requests[2].input());
        for (request, label) in
            requests
                .iter()
                .zip(["named-source", "named-source", "named-target"])
        {
            let account = format!("{label}-account");
            assert_eq!(request.body_json()["model"].as_str(), Some(MODEL));
            assert!(
                request.header("authorization").as_deref()
                    == Some(format!("Bearer access-for-{label}").as_str()),
                "the request used {label}'s fixture credential"
            );
            assert_eq!(
                request.header("chatgpt-account-id").as_deref(),
                Some(account.as_str())
            );
            assert_eq!(
                request.body_json()["client_metadata"]["turn_id"].as_str(),
                Some(turn.as_str())
            );
        }
        assert_eq!(
            requests[2]
                .message_input_texts("user")
                .iter()
                .filter(|text| text.as_str() == PROMPT)
                .count(),
            1
        );
        for kind in ["function_call", "function_call_output"] {
            assert_eq!(
                requests[2]
                    .inputs_of_type(kind)
                    .iter()
                    .filter(|item| item["call_id"] == CALL)
                    .count(),
                1
            );
        }
    });
}

#[test]
fn named_limit_on_an_older_host_keeps_the_usage_wait_without_a_prepare() {
    big_stack(|| async {
        let refused = refused(false).await;
        let h = &refused.h;
        wait_until("the old host leaves a paced usage wait", || {
            journal_has(h, "s for usage-resume@v1")
        })
        .await;
        // Ordinary rotation is enabled under the same policy as the positive case.
        for request in h.fake.requests().iter().chain(&h.fake.gated_requests()) {
            assert!(!request.contains(" codex-rotation prepare "), "{request}");
        }
        assert!(h.shared.flush_record());
        let saved = record::read(&refused.record).expect("saved usage wait");
        assert!(saved.owed.and_then(|owed| owed.rotation).is_none());
        let recovery: codex_alab_core::session::Recovery = saved.recovery.expect("recovery").into();
        assert_eq!(recovery.turn_id, refused.turn);
        let codex_alab_core::session::RecoveryWall::Usage {
            quota: Some(quota), ..
        } = recovery.wall
        else {
            panic!("authenticated quota wait");
        };
        assert_eq!(quota.bucket, LIMIT);
        assert_eq!(quota.account, "named-source-account");
        assert_eq!(h.mock.requests().len(), 2);
    });
}
