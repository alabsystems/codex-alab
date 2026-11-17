// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual original child/open/read/result controls. No C0 Native role,
//! renderer, frontend entry, kernel FD-retirement or model permission is made.
use super::*;
use std::process::Stdio;

#[tokio::test(flavor = "current_thread")]
async fn c0_keeps_original_file_symlink_utf8_and_native_errors_with_one_result() -> io::Result<()> {
    for case in ["file", "symlink", "invalid-utf8", "missing", "directory"] {
        let root = tempfile::tempdir()?;
        let document = tempfile::NamedTempFile::new_in(root.path())?.into_temp_path();
        let path = document.to_path_buf();
        let reference = root.path().join("canonical-reference");
        match case {
            "file" => {
                std::fs::write(&path, "same original text é\n")?;
                std::fs::write(&reference, "same original text é\n")?;
            }
            "symlink" => {
                let target = root.path().join("actual-link-target");
                let reference_target = root.path().join("reference-link-target");
                std::fs::write(&target, "same followed link\n")?;
                std::fs::write(&reference_target, "same followed link\n")?;
                std::fs::remove_file(&path)?;
                std::os::unix::fs::symlink(target, &path)?;
                std::os::unix::fs::symlink(reference_target, &reference)?;
            }
            "invalid-utf8" => {
                std::fs::write(&path, [0xff, 0xfe, 0x00])?;
                std::fs::write(&reference, [0xff, 0xfe, 0x00])?;
            }
            "missing" => std::fs::remove_file(&path)?,
            "directory" => {
                std::fs::remove_file(&path)?;
                std::fs::create_dir(&path)?;
                std::fs::create_dir(&reference)?;
            }
            _ => return Err(unavailable("unknown original-read control case")),
        }
        // Independent reference path; the actual original operation opens and
        // reads its own document once, never rereads to construct a receipt.
        let expected = std::fs::read_to_string(&reference);
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let operation = Operation::prepare(command, document)?;
        operation.start()?;
        let capture = operation.capture_c0()?;
        let spawned = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            capture.spawn_completed(),
        )
        .await
        .map_err(io::Error::other)??;
        assert!(spawned.is_for(&capture) && spawned.is_for_operation(&operation));
        let finished = tokio::time::timeout(std::time::Duration::from_secs(60), capture.finished())
            .await
            .map_err(io::Error::other)??;
        match (expected, finished.outcome()) {
            (Ok(expected), Ok(actual)) => assert_eq!(actual, expected, "{case}"),
            (Err(expected), Err(Failure::Document(actual))) => {
                assert_eq!(actual.kind(), expected.kind(), "{case}");
                assert_eq!(actual.raw_os_error(), expected.raw_os_error(), "{case}");
                assert_eq!(actual.to_string(), expected.to_string(), "{case}");
            }
            (expected, actual) => panic!(
                "{case}: canonical {expected:?}, actual {:?}",
                actual.map_err(|error| error.to_string())
            ),
        }
        let repeated = capture.finished().await?;
        assert!(
            Arc::ptr_eq(&finished.value, &repeated.value),
            "actual native result was recreated"
        );
        let observed = capture.observe()?;
        assert_eq!(observed.phase(), C0Phase::FinishedPending);
        let read = observed
            .original_read()
            .ok_or_else(|| unavailable("original read slot absent"))?;
        assert_eq!(read.phase()?, OriginalReadPhase::Settled);
        assert!(
            read.description()?.is_none(),
            "completed original File must not remain pinned by an observer"
        );
        finished.apply_and_retire(|| Ok(()))?;
        assert!(capture.observe().is_err());
    }
    Ok(())
}
