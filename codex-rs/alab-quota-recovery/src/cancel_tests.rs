// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test]
async fn cancelled_redemption_releases_custody_and_reconciles_the_same_operation() {
    interrupted_redemption(false, false).await;
}

#[tokio::test]
async fn timed_out_redemption_releases_custody_and_reconciles_the_same_operation() {
    interrupted_redemption(true, false).await;
}

#[tokio::test]
async fn natural_reset_releases_pending_redemption_custody_without_losing_its_key() {
    interrupted_redemption(false, true).await;
}

async fn interrupted_redemption(automatic_timeout: bool, natural_reset: bool) {
    codex_alab_core::activate();
    let server = MockServer::start().await;
    let restored = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&restored);
    let sent = Arc::new(AtomicBool::new(false));
    let observed_sent = Arc::clone(&sent);
    let reads = Arc::new(AtomicUsize::new(0));
    let observed_reads = Arc::clone(&reads);
    let cycle = chrono::Utc::now().timestamp() + 3600;
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(move |_: &wiremock::Request| {
            let count = observed_reads.fetch_add(1, Ordering::SeqCst);
            let healthy = observed.load(Ordering::SeqCst) && count >= 3;
            let reset = cycle
                + if observed_sent.load(Ordering::SeqCst) {
                    3600
                } else {
                    0
                };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "account_id":"account", "user_id":"user", "plan_type":"plus",
                "rate_limit":{"allowed":healthy,"limit_reached":!healthy,
                    "secondary_window":{"used_percent":if healthy {0}else{100},
                        "limit_window_seconds":604800,"reset_at":reset,"reset_after_seconds":3600}}
            }))
        })
        .expect(4)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/codex/rate-limit-reset-credits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "available_count":2,"credits":[{"id":"A","reset_type":"codex_rate_limits",
                "status":"available","granted_at":"2026-01-01T00:00:00Z",
                "expires_at":"2030-01-01T00:00:00Z"},
                {"id":"B","reset_type":"codex_rate_limits","status":"available",
                "granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-02-01T00:00:00Z"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let posted = Arc::new(tokio::sync::Notify::new());
    let notification = Arc::clone(&posted);
    let posts = Arc::new(AtomicUsize::new(0));
    let observed_posts = Arc::clone(&posts);
    Mock::given(method("POST"))
        .and(path("/api/codex/rate-limit-reset-credits/consume"))
        .respond_with(move |_: &wiremock::Request| {
            if observed_posts.fetch_add(1, Ordering::SeqCst) == 0 {
                // The reset was applied, but its ACK and usage propagation lag.
                sent.store(true, Ordering::SeqCst);
                notification.notify_one();
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
                    .set_delay(Duration::from_secs(60))
            } else {
                restored.store(true, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":"already_redeemed","windows_reset":2}))
            }
        })
        .expect(2)
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
    let owner = state.owner.as_ref().unwrap();
    {
        let recovering = wait_for_reset(
            &manager,
            &config,
            owner,
            "turn",
            Duration::from_millis(10),
            natural_reset.then(|| chrono::Utc::now().timestamp() + 3),
        );
        tokio::pin!(recovering);
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                _ = &mut recovering => panic!("redemption completed before cancellation"),
                _ = posted.notified() => {}
            }
        })
        .await
        .unwrap();
        if automatic_timeout {
            assert!(
                tokio::time::timeout(Duration::from_secs(55), &mut recovering)
                    .await
                    .expect("automatic timeout did not release redemption custody")
            );
        } else if natural_reset {
            assert!(
                !tokio::time::timeout(Duration::from_secs(5), &mut recovering)
                    .await
                    .expect("natural reset did not release redemption custody")
            );
        }
        // Otherwise drop the request while its POST and custody lock are outstanding.
    }
    let account = digest(&format!("{}\0account\0user", server.uri()));
    let dir = home.path().join(format!("weekly-reset-{account}.d"));
    let ledger = load::<Ledger>(&dir.join("state.json")).unwrap().unwrap();
    assert!(ledger.current.as_ref().is_some_and(|op| op.outcome
        == if automatic_timeout {
            Outcome::Consumed
        } else {
            Outcome::AwaitingAck
        }));
    assert_eq!(ledger.version, 2);
    assert_eq!(credit_at(&dir, "A").unwrap().consumed, automatic_timeout);
    if !automatic_timeout {
        assert!(
            tokio::time::timeout(
                Duration::from_secs(5),
                wait_for_reset(
                    &manager,
                    &config,
                    owner,
                    "next-turn",
                    Duration::from_millis(10),
                    None
                )
            )
            .await
            .expect("cancelled request retained the custody lock")
        );
    }
    let requests = server.received_requests().await.unwrap();
    let bodies: Vec<serde_json::Value> = requests
        .iter()
        .filter(|request| request.method.as_str() == "POST")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    assert_eq!(bodies.len(), 2);
    assert_eq!(
        bodies[0], bodies[1],
        "cancellation must preserve the exact redemption key and credit"
    );
    assert_eq!(bodies[0]["credit_id"], "A");
    assert_eq!(
        bodies[0]["redeem_request_id"],
        ledger.current.as_ref().unwrap().key
    );
    let credit = credit_at(&dir, "A").unwrap();
    assert!(credit.consumed && credit.retry_released);
}
