// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Frozen policy routing only. No actual Entered resource, Native success,
//! credential-reader Closed receipt or UI application is constructed here.
use super::*;
use codex_config::CloudConfigBundleSnapshot;
use codex_config::ConfigServer;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::cell::Cell;
fn snapshot(
    server: Option<ConfigServer>,
) -> io::Result<crate::owned_local_ui_snapshot::UiSnapshot> {
    crate::owned_local_ui_snapshot::UiSnapshot::from_backend(
        &CloudConfigBundleSnapshot {
            bundle: Ok(None),
            binding: None,
            config_server: server,
        },
        AbsolutePathBuf::from_absolute_path(std::path::PathBuf::from("/"))
            .map_err(io::Error::other)?,
        None,
    )
}
#[tokio::test]
async fn frozen_backend_policy_is_observed_without_constructing_original_auth() -> io::Result<()> {
    let actual = snapshot(None)?;
    let calls = Cell::new(0);
    ensure_with_snapshot(
        Some(&actual),
        || {
            calls.set(calls.get() + 1);
            Err(io::Error::other("must not read C2 credentials"))
        },
        &ConfigRequirementsToml::default(),
    )
    .await?;
    assert_eq!(calls.get(), 0);
    Ok(())
}
#[tokio::test]
async fn changed_config_server_does_not_fall_back_to_oauth_or_an_empty_default() -> io::Result<()> {
    let actual = snapshot(Some(ConfigServer {
        base_url: "https://original.example/".into(),
        oauth: None,
    }))?;
    let changed = ConfigRequirementsToml {
        config_server: Some(ConfigServer {
            base_url: "https://different.example/".into(),
            oauth: None,
        }),
        ..Default::default()
    };
    let calls = Cell::new(0);
    let result = ensure_with_snapshot(
        Some(&actual),
        || {
            calls.set(calls.get() + 1);
            Err(io::Error::other("must not reauthenticate"))
        },
        &changed,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(calls.get(), 0);
    Ok(())
}
#[tokio::test]
async fn ordinary_auth_constructor_error_remains_original_and_is_evaluated_once() {
    let calls = Cell::new(0);
    let result = ensure_with_snapshot(
        None,
        || {
            calls.set(calls.get() + 1);
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "original caller denial",
            ))
        },
        &ConfigRequirementsToml::default(),
    )
    .await;
    assert_eq!(calls.get(), 1);
    let error = match result {
        Err(error) => error,
        Ok(()) => panic!("ordinary error became success"),
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "original caller denial");
}
