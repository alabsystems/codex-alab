// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
#[path = "running_version_process.rs"]
mod process;

#[test]
fn mcp_headers_keep_the_full_running_build_and_explicit_overrides() -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    process::run(module_path!())?;
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
fn running_version_consumer_child() {
    let Some((version, _)) = process::identity(module_path!()) else {
        return;
    };
    codex_alab_core::version::initialize(version, None, None).expect("first identity");
    assert!(!codex_alab_core::active());
    let expected = format!("codex-mcp-client/{version}");
    let headers = build_default_headers(None, None).expect("default headers");
    assert_eq!(
        headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok()),
        Some(expected.as_str())
    );
    let overrides = HashMap::from([("user-agent".to_owned(), "explicit-client/9".to_owned())]);
    let headers = build_default_headers(Some(overrides), None).expect("explicit headers");
    assert_eq!(
        headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok()),
        Some("explicit-client/9")
    );
    println!("{}", process::COMPLETE);
}
