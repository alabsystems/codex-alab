// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
#![cfg(unix)]

use anyhow::Result;
use std::path::Path;
use std::process::Command;

const ID: &str = "0123456789abcdef0123456789abcdef";

fn apply(home: &Path, operation: &str, scope: &str) -> Result<std::process::Output> {
    Ok(Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", home)
        .env("HOME", home)
        .env_remove("CODEX_AUTH_PATH")
        .env_remove("OPENAI_API_KEY")
        .env_remove("ATERM_SESSION_ID")
        .current_dir(home)
        .args(["debug", "aterm-model-policy", operation, scope])
        .output()?)
}

#[test]
fn sessions_only_receipt_survives_restart_and_scope_collision_is_refused() -> Result<()> {
    let home = tempfile::tempdir()?;
    let first = apply(home.path(), ID, "sessions")?;
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&first.stdout)?;
    assert_eq!(
        receipt,
        serde_json::json!({"version":1,"operation":ID,"scope":"sessions","sessions":[],"defaults":null})
    );
    assert_eq!(apply(home.path(), ID, "sessions")?.stdout, first.stdout);
    assert!(!apply(home.path(), ID, "defaults")?.status.success());
    assert!(
        !apply(home.path(), "../../other", "sessions")?
            .status
            .success()
    );
    assert!(!home.path().join("config.toml").exists());
    Ok(())
}

#[test]
fn explicit_launch_pin_is_reported_and_preserved() -> Result<()> {
    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    let original = "model = 'gpt-6.2-astra'\nmodel_reasoning_effort = 'low'\n";
    std::fs::write(&path, original)?;
    let output = apply(home.path(), ID, "defaults,sessions")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(receipt["defaults"]["status"], "overridden");
    assert_eq!(std::fs::read_to_string(path)?, original);
    Ok(())
}

#[test]
fn unpinned_defaults_use_the_serving_catalog_and_remain_unpinned() -> Result<()> {
    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    std::fs::write(
        &path,
        "# keep my settings\nmodel_reasoning_effort = 'low'\n",
    )?;
    let output = apply(home.path(), ID, "defaults")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let default = &receipt["defaults"];
    assert_eq!(default["status"], "applied");
    assert_eq!(default["effort"], "ultra");
    assert!(default["model"].as_str().unwrap().ends_with("-sol"));
    let config = std::fs::read_to_string(&path)?;
    assert!(config.contains("# keep my settings"));
    let data: toml::Value = toml::from_str(&config)?;
    assert!(data.get("model").is_none());
    assert_eq!(data["model_reasoning_effort"].as_str(), Some("ultra"));
    std::fs::write(&path, "model = 'gpt-6.2-astra'\n")?;
    assert_eq!(apply(home.path(), ID, "defaults")?.stdout, output.stdout);
    assert_eq!(std::fs::read_to_string(&path)?, "model = 'gpt-6.2-astra'\n");
    Ok(())
}

#[test]
fn later_personal_edit_is_overridden_and_multiple_registered_homes_keep_their_own_defaults()
-> Result<()> {
    let account = tempfile::tempdir()?;
    let primary = account.path().join(".codex");
    let custom = account.path().join("other-codex");
    for home in [&primary, &custom] {
        std::fs::create_dir(home)?;
    }
    let registry = primary.join("alab/model-homes");
    codex_alab_tui::record::ensure_dir(&registry)?;
    let registered = codex_alab_tui::model_homes::Home::owned(&custom)?;
    std::fs::write(
        registry.join("ab".repeat(32)),
        serde_json::to_vec(&registered)?,
    )?;
    std::fs::write(custom.join("config.toml"), "model = 'gpt-6.2-astra'\n")?;
    let call = |id: &str| -> Result<serde_json::Value> {
        let out = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
            .env("HOME", account.path())
            .env("CODEX_HOME", &primary)
            .env_remove("CODEX_AUTH_PATH")
            .env_remove("OPENAI_API_KEY")
            .current_dir(account.path())
            .args(["debug", "aterm-model-policy", id, "defaults"])
            .output()?;
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(serde_json::from_slice(&out.stdout)?)
    };
    let first = call(ID)?;
    assert_eq!(first["defaults"]["status"], "applied");
    assert_eq!(
        first["homes"][0]["receipt"]["defaults"]["status"],
        "overridden"
    );
    std::fs::write(
        primary.join("config.toml"),
        "model_reasoning_effort = 'low'\n",
    )?;
    let next = call("ffffffffffffffffffffffffffffffff")?;
    assert_eq!(next["defaults"]["status"], "overridden");
    assert_eq!(
        std::fs::read_to_string(primary.join("config.toml"))?,
        "model_reasoning_effort = 'low'\n"
    );
    Ok(())
}
