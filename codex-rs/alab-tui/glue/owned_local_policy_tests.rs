// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real original config consumers with isolated on-disk layers. No Native SDK,
//! frontend activation, synthetic config response or credential reader grant.
use super::*;
use color_eyre::eyre::ensure;
use tempfile::TempDir;

#[tokio::test]
async fn original_fallible_config_consumers_preserve_real_cli_layer_and_loader()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        "model = 'stored-original-model'\n",
    )?;
    let cwd = AbsolutePathBuf::from_absolute_path(home.path())?;
    let loader = LoaderOverrides::without_managed_config_for_tests();
    let overrides = vec![(
        "model".into(),
        toml::Value::String("actual-cli-model".into()),
    )];
    let local = bootstrap(
        home.path(),
        Some(&cwd),
        overrides.clone(),
        loader.clone(),
        false,
        CloudConfigBundleLoader::default(),
    )
    .await?;
    ensure!(local.config_toml.model.as_deref() == Some("actual-cli-model"));
    Ok(())
}

#[tokio::test]
async fn original_bootstrap_strict_error_is_preserved_for_backend_cleanup() -> color_eyre::Result<()>
{
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        "unknown_shared_bootstrap_key = true\n",
    )?;
    let result = bootstrap(
        home.path(),
        None,
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        true,
        CloudConfigBundleLoader::default(),
    )
    .await;
    ensure!(
        result.is_err(),
        "strict original loader silently accepted unknown field"
    );
    Ok(())
}
