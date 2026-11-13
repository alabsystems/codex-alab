// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Observe task ownership before a current-thread runtime can poll spawned work.
//! No timers, network requests or scheduling races establish the negative result.

use super::CodexAuth;
use super::Product;
use super::load_config;
use super::set_test_auth;
use super::test_auth_manager;
use super::test_plugins_manager;
use super::test_plugins_manager_with_auth_manager;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "current_thread")]
async fn disabled_plugins_and_recommendations_schedule_no_background_work() {
    let home = tempfile::tempdir().unwrap();
    let mut config = load_config(home.path(), home.path())
        .await
        .with_plugin_recommendations_enabled(false);
    config.plugins_enabled = false;
    let manager = std::sync::Arc::new(test_plugins_manager(home.path().to_path_buf()));
    manager.maybe_start_remote_plugin_caches_refresh(
        &config,
        Some(CodexAuth::create_dummy_chatgpt_auth_for_testing()),
        None,
    );
    // Tokio never polls a spawned task synchronously. Before this test yields,
    // an accidentally scheduled recommendation worker still owns its manager.
    assert_eq!(std::sync::Arc::strong_count(&manager), 1);
    let state = manager
        .remote_installed_plugins_cache_refresh_state
        .read()
        .unwrap();
    assert!(!state.in_flight);
    assert!(state.requested.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_recommendations_preserve_only_the_installed_plugin_worker() {
    let home = tempfile::tempdir().unwrap();
    let mut config = load_config(home.path(), home.path())
        .await
        .with_plugin_recommendations_enabled(false);
    config.plugins_enabled = true;
    let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();
    let auth_manager = test_auth_manager(None);
    set_test_auth(&auth_manager, Some(auth.clone())).await;
    let manager = std::sync::Arc::new(test_plugins_manager_with_auth_manager(
        home.path().to_path_buf(),
        Some(Product::Codex),
        auth_manager,
    ));
    manager.maybe_start_remote_plugin_caches_refresh(&config, Some(auth), None);
    let state = manager
        .remote_installed_plugins_cache_refresh_state
        .read()
        .unwrap();
    assert!(state.in_flight);
    assert!(state.requested.is_some());
    // Exactly the installed-plugin task owns the additional reference. An
    // unconditional recommendation task would retain a third reference.
    assert_eq!(std::sync::Arc::strong_count(&manager), 2);
}
