// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;

#[cfg(unix)]
#[tokio::test]
async fn shared_original_builder_keeps_seed_argv_and_original_cleanup() {
    let home = tempfile::tempdir().unwrap();
    let script = home.path().join("actual-editor.sh");
    codex_utils_cargo_bin::write_executable(&script,
        "#!/bin/sh\n[ \"$1\" = 'literal argument with spaces' ] || exit 7\n[ \"$(cat \"$2\")\" = 'original seed' ] || exit 8\nprintf 'edited original document' > \"$2\"\n").unwrap();
    let original_argv = vec![
        script.to_string_lossy().into_owned(),
        "literal argument with spaces".into(),
    ];
    let prepared = prepare(
        "original seed",
        &original_argv,
        home.path(),
        &FileSystemSandboxPolicy::read_only(),
        home.path(),
    )
    .unwrap();
    let original_document = prepared.document.to_path_buf();
    assert_eq!(
        prepared.run_original().await.unwrap(),
        "edited original document"
    );
    assert!(
        !original_document.exists(),
        "same original TempPath deletion owner must run after completion"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn moving_actual_command_and_document_keeps_one_real_cleanup_owner() {
    let home = tempfile::tempdir().unwrap();
    let script = home.path().join("actual-editor.sh");
    codex_utils_cargo_bin::write_executable(
        &script,
        "#!/bin/sh\nprintf 'same moved document' > \"$1\"\n",
    )
    .unwrap();
    let argv = vec![script.to_string_lossy().into_owned()];
    let prepared = prepare(
        "original seed",
        &argv,
        home.path(),
        &FileSystemSandboxPolicy::read_only(),
        home.path(),
    )
    .unwrap();
    let original = prepared.document.to_path_buf();
    let (mut command, document) = prepared.into_original_parts();
    assert!(command.status().await.unwrap().success());
    assert_eq!(
        fs::read_to_string(&document).unwrap(),
        "same moved document"
    );
    assert!(
        original.exists(),
        "document is retained across real child completion until its owner drains"
    );
    drop(document);
    assert!(!original.exists());
}
