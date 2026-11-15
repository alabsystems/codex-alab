// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bounded parent custody for the real SQLite contract test children.

use std::time::Duration;

/// Return true after the parent ran the exact child, or false in that child.
///
/// The caller supplies its own `module_path!()`, never this helper's namespace.
/// Child detection precedes runtime creation so the original synchronous child
/// bodies can own and destroy their own runtimes and activation state.
pub(super) fn parent_handled(caller_module: &str, name: &str, child_key: &str) -> bool {
    let module = caller_module
        .split_once("::")
        .expect("a state contract test has a module below the crate root")
        .1;
    let exact = format!("{module}::{name}");
    if let Some(child) = std::env::var_os(child_key) {
        assert_eq!(child.to_str(), Some(exact.as_str()));
        return false;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
                .env(child_key, &exact);
            crate::test_child_process::run(command, Duration::from_secs(30))
                .await
                .unwrap_or_else(|error| panic!("{exact}: {error}"))
                .assert_test_success(&exact);
        });
    true
}
