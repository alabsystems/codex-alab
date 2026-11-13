// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real manager changes, command executions and cancellation; no native image proof.
use super::*;
use crate::SharedModelProvider;
use crate::auth::ResolvedProviderAuth;
use codex_login::CodexAuth;
use codex_login::ExternalAuth;
use codex_login::ExternalAuthFuture;
use codex_login::ExternalAuthRefreshContext;
use std::path::Path;
use std::time::Duration;

struct External(CodexAuth);
impl ExternalAuth for External {
    fn resolve(&self) -> ExternalAuthFuture<'_, CodexAuth> {
        let auth = self.0.clone();
        Box::pin(async move { Ok(auth) })
    }
    fn refresh(&self, _: ExternalAuthRefreshContext) -> ExternalAuthFuture<'_, CodexAuth> {
        Box::pin(async { Err(std::io::Error::other("unused fixture refresh")) })
    }
}

fn info(args: &[&Path]) -> ModelProviderInfo {
    serde_json::from_value(serde_json::json!({
        "name": "gateway",
        "base_url": "http://127.0.0.1:1/v1",
        "requires_openai_auth": true,
        "gateway_oauth": {
            "delivery": { "kind": "cookie", "name": "gateway" },
            "credential_export": { "command": "/bin/sh", "args": args }
        }
    }))
    .unwrap()
}

async fn prepared(provider: &SharedModelProvider) -> ResolvedProviderAuth {
    provider.request_provider_and_auth(None).await.unwrap().1
}

async fn wait_path(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actual command milestone");
}

#[tokio::test]
async fn gateway_cache_tracks_sources_and_late_rejections_without_cross_owner_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("export.sh");
    let counter = dir.path().join("counter");
    let token = dir.path().join("token");
    std::fs::write(&script, "printf x >> \"$1\"\ncat \"$2\"\n").unwrap();
    std::fs::write(&counter, "").unwrap();
    std::fs::write(&token, "first").unwrap();
    let info = info(&[&script, &counter, &token]);
    let owner = AuthManager::from_auth_for_testing_with_home(
        CodexAuth::from_api_key("fixture-primary"),
        dir.path().to_owned(),
    );
    let first = crate::create_model_provider(info.clone(), Some(owner.clone()));
    let second = crate::create_model_provider(info.clone(), Some(owner.clone()));
    let gateway = first.gateway_auth_manager().unwrap().unwrap();
    let before = prepared(&first).await;
    let _same = prepared(&second).await;
    let count = || std::fs::read_to_string(&counter).unwrap().len();
    assert_eq!(
        count(),
        1,
        "equivalent provider instances share the upstream export"
    );
    std::fs::write(&token, "second").unwrap();
    owner
        .set_external_auth(Arc::new(External(
            CodexAuth::from_external_chatgpt_tokens(
                "header.e30.fixture",
                "fixture-workspace",
                Some("enterprise"),
            )
            .unwrap(),
        )))
        .await
        .unwrap();
    let after = prepared(&first).await;
    let _same_source = prepared(&second).await;
    assert_eq!(
        count(),
        2,
        "a source change cannot reuse the startup export"
    );
    assert!(
        !before.auth.alab_gateway_recovery().unwrap().await,
        "old-source rejection is retired"
    );
    assert!(
        gateway.resolve_access_token().await.is_err(),
        "captured startup export stays retired"
    );
    let (selected, _) = gateway.alab_resolve_for_primary(&after.auth).await.unwrap();
    assert_eq!(selected.as_ref(), "second");
    assert_eq!(
        count(),
        2,
        "late old-source recovery did not clear the current export"
    );

    assert!(after.auth.alab_gateway_recovery().unwrap().await);
    std::fs::write(&token, "third").unwrap();
    let renewed = prepared(&second).await;
    assert_eq!(count(), 3);
    assert!(
        after.auth.alab_gateway_recovery().unwrap().await,
        "a repeated rejection can reuse an already refreshed generation"
    );
    let (selected, _) = gateway
        .alab_resolve_for_primary(&renewed.auth)
        .await
        .unwrap();
    assert_eq!(selected.as_ref(), "third");
    assert_eq!(
        count(),
        3,
        "late rejected token must not clear a newer cached generation"
    );

    assert!(renewed.auth.alab_gateway_recovery().unwrap().await);
    let _identical_bytes = prepared(&first).await;
    assert_eq!(count(), 4);
    assert!(renewed.auth.alab_gateway_recovery().unwrap().await);
    let _after_late_identical_rejection = prepared(&second).await;
    assert_eq!(
        count(),
        4,
        "identical token bytes must not conflate export generations"
    );

    let other_home = tempfile::tempdir().unwrap();
    let other_owner = AuthManager::from_auth_for_testing_with_home(
        CodexAuth::from_api_key("fixture-other"),
        other_home.path().to_owned(),
    );
    let other = crate::create_model_provider(info, Some(other_owner));
    let foreign = prepared(&other).await;
    assert_eq!(
        count(),
        5,
        "independent owners never share command credentials"
    );
    assert!(
        gateway
            .alab_resolve_for_primary(&foreign.auth)
            .await
            .is_err()
    );
    assert_eq!(
        count(),
        5,
        "mixed primary/gateway owners refuse before command execution"
    );
}

