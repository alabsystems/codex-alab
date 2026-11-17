// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original source builder must clear only destination cwd while preserving
//! actual configuration inputs. No ConfigManager substitution or fake SDK.
use super::*;
use color_eyre::eyre::ensure;
use tempfile::TempDir;

#[tokio::test]
async fn original_source_builder_clears_destination_and_retains_real_overrides()
-> color_eyre::Result<()> {
    let home = TempDir::new()?;
    let destination = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        "model = 'stored-source-model'\n",
    )?;
    let builder = create(
        home.path(),
        vec![(
            "model".into(),
            toml::Value::String("actual-source-override".into()),
        )],
        ConfigOverrides {
            cwd: Some(destination.path().into()),
            ..Default::default()
        },
        LoaderOverrides::without_managed_config_for_tests(),
        CloudConfigBundleLoader::default(),
    );
    let actual = builder.build().await?;
    ensure!(actual.model.as_deref() == Some("actual-source-override"));
    ensure!(actual.cwd.as_path() == std::env::current_dir()?);
    ensure!(actual.cwd.as_path() != destination.path());
    ensure!(actual.codex_home.as_path() == home.path());
    Ok(())
}
