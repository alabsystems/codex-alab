// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual ConfigBuilder and Unix DATA controls. No credential manager, Native
//! grant, model response or invented app-server configuration RPC is supplied.
use super::*;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use codex_config::CloudConfigFragment;
use codex_config::CloudConfigTomlBundle;
use codex_config::CloudRequirementsFragment;
use codex_config::CloudRequirementsTomlBundle;
use codex_config::LoaderOverrides;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::ensure;
use std::future::Future;
use std::time::Duration;
use tempfile::TempDir;

async fn bounded<F: Future>(future: F) -> color_eyre::Result<F::Output> {
    tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .wrap_err("UI snapshot control timed out")
}

fn bundle() -> CloudConfigBundle {
    CloudConfigBundle {
        config_toml: CloudConfigTomlBundle { enterprise_managed: vec![CloudConfigFragment {
            id: "actual-config-fixture".into(), name: "Cloud configuration fixture".into(),
            contents: "model = 'cloud-model'\n".into(),
        }] },
        requirements_toml: CloudRequirementsTomlBundle { enterprise_managed: vec![CloudRequirementsFragment {
            id: "actual-requirements-fixture".into(), name: "Cloud requirements fixture".into(),
            contents: "allowed_approval_policies = ['on-request']\nallowed_sandbox_modes = ['read-only']\n".into(),
        }] },
    }
}

async fn transfer(snapshot: UiSnapshot) -> color_eyre::Result<UiSnapshot> {
    let (mut backend, mut frontend) = UnixStream::pair()?;
    let writer = tokio::spawn(async move { write(&mut backend, &snapshot).await });
    let received = bounded(read(&mut frontend)).await;
    let sent = bounded(writer).await;
    let received = received??;
    sent???;
    Ok(received)
}

#[tokio::test]
async fn ui_data_and_actual_backend_builder_preserve_cloud_local_policy_and_cli()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let cwd = home.path().join("selected-workspace");
    std::fs::create_dir(&cwd)?;
    std::fs::write(
        home.path().join("config.toml"),
        "approval_policy = 'on-request'\nsandbox_mode = 'read-only'\nforced_login_method = 'chatgpt'\nforced_chatgpt_workspace_id = ['123e4567-e89b-42d3-a456-426614174000']\n",
    )?;
    let delivered = bundle();
    let backend_loader = CloudConfigBundleLoader::new(async move { Ok(Some(delivered)) });
    let backend_snapshot = backend_loader.get_snapshot().await;
    let snapshot = UiSnapshot::from_backend(
        &backend_snapshot,
        AbsolutePathBuf::from_absolute_path(cwd.clone())?,
        None,
    )?;
    let ui = transfer(snapshot).await?;
    let original_builder = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .cli_overrides(vec![(
            "model".into(),
            toml::Value::String("original-cli-model".into()),
        )])
        .harness_overrides(ConfigOverrides {
            cwd: Some(cwd.clone()),
            ..Default::default()
        });
    let backend = original_builder
        .clone()
        .cloud_config_bundle(CloudConfigBundleLoader::from_snapshot(backend_snapshot))
        .build()
        .await?;
    let frontend = original_builder
        .cloud_config_bundle(ui.ui_cloud_loader())
        .build()
        .await?;
    ensure!(backend.cwd == frontend.cwd && frontend.cwd.as_path() == cwd);
    ensure!(
        backend.model == frontend.model && backend.model_provider_id == frontend.model_provider_id
    );
    ensure!(
        backend.permissions.approval_policy.value() == frontend.permissions.approval_policy.value()
    );
    ensure!(
        backend.permissions.effective_permission_profile()
            == frontend.permissions.effective_permission_profile()
    );
    ensure!(backend.forced_login_method == frontend.forced_login_method);
    ensure!(backend.forced_chatgpt_workspace_id == frontend.forced_chatgpt_workspace_id);
    ensure!(
        backend.config_layer_stack.requirements_toml()
            == frontend.config_layer_stack.requirements_toml()
    );
    ensure!(
        frontend.forced_login_method.is_some() && frontend.forced_chatgpt_workspace_id.is_some(),
        "local auth restrictions were stripped as though this were a remote workspace"
    );
    let backend_params = crate::app_server_session::thread_start_params_from_config(
        &backend,
        crate::app_server_session::ThreadParamsMode::Embedded,
        None,
        None,
    );
    let local_params = crate::app_server_session::thread_start_params_from_config(
        &frontend,
        crate::app_server_session::ThreadParamsMode::Embedded,
        None,
        None,
    );
    let remote_params = crate::app_server_session::thread_start_params_from_config(
        &frontend,
        crate::app_server_session::ThreadParamsMode::Remote,
        None,
        None,
    );
    ensure!(
        serde_json::to_value(&local_params)? == serde_json::to_value(&backend_params)?,
        "UI DATA changed the original local request settings"
    );
    ensure!(local_params.cwd.as_deref() == cwd.to_str());
    ensure!(local_params.runtime_workspace_roots.as_ref() == Some(&frontend.workspace_roots));
    ensure!(
        remote_params.runtime_workspace_roots.is_none(),
        "control did not distinguish remote workspace behavior"
    );
    Ok(())
}

#[tokio::test]
async fn ui_snapshot_preserves_non_utf8_selected_cwd_and_private_domain_values()
-> color_eyre::Result<()> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;
    let loader = CloudConfigBundleLoader::new(async { Ok(Some(bundle())) });
    let backend = loader.get_snapshot().await;
    let cwd = std::path::PathBuf::from(OsString::from_vec(b"/selected/cwd-\xff".to_vec()));
    let snapshot = UiSnapshot::from_backend(
        &backend,
        AbsolutePathBuf::from_absolute_path(cwd.clone())?,
        Some(OssChoice {
            provider: "ollama".into(),
            manually_selected: true,
        }),
    )?;
    ensure!(!format!("{snapshot:?}").contains("cloud-model"));
    let ui = transfer(snapshot).await?;
    ensure!(ui.selected_cwd.as_path().as_os_str().as_bytes() == cwd.as_os_str().as_bytes());
    let observed_bundle = ui.ui_cloud_loader().get().await?;
    let expected_bundle = backend.bundle?;
    ensure!(observed_bundle == expected_bundle);
    ensure!(
        ui.oss
            .as_ref()
            .is_some_and(|choice| choice.provider == "ollama" && choice.manually_selected)
    );
    Ok(())
}

#[tokio::test]
async fn ui_snapshot_requires_physical_eof_without_trailing_data() -> color_eyre::Result<()> {
    let loader = CloudConfigBundleLoader::new(async { Ok(None) });
    let backend = loader.get_snapshot().await;
    let snapshot = UiSnapshot::from_backend(
        &backend,
        AbsolutePathBuf::from_absolute_path("/selected")?,
        None,
    )?;
    let (mut writer, mut reader) = UnixStream::pair()?;
    let task = tokio::spawn(async move {
        let bytes = serde_json::to_vec(&snapshot).map_err(|_| unavailable())?;
        writer.write_all(MAGIC).await?;
        writer
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await?;
        writer.write_all(&bytes).await?;
        writer.write_all(b"trailing").await?;
        writer.shutdown().await
    });
    let observed = bounded(read(&mut reader)).await;
    let joined = bounded(task).await;
    ensure!(
        observed?.is_err(),
        "trailing data became an accepted UI snapshot"
    );
    joined???;
    Ok(())
}