#[tokio::test]
async fn gateway_catalog_and_inference_share_only_the_same_selected_export() {
    use codex_api::Compression;
    use codex_api::ResponsesClient;
    use codex_http_client::ClientRouteClass;
    use codex_http_client::HttpClientFactory;
    use codex_http_client::OutboundProxyPolicy;
    use codex_http_client::ReqwestTransport;
    use codex_login::default_client::ClientRedirectPolicy;
    use codex_login::default_client::create_client_for_route;
    use codex_models_manager::manager::RefreshStrategy;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("export.sh");
    let counter = dir.path().join("counter");
    let token = dir.path().join("token");
    std::fs::write(&script, "printf x >> \"$1\"\ncat \"$2\"\n").unwrap();
    std::fs::write(&counter, "").unwrap();
    std::fs::write(&token, "fixture-gateway").unwrap();
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"models": []})))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"fixture\"}}\n\n",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut info = info(&[&script, &counter, &token]);
    info.base_url = Some(format!("{}/v1", server.uri()));
    info.model_catalog_url = Some(format!("{}/v1/models", server.uri()).into());
    let owner = AuthManager::from_auth_for_testing_with_home(
        CodexAuth::from_api_key("fixture-primary"),
        dir.path().to_owned(),
    );
    let catalog_provider = crate::create_model_provider(info.clone(), Some(owner.clone()));
    let inference_provider = crate::create_model_provider(info, Some(owner));
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    for provider in [&catalog_provider, &inference_provider] {
        let models = provider.models_manager_without_cache(None);
        models.set_api_key_model_discovery_enabled(true);
        models
            .raw_model_catalog(RefreshStrategy::Online, factory.clone())
            .await;
    }
    let (api, selected) = inference_provider
        .request_provider_and_auth(None)
        .await
        .unwrap();
    let bound = selected.bind_http_client_factory(factory).unwrap();
    let transport = ReqwestTransport::from_http_client(
        create_client_for_route(
            &bound,
            &api.url_for_path("responses"),
            ClientRouteClass::Api,
            ClientRedirectPolicy::Reject,
        )
        .unwrap(),
    );
    let client = ResponsesClient::new(transport, api, selected.auth);
    let response = client
        .stream(
            serde_json::json!({"model":"fixture", "input":[]}),
            http::HeaderMap::new(),
            Compression::None,
            None,
        )
        .await
        .unwrap();
    drop(response);
    assert_eq!(std::fs::read_to_string(counter).unwrap(), "x");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    for request in requests {
        assert_eq!(request.headers["authorization"], "Bearer fixture-primary");
        assert_eq!(request.headers["cookie"], "gateway=fixture-gateway");
    }
    server.verify().await;
}

#[tokio::test]
async fn cancelled_gateway_selection_keeps_owner_until_the_actual_command_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("blocked-export.sh");
    let started = dir.path().join("started");
    let release = dir.path().join("release");
    let finished = dir.path().join("finished");
    std::fs::write(&script, "printf started > \"$1\"\nwhile [ ! -e \"$2\" ]; do /bin/sleep 0.02; done\nprintf done > \"$3\"\nprintf fixture-token\n").unwrap();
    let owner = AuthManager::from_auth_for_testing_with_home(
        CodexAuth::from_api_key("fixture-primary"),
        dir.path().to_owned(),
    );
    let weak = Arc::downgrade(&owner);
    let provider = crate::create_model_provider(
        info(&[&script, &started, &release, &finished]),
        Some(owner.clone()),
    );
    let task = tokio::spawn(async move { provider.request_provider_and_auth(None).await });
    wait_path(&started).await;
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    drop(owner);
    assert!(
        weak.upgrade().is_some(),
        "cancelled caller cannot end actual command ownership"
    );
    assert!(!finished.exists());
    std::fs::write(&release, "release").unwrap();
    wait_path(&finished).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while weak.upgrade().is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("command completion must release the actual owner");
}
