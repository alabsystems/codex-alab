// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
#[path = "running_version_process.rs"]
mod process;

#[test]
fn compiled_build_identity_preserves_packaged_credential_store_policy() -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    process::run(module_path!())?;
    assert!(!codex_alab_core::active());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[expect(clippy::print_stdout, reason = "exact child completion handshake for the owning test")]
async fn running_version_consumer_child() -> std::io::Result<()> {
    let Some((version, _)) = process::identity(module_path!()) else {
        return Ok(());
    };
    codex_alab_core::version::initialize(version, None, None).expect("first identity");
    assert!(!codex_alab_core::active());
    let home = tempfile::Builder::new()
        .prefix("version-config-")
        .tempdir()?;
    let config = Config::load_from_base_config_with_overrides(
        ConfigToml {
            cli_auth_credentials_store: Some(AuthCredentialsStoreMode::Keyring),
            mcp_oauth_credentials_store: Some(OAuthCredentialsStoreMode::Auto),
            ..Default::default()
        },
        ConfigOverrides::default(),
        AbsolutePathBuf::from_absolute_path(home.path())
            .expect("absolute private home"),
    )
    .await?;
    assert_eq!(
        config.cli_auth_credentials_store_mode,
        AuthCredentialsStoreMode::Keyring
    );
    assert_eq!(
        config.mcp_oauth_credentials_store_mode,
        OAuthCredentialsStoreMode::Auto
    );
    println!("{}", process::COMPLETE);
    Ok(())
}
