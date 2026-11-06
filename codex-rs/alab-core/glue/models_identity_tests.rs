// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(model-poll) glue tests, compiled inside codex-model-provider as
//! `models_identity::alab_tests` (one `#[path]` line at the end of
//! `model-provider/src/models_identity.rs`): the cache identity's two arms. codex's `identity`
//! names its own build. Only exact private children activate; the shared test process
//! retains upstream behavior.

use super::*;
use pretty_assertions::assert_eq;

#[path = "running_version_process.rs"]
mod process;

#[test]
fn the_actual_catalog_identity_uses_the_running_stamp_not_the_library_version()
-> std::io::Result<()> {
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
    codex_alab_core::activate();
    let mut provider = ModelProviderInfo::create_openai_provider(None);
    // Isolate the build salt from the independent version HTTP header.
    provider.http_headers = None;
    let actual = identity(&provider, None).expect("actual catalog identity");
    assert_eq!(
        actual,
        identity_of_build(&provider, None, Some(version)).expect("stamped identity")
    );
    assert_ne!(
        actual,
        identity_of_build(&provider, None, Some(env!("CARGO_PKG_VERSION")))
            .expect("library identity")
    );
    assert_ne!(
        actual,
        identity_of_build(&provider, None, None).expect("upstream identity")
    );
    println!("{}", process::COMPLETE);
}

/// A catalog cached by another codex-alab build is keyed apart from this build's, so it never
/// serves this one as fresh: every dev build's cache says client version `0.165.0`, and an older
/// build writes back only the catalog fields it knows. NEGATIVE CONTROL: without the build in
/// the digest, the two builds' identities are equal.
#[test]
fn each_codex_alab_build_keys_its_own_catalog() {
    let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
    let auth = CodexAuth::from_api_key("api-key");
    let build = |build| identity_of_build(&provider, Some(&auth), build).unwrap();
    let older = build(Some("0.165.0-dev.13344+gf02eae4fc"));
    let newer = build(Some("0.165.0-dev.13355+g5ba53cc6a"));
    assert_ne!(older, newer);
    assert_eq!(build(Some("0.165.0-dev.13355+g5ba53cc6a")), newer, "stable");
    assert_ne!(build(None), newer, "upstream's identity is not a build's");
}

/// Upstream's identity (every binary but codex) is the digest it always was.
#[test]
fn upstreams_identity_is_unchanged() {
    assert!(!codex_alab_core::active());
    let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
    let auth = CodexAuth::from_api_key("api-key");
    assert_eq!(
        identity(&provider, Some(&auth)).unwrap(),
        identity_of_build(&provider, Some(&auth), None).unwrap()
    );
}
