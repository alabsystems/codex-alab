// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_partial_json;
use wiremock::matchers::method;
use wiremock::matchers::path;

async fn reconcile_old_credit(had_usable_allowance: bool, committed_ack: bool) {
    codex_alab_core::activate();
    let server = MockServer::start().await;
    let restored = Arc::new(AtomicBool::new(had_usable_allowance));
    let observed = Arc::clone(&restored);
    let cycle = chrono::Utc::now().timestamp() + 24 * 3600;
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(move |_: &wiremock::Request| {
            let healthy = observed.load(Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "account_id":"account","user_id":"user","plan_type":"plus",
                "rate_limit":{"allowed":healthy,"limit_reached":!healthy,
                    "secondary_window":{"used_percent":if healthy {0}else{100},
                        "limit_window_seconds":604800,"reset_at":cycle,"reset_after_seconds":86400}}
            }))
        })
        .expect(if had_usable_allowance && !committed_ack {
            4
        } else {
            3
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/codex/rate-limit-reset-credits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "available_count":1,"credits":[{"id":"fresh","reset_type":"codex_rate_limits",
                "status":"available","granted_at":"2026-01-01T00:00:00Z",
                "expires_at":"2030-01-01T00:00:00Z"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/codex/rate-limit-reset-credits/consume"))
        .and(body_partial_json(serde_json::json!({"credit_id":"old"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"already_redeemed","windows_reset":2})),
        )
        .expect(if committed_ack { 0 } else { 1 })
        .mount(&server)
        .await;
    let applied = Arc::clone(&restored);
    Mock::given(method("POST"))
        .and(path("/api/codex/rate-limit-reset-credits/consume"))
        .and(body_partial_json(serde_json::json!({"credit_id":"fresh"})))
        .respond_with(move |_: &wiremock::Request| {
            applied.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().unwrap();
    let auth = codex_login::CodexAuth::from_external_chatgpt_tokens("e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture", "account", Some("plus")).unwrap();
    let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
    let state = State::bind(&manager).await;
    let config = Settings {
        home: home.path().to_owned(),
        base: server.uri(),
        factory: HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
    };
    let account = digest(&format!("{}\0account\0user", server.uri()));
    let dir = home.path().join(format!("weekly-reset-{account}.d"));
    private_dir(&dir).unwrap();
    let mut ledger = Ledger::default();
    let old_cycle = if had_usable_allowance {
        cycle
    } else {
        cycle - 6 * 3600
    };
    let old = begin(&dir, &account, &mut ledger, old_cycle, "old").unwrap();
    if !had_usable_allowance {
        ledger.version = 1;
        ledger.current.as_mut().unwrap().outcome = Outcome::Pending;
    }
    save(&dir.join("state.json"), &ledger).unwrap();
    if committed_ack {
        let mut credit = credit_at(&dir, "old").unwrap();
        credit.consumed = true;
        save(&credit_path(&dir, "old"), &credit).unwrap();
    }
    if had_usable_allowance {
        assert_eq!(
            recover(
                &manager,
                &config,
                state.owner.as_ref().unwrap(),
                "earlier-turn",
                if committed_ack {
                    Purpose::Preflight
                } else {
                    Purpose::Recover
                }
            )
            .await,
            if committed_ack {
                Recovery::Stop
            } else {
                Recovery::Restored
            }
        );
        let credit = credit_at(&dir, "old").unwrap();
        if committed_ack {
            assert!(
                credit.consumed && credit.retry_released,
                "preflight must settle an acknowledged receipt before the next exhaustion"
            );
        } else {
            assert!(!credit.consumed);
        }
        let restored_ledger = load::<Ledger>(&dir.join("state.json")).unwrap().unwrap();
        assert_eq!(restored_ledger.version, 2);
        assert!(
            restored_ledger
                .current
                .as_ref()
                .is_some_and(|op| op.outcome == Outcome::RestoredPending)
        );
        restored.store(false, Ordering::SeqCst);
    }
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(5),
            recover(
                &manager,
                &config,
                state.owner.as_ref().unwrap(),
                "turn",
                Purpose::Recover
            )
        )
        .await
        .unwrap(),
        Recovery::Restored
    );
    let requests = server.received_requests().await.unwrap();
    let bodies: Vec<serde_json::Value> = requests
        .iter()
        .filter(|request| request.method.as_str() == "POST")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    assert_eq!(bodies.len(), if committed_ack { 1 } else { 2 });
    if !committed_ack {
        assert_eq!(bodies[0]["redeem_request_id"], old.key);
        assert_eq!(bodies[0]["credit_id"], "old");
    }
    assert_eq!(bodies.last().unwrap()["credit_id"], "fresh");
    let old_credit = credit_at(&dir, "old").unwrap();
    assert!(old_credit.consumed && old_credit.retry_released);
    assert_eq!(
        load::<Ledger>(&dir.join("state.json"))
            .unwrap()
            .unwrap()
            .version,
        2
    );
    assert!(credit_at(&dir, "fresh").unwrap().retry_released);
}

#[tokio::test]
async fn legacy_old_cycle_reconciliation_can_redeem_a_fresh_credit() {
    reconcile_old_credit(false, false).await;
}

#[tokio::test]
async fn external_restoration_then_redepletion_can_use_a_fresh_credit() {
    reconcile_old_credit(true, false).await;
}

#[tokio::test]
async fn committed_credit_survives_a_crash_before_ledger_acknowledgement() {
    reconcile_old_credit(true, true).await;
}
