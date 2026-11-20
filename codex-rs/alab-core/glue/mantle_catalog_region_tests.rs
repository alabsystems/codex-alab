// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::AmazonBedrockModelProvider;
use codex_model_provider_info::ModelProviderInfo;
use std::process::Command;

#[test]
fn mantle_catalog_tracks_the_request_partition_without_instance_metadata() {
    const CHILD: &str = "ALAB_TEST_MANTLE_REGIONAL_CATALOG";
    let Ok(case) = std::env::var(CHILD) else {
        let files = tempfile::tempdir().unwrap();
        std::fs::write(
            files.path().join("config"),
            "[profile lab]\nsource_profile = gov\n[profile gov]\nregion = us-gov-west-1\n",
        )
        .unwrap();
        let name = concat!(
            module_path!(),
            "::mantle_catalog_tracks_the_request_partition_without_instance_metadata"
        );
        let name = name.strip_prefix("codex_model_provider::").unwrap();
        for case in [
            "configured-gov",
            "environment-gov",
            "profile-gov",
            "commercial",
            "explicit-commercial",
            "unknown",
        ] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", name, "--nocapture"])
                .env(CHILD, case)
                .env_remove("AWS_PROFILE")
                .env_remove("AWS_DEFAULT_PROFILE")
                .env_remove("AWS_BEARER_TOKEN_BEDROCK")
                .env_remove("AWS_ACCESS_KEY_ID")
                .env_remove("AWS_SECRET_ACCESS_KEY")
                .env_remove("AWS_REGION")
                .env_remove("AWS_DEFAULT_REGION")
                .env("AWS_EC2_METADATA_DISABLED", "true")
                .env("AWS_CONFIG_FILE", files.path().join("config"))
                .env(
                    "AWS_SHARED_CREDENTIALS_FILE",
                    files.path().join("missing-credentials"),
                );
            match case {
                "configured-gov" | "commercial" => {
                    command.env("AWS_REGION", "us-west-2");
                }
                "environment-gov" | "explicit-commercial" => {
                    command.env("AWS_DEFAULT_REGION", "us-gov-east-1");
                }
                _ => {}
            }
            let output = command.output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{case}: {stdout} {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    codex_alab_core::activate();
    let mut info = ModelProviderInfo::create_amazon_bedrock_provider(None);
    let aws = info.aws.as_mut().unwrap();
    match case.as_str() {
        "configured-gov" => aws.region = Some("us-gov-west-1".into()),
        "profile-gov" => aws.profile = Some("lab".into()),
        "explicit-commercial" => {
            info.base_url = Some("https://bedrock-mantle.us-west-2.api.aws/openai/v1".into())
        }
        _ => {}
    }
    let provider = AmazonBedrockModelProvider::new(info, None);
    let catalog = {
        use crate::ModelProvider as _;
        let manager = provider.models_manager_without_cache(None);
        codex_protocol::openai_models::ModelsResponse {
            models: manager.try_get_remote_models().unwrap(),
        }
    };
    let slugs = catalog
        .models
        .iter()
        .map(|m| m.slug.as_str())
        .collect::<Vec<_>>();
    if matches!(
        case.as_str(),
        "commercial" | "explicit-commercial" | "unknown"
    ) {
        assert!(slugs.contains(&"openai.gpt-6.2-sol") && slugs.contains(&"openai.gpt-6.2-astra"));
    } else {
        assert_eq!(slugs, ["openai.gpt-5.6-terra", "openai.gpt-5.6-luna"]);
    }
}

#[tokio::test]
async fn metadata_only_mantle_browses_offline_and_retries_actual_region_for_admission() {
    use crate::ModelProvider as _;
    use codex_http_client::HttpClientFactory;
    use codex_http_client::OutboundProxyPolicy;
    use codex_models_manager::ModelOrigin;
    use codex_models_manager::manager::RefreshStrategy;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    const CHILD: &str = "ALAB_TEST_MANTLE_METADATA_REGION";
    const READY: &str = "ALAB_TEST_MANTLE_METADATA_READY";
    if let Ok(region) = std::env::var(CHILD) {
        codex_alab_core::activate();
        let dir = std::path::PathBuf::from(std::env::var(READY).unwrap());
        let owner = codex_login::AuthManager::from_auth_for_testing_with_home(
            codex_login::CodexAuth::from_api_key("fixture-initial-owner"),
            dir.clone(),
        );
        let provider = AmazonBedrockModelProvider::new(
            ModelProviderInfo::create_amazon_bedrock_provider(None),
            Some(std::sync::Arc::clone(&owner)),
        );
        let manager = provider.models_manager_without_cache(None);
        let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
        let inspect = async || {
            manager
                .model_catalog_with_origins(RefreshStrategy::Online, factory.clone())
                .await
        };
        let before = inspect().await;
        let sol = "openai.gpt-6.2-sol";
        let terra = "openai.gpt-5.6-terra";
        assert_eq!(
            before.iter().find(|m| m.slug == sol).unwrap().origin,
            ModelOrigin::Bundled
        );
        assert_eq!(
            before.iter().find(|m| m.slug == terra).unwrap().origin,
            ModelOrigin::Provider
        );
        assert_eq!(manager.alab_confirmed_mantle_region(), None);
        std::fs::write(dir.join("browsed"), b"").unwrap();
        manager.alab_prepare_mantle_thread().await;
        assert_eq!(manager.alab_confirmed_mantle_region(), None);
        std::fs::write(dir.join("ready"), b"").unwrap();
        if region == "replace-managed" {
            let pending = tokio::spawn({
                let manager = std::sync::Arc::clone(&manager);
                async move { manager.alab_prepare_mantle_thread().await }
            });
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while !dir.join("observing").is_file() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            std::fs::write(dir.join("auth.json"), br#"{"auth_mode":"bedrockApiKey","bedrock_api_key":{"api_key":"fixture-managed-key","region":"us-gov-west-1"}}"#).unwrap();
            assert!(owner.reload().await);
            assert!(matches!(
                owner.auth_cached(),
                Some(codex_login::CodexAuth::BedrockApiKey(_))
            ));
            std::fs::write(dir.join("changed"), b"").unwrap();
            pending.await.unwrap();
            assert_eq!(manager.alab_confirmed_mantle_region(), None);
            let after = inspect().await;
            assert!(after.iter().any(|r| r.slug == terra) && !after.iter().any(|r| r.slug == sol));
            assert_eq!(
                provider.runtime_base_url().await.unwrap().as_deref(),
                Some("https://bedrock-mantle.us-gov-west-1.api.aws/openai/v1")
            );
            return;
        }
        // The first real SDK region read is held by the actual metadata
        // endpoint. Start a second ORIGINAL call before that read can commit.
        let first = tokio::spawn({
            let manager = std::sync::Arc::clone(&manager);
            async move { manager.alab_prepare_mantle_thread().await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !dir.join("observing").is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut second = manager.alab_prepare_mantle_thread();
        let second_pending = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(second.as_mut(), cx).is_pending())
        })
        .await;
        // Always release the held endpoint before asserting the pending oracle.
        // Resume this SAME second future; never submit a replacement operation.
        std::fs::write(dir.join("contended"), b"").unwrap();
        first.await.unwrap();
        if second_pending {
            second.await;
        }
        assert!(
            second_pending,
            "second actual region admission bypassed its held peer"
        );
        assert_eq!(manager.alab_confirmed_mantle_region(), Some(region.clone()));
        let after = inspect().await;
        assert!(after.iter().all(|m| m.origin == ModelOrigin::Provider));
        assert_eq!(after.iter().any(|m| m.slug == sol), region == "us-west-2");
        assert!(after.iter().any(|m| m.slug == terra));
        manager.alab_prepare_mantle_thread().await;
        return;
    }
    let name = concat!(
        module_path!(),
        "::metadata_only_mantle_browses_offline_and_retries_actual_region_for_admission"
    );
    for region in ["us-gov-west-1", "us-west-2", "replace-managed"] {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().to_owned();
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/latest/api/token"))
            .respond_with(move |_: &wiremock::Request| {
                assert!(
                    state.join("browsed").is_file(),
                    "browsing must not contact metadata"
                );
                if state.join("ready").is_file() {
                    ResponseTemplate::new(200)
                        .insert_header("x-aws-ec2-metadata-token-ttl-seconds", "21600")
                        .set_body_string("local-metadata-token")
                } else {
                    ResponseTemplate::new(403)
                }
            })
            .mount(&server)
            .await;
        let reading = dir.path().to_owned();
        Mock::given(method("GET"))
            .and(path("/latest/meta-data/placement/region"))
            .and(wiremock::matchers::header(
                "x-aws-ec2-metadata-token",
                "local-metadata-token",
            ))
            .respond_with(move |_: &wiremock::Request| {
                if region == "replace-managed" {
                    std::fs::write(reading.join("observing"), b"").unwrap();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while !reading.join("changed").is_file() {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "auth owner did not change before release"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    ResponseTemplate::new(200).set_body_string("us-west-2")
                } else {
                    std::fs::write(reading.join("observing"), b"").unwrap();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while !reading.join("contended").is_file() {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "second actual preparation was not polled before release"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    ResponseTemplate::new(200).set_body_string(region)
                }
            })
            .expect(1)
            .mount(&server)
            .await;
        let output = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                name.strip_prefix("codex_model_provider::").unwrap(),
                "--nocapture",
            ])
            .env(CHILD, region)
            .env(READY, dir.path())
            .env_remove("AWS_REGION")
            .env_remove("AWS_DEFAULT_REGION")
            .env_remove("AWS_PROFILE")
            .env_remove("AWS_BEARER_TOKEN_BEDROCK")
            .env("AWS_ACCESS_KEY_ID", "fixture-key")
            .env("AWS_SECRET_ACCESS_KEY", "fixture-secret")
            .env("AWS_CONFIG_FILE", dir.path().join("config"))
            .env(
                "AWS_SHARED_CREDENTIALS_FILE",
                dir.path().join("credentials"),
            )
            .env("AWS_EC2_METADATA_DISABLED", "false")
            .env("AWS_EC2_METADATA_V1_DISABLED", "true")
            .env("NO_PROXY", "localhost,127.0.0.1")
            .env("AWS_EC2_METADATA_SERVICE_ENDPOINT", server.uri())
            .output()
            .await
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{region}: {stdout} {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
