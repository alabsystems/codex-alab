// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real foreground compaction requests, authenticated usage reads and earned-reset POST.

use super::*;
use pretty_assertions::assert_eq;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::matchers::method;
use wiremock::matchers::path;

const CHILD: &str = "ALAB_WEEKLY_COMPACTION_CHILD";

#[test]
fn weekly_reset_retries_original_local_and_remote_compaction() {
    let (_, local_module) = module_path!().split_once("::").unwrap();
    let exact = format!("{local_module}::weekly_compaction_child");
    let root = tempfile::tempdir().unwrap();
    let guard =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../alab/command_guard.py");
    let evidence = root.path().join("guardian.json");
    let output = std::process::Command::new("/usr/bin/python3")
        .arg(&guard)
        .args(["--", "/usr/bin/python3"])
        .arg(&guard)
        .arg("--result-file")
        .arg(&evidence)
        .args(["--", "--inherited-group-timeout", "60", "--"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD, "foreground")
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("PATH", "/usr/bin:/bin")
        .env("RUST_MIN_STACK", "16777216")
        .current_dir(root.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "private compaction child failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("ALAB_WEEKLY_COMPACTION_OK"));
    let guardian: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence).unwrap()).unwrap();
    assert_eq!(guardian["returncode"], 0);
    assert_eq!(guardian["timed_out"], false);
}

#[tokio::test]
#[expect(
    clippy::print_stdout,
    reason = "private test child completion evidence"
)]
async fn weekly_compaction_child() {
    if std::env::var(CHILD).as_deref() != Ok("foreground")
        || !std::env::args().any(|arg| arg == "--exact")
    {
        return;
    }
    codex_alab_core::activate();
    for (remote, preflight) in [(false, false), (true, false), (false, true), (true, true)] {
        let server = responses::start_mock_server().await;
        let home = tempfile::tempdir().unwrap();
        let restored = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&restored);
        let usage_reads = Arc::new(AtomicUsize::new(0));
        let reset = chrono::Utc::now().timestamp() + 13 * 3600;
        Mock::given(method("GET"))
            .and(path("/api/codex/usage"))
            .respond_with(move |_: &wiremock::Request| {
                // Reactive cases exhaust quota after a healthy preflight. Proactive
                // cases begin under pressure and must redeem before the model request.
                let first_read = usage_reads.fetch_add(1, Ordering::SeqCst) == 0;
                let healthy = (!preflight && first_read) || observed.load(Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({
                    "account_id":"account","user_id":"user","plan_type":"plus",
                    "rate_limit":{"allowed":healthy,"limit_reached":!healthy,
                        "secondary_window":{"used_percent":if healthy {0}else{100},
                            "limit_window_seconds":604800,"reset_at":reset,"reset_after_seconds":46800}}
                }))
            })
            .expect(if preflight { 2 } else { 3 })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/codex/rate-limit-reset-credits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "available_count":1,"credits":[{"id":"earned-compaction",
                    "status":"available","reset_type":"codex_rate_limits",
                    "granted_at":"2026-01-01T00:00:00Z"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let redeemed = Arc::clone(&restored);
        Mock::given(method("POST"))
            .and(path("/api/codex/rate-limit-reset-credits/consume"))
            .respond_with(move |_: &wiremock::Request| {
                redeemed.store(true, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({"code":"reset","windows_reset":2}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let auth = CodexAuth::from_external_chatgpt_tokens(
            "e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture",
            "account", Some("plus"),
        ).unwrap();
        let mut provider = built_in_model_providers(None)["openai"].clone();
        provider.base_url = Some(format!("{}/v1", server.uri()));
        provider.supports_websockets = false;
        let base = server.uri();
        let (session, turn, events) = make_session_and_context_with_auth_config_home_and_rx(
            auth,
            Vec::new(),
            home.path(),
            move |config| {
                config.model = Some("gpt-5.2".into());
                config.model_provider = provider;
                config.chatgpt_base_url = base;
                let _ = config.features.disable(Feature::TokenBudget);
            },
        )
        .await;
        turn.extension_data
            .insert_if(codex_alab_core::quota::HeadlessExec, |old| old.is_none());
        session
            .record_conversation_items(
                &turn,
                turn.model_info(),
                &[user_message("keep this exact compaction input")],
            )
            .await;
        session
            .spawn_task(
                Arc::clone(&turn),
                Vec::new(),
                NeverEndingTask {
                    kind: TaskKind::Regular,
                    listen_to_cancellation_token: true,
                },
            )
            .await;
        let step = session
            .capture_step_context(Arc::clone(&turn), &CancellationToken::new())
            .await
            .unwrap();
        let world = Arc::new(
            session
                .build_world_state_for_step(&step, true)
                .await
                .unwrap(),
        );
        let item = if remote {
            json!({"type":"response.output_item.done","item":{"type":"compaction","encrypted_content":"summary"}})
        } else {
            responses::ev_assistant_message("summary", "retained summary")
        };
        let mut replies = Vec::new();
        if !preflight {
            replies.push(ResponseTemplate::new(429).set_body_json(json!({"error":{
                "type":"usage_limit_reached","message":"Fixture weekly quota exhausted","resets_at":reset
            }})));
        }
        replies.push(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(responses::sse(vec![
                    item,
                    responses::ev_completed("compact-success"),
                ])),
        );
        let requests = responses::mount_response_sequence(&server, replies).await;
        let compact = async {
            if remote {
                crate::compact_remote_v2::run_remote_compact_task(Arc::clone(&session), step, world)
                    .await
            } else {
                crate::compact::run_compact_task(
                    Arc::clone(&session),
                    step,
                    world,
                    vec![UserInput::Text {
                        text: "summarize without replacing input".into(),
                        text_elements: Vec::new(),
                    }],
                )
                .await
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(15), compact).await;
        if result.is_err() {
            let paths = server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .map(|request| format!("{} {}", request.method, request.url.path()))
                .collect::<Vec<_>>();
            let errors = std::iter::from_fn(|| events.try_recv().ok())
                .filter_map(|event| match event.msg {
                    EventMsg::Error(error) => Some(error.message),
                    _ => None,
                })
                .collect::<Vec<_>>();
            panic!(
                "compaction stalled: remote={remote}, reset={}, paths={paths:?}, errors={errors:?}",
                restored.load(Ordering::SeqCst)
            );
        }
        result.unwrap().unwrap();
        let bodies = requests.requests();
        assert_eq!(bodies.len(), if preflight { 1 } else { 2 });
        assert!(
            bodies[0].body_json()["input"]
                .to_string()
                .contains("keep this exact compaction input"),
            "the original history reaches compaction"
        );
        if !preflight {
            assert_eq!(
                bodies[0].body_json()["input"],
                bodies[1].body_json()["input"],
                "compaction input survives redemption"
            );
        }
        assert!(restored.load(Ordering::SeqCst));
        assert!(
            turn.extension_data
                .get::<codex_alab_quota_recovery::State>()
                .is_some()
        );
        session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    }
    println!("ALAB_WEEKLY_COMPACTION_OK");
}
