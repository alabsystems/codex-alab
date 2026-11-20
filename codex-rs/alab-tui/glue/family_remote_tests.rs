// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::startup::startup_model;
use crate::app_server_session::StartupLaunchChoices;
use crate::app_server_session::start_thread_with_request_handle;
use pretty_assertions::assert_eq;

use core_test_support::alab_test_process as process;

#[tokio::test]
async fn family_unmarked_remote_supplies_its_own_model_at_thread_start() -> Result<()> {
    if !process::child(concat!(
        module_path!(),
        "::family_unmarked_remote_supplies_its_own_model_at_thread_start"
    )) {
        return Ok(());
    }
    let home = tempdir()?;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    config.model = None;
    let (mut server, requests, proxy) = start_recording_remote_app_server(&config).await?;
    let mut bootstrap = server.bootstrap(&config).await?;
    // Simulate a legacy model/list response with only an unmarked display row.
    let mut row = bootstrap.available_models.remove(0);
    row.model = "legacy-provisional-display-row".into();
    row.is_default = false;
    bootstrap.available_models = vec![row];
    codex_alab_core::activate();
    assert_eq!(
        config.alab_provisional_startup_model(&bootstrap.available_models, true, true),
        Ok(true)
    );
    assert_eq!(
        startup_model(&config, &bootstrap, true)?,
        "legacy-provisional-display-row"
    );
    let started = start_thread_with_request_handle(
        server.request_handle(),
        &crate::local_settings::LocalSettings::from(&config),
        config,
        server.thread_params_mode(),
        None,
        server.thread_tool_transport(),
        StartupLaunchChoices::default(),
    )
    .await?;
    let starts = recorded_params(&requests, "thread/start");
    assert_eq!(starts.len(), 1);
    assert!(starts[0]["model"].is_null());
    assert!(!started.session.model.is_empty());
    assert_ne!(started.session.model, "legacy-provisional-display-row");
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}
