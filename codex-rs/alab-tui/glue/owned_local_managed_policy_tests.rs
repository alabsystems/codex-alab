// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::OwnedLocalTarget;
use crate::cloud_config_bundle_for_app_server_target;
use crate::owned_local_ui_snapshot::UiSnapshot;
use codex_app_server_client::EmbeddedNetworkPolicy;
use codex_app_server_client::retained_readers::ManagedCloudData;
use codex_config::CloudConfigBundleLoadError;
use codex_config::CloudConfigBundleLoadErrorCode;
use codex_config::CloudConfigBundleLoader;
use codex_config::ConfigServer;
use codex_config::LoaderOverrides;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::io;

struct ConfigLoader {
    home: tempfile::TempDir,
    bootstrap: crate::ConfigTomlLoadResult,
    network: EmbeddedNetworkPolicy,
}

impl ConfigLoader {
    async fn new() -> io::Result<Self> {
        let home = tempfile::tempdir()?;
        let overrides = LoaderOverrides {
            ignore_user_config: true,
            ..Default::default()
        };
        let bootstrap = crate::legacy_core::config::load_config_toml_with_layer_stack(
            home.path(),
            None,
            Vec::new(),
            overrides.clone(),
        )
        .await?;
        let network = EmbeddedNetworkPolicy::load(&overrides).await;
        Ok(Self {
            home,
            bootstrap,
            network,
        })
    }

    async fn load(&self, target: &OwnedLocalTarget) -> io::Result<CloudConfigBundleLoader> {
        cloud_config_bundle_for_app_server_target(
            &crate::AppServerTarget::OwnedLocal {
                target: target.clone(),
            },
            &self.bootstrap,
            self.home.path(),
            &self.network,
        )
        .await
    }

    async fn target(&self, server: &str) -> io::Result<OwnedLocalTarget> {
        let snapshot = UiSnapshot::from_backend(
            &policy(server).get_snapshot().await,
            AbsolutePathBuf::from_absolute_path(self.home.path())?,
            None,
        )?;
        // This test exercises local DATA routing. Neither target acquires a Native controller.
        let target = OwnedLocalTarget::new("a".repeat(32), None, None);
        target.install_snapshot(snapshot)?;
        Ok(target)
    }
}

fn policy(server: &str) -> CloudConfigBundleLoader {
    CloudConfigBundleLoader::new(async { Ok(None) }).with_config_server(ConfigServer {
        base_url: server.into(),
        oauth: None,
    })
}

#[tokio::test]
async fn retained_configuration_uses_only_its_targets_latest_managed_observation() -> io::Result<()>
{
    let config = ConfigLoader::new().await?;
    let current = config.target("https://initial.example/").await?;
    let other = config.target("https://other.example/").await?;
    let observed = policy("https://after-login.example/").get_snapshot().await;
    current.observe_managed_cloud(ManagedCloudData::from_actual(&observed))?;
    let selected = config.load(&current).await?.get_snapshot().await;
    assert_eq!(selected.config_server, observed.config_server);
    let independent = config.load(&other).await?.get_snapshot().await;
    assert_eq!(
        independent.config_server,
        policy("https://other.example/").config_server().cloned()
    );
    let withdrawn = CloudConfigBundleLoader::new(async { Ok(None) })
        .get_snapshot()
        .await;
    current.observe_managed_cloud(ManagedCloudData::from_actual(&withdrawn))?;
    assert!(config.load(&current).await?.config_server().is_none());
    Ok(())
}

#[tokio::test]
async fn retained_configuration_preserves_the_current_backend_policy_error_until_it_recovers()
-> io::Result<()> {
    let config = ConfigLoader::new().await?;
    let current = config.target("https://initial.example/").await?;
    let failed = CloudConfigBundleLoader::new(async {
        Err(CloudConfigBundleLoadError::new(
            CloudConfigBundleLoadErrorCode::Auth,
            Some(401),
            "account no longer authorized",
        ))
    })
    .get_snapshot()
    .await;
    current.observe_managed_cloud(ManagedCloudData::from_actual(&failed))?;
    let failure = config
        .load(&current)
        .await?
        .get()
        .await
        .expect_err("old managed policy masked the backend rejection");
    assert_eq!(failure.code(), CloudConfigBundleLoadErrorCode::Auth);
    assert_eq!(failure.status_code(), Some(401));
    assert_eq!(failure.to_string(), "account no longer authorized");
    assert!(current.snapshot_for_transfer().is_err());
    let recovered = policy("https://after-reauth.example/").get_snapshot().await;
    current.observe_managed_cloud(ManagedCloudData::from_actual(&recovered))?;
    assert_eq!(
        config
            .load(&current)
            .await?
            .get_snapshot()
            .await
            .config_server,
        recovered.config_server
    );
    Ok(())
}

#[tokio::test]
async fn entered_policy_refuses_another_target_with_the_same_service_label() -> io::Result<()> {
    let config = ConfigLoader::new().await?;
    let entered = config.target("https://owner.example/").await?;
    let foreign = config.target("https://foreign.example/").await?;
    assert_eq!(entered.service_data(), foreign.service_data());
    let validate = |target| {
        crate::owned_local_entered_readers::validate_target(
            &crate::AppServerTarget::OwnedLocal { target },
            &entered,
        )
    };
    validate(entered.clone())?;
    assert!(validate(foreign).is_err());
    Ok(())
}

#[tokio::test]
async fn post_login_refuses_an_unbound_session_without_changing_the_retained_policy()
-> io::Result<()> {
    use crate::app_server_session::AppServerSession;
    use crate::app_server_session::ThreadParamsMode;
    use codex_app_server_client::AppServerClient;
    use codex_app_server_client::CloudClient;
    use tokio::sync::mpsc::error::TryRecvError;
    let config = ConfigLoader::new().await?;
    let current = config.target("https://prior.example/").await?;
    let observed = policy("https://owner.example/").get_snapshot().await;
    current.observe_managed_cloud(ManagedCloudData::from_actual(&observed))?;
    let (foreign, mut commands, _events) = CloudClient::channel(4);
    let session = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        AppServerClient::Cloud(foreign),
        ThreadParamsMode::Cloud,
    );
    let target = crate::AppServerTarget::OwnedLocal {
        target: current.clone(),
    };
    let result = crate::owned_local_managed_readers::after_login_loader(
        &target,
        Some(&session),
        crate::bootstrap_auth_config(config.home.path(), &config.bootstrap)?,
        config.bootstrap.config_layer_stack.requirements_toml(),
        false,
    )
    .await;
    assert!(result.is_err());
    assert!(matches!(commands.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(
        config
            .load(&current)
            .await?
            .get_snapshot()
            .await
            .config_server,
        observed.config_server
    );
    Ok(())
}
