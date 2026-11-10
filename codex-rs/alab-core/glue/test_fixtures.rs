// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Fixtures compiled only by codex-core's unit tests.

/// The resistant child inherits SIG_IGN before the parent publishes readiness.
/// A shell builtin wait lets the parent's cleanup trap run during the existing
/// TERM grace instead of deferring it behind a foreground external sleep.
#[cfg(unix)]
pub(crate) const SIGTERM_CLEANUP_SCRIPT: &str = r#"trap '' TERM
sleep 60 &
descendant_pid=$!
printf '%s' "$descendant_pid" > "$DESCENDANT_PID_MARKER"
trap 'printf cleaned > "$CLEANUP_MARKER"; exit 0' TERM
printf ready > "$READY_MARKER"
while :; do wait "$descendant_pid"; done"#;

/// Bounded phase names and timings make an unchanged test deadline actionable.
#[expect(
    clippy::print_stderr,
    reason = "test-only phase diagnostics must survive a deadline failure"
)]
pub(crate) fn phase(name: &'static str) -> Phase {
    eprintln!("ALab test phase started: {name}");
    Phase {
        name,
        started: std::time::Instant::now(),
    }
}

pub(crate) struct Phase {
    name: &'static str,
    started: std::time::Instant,
}

impl Drop for Phase {
    #[expect(
        clippy::print_stderr,
        reason = "test-only phase diagnostics include cancelled futures"
    )]
    fn drop(&mut self) {
        eprintln!(
            "ALab test phase left: {} after {:?}",
            self.name,
            self.started.elapsed()
        );
    }
}

/// Session constructors drop their local home before returning. Keep a distinct
/// workspace for the command's lifetime; the contained driver owns outer TMPDIR
/// cleanup after the tests. Uncontained runs retain this generated scratch.
pub(crate) fn session_workspace() -> std::path::PathBuf {
    tempfile::Builder::new()
        .prefix("alab-core-workspace.")
        .tempdir()
        .expect("create session fixture workspace")
        .keep()
}

/// Keep the real /tmp -> /private/tmp alias while staying inside an admitted
/// command scratch directory. Outside that scratch, retain the original /tmp
/// fixture location rather than substituting an unrelated system alias.
#[cfg(target_os = "macos")]
pub(crate) fn system_alias_directory() -> std::io::Result<tempfile::TempDir> {
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    let temporary = std::env::temp_dir().canonicalize()?;
    let alias_parent = temporary
        .strip_prefix("/private/tmp")
        .map(|suffix| Path::new("/tmp").join(suffix))
        .unwrap_or_else(|_| Path::new("/tmp").to_path_buf());
    let directory = tempfile::tempdir_in(alias_parent)?;
    let alias = directory.path();
    let canonical = alias.canonicalize()?;
    assert!(alias.starts_with("/tmp"));
    assert_ne!(alias, canonical.as_path());
    assert_eq!(
        canonical,
        Path::new("/private/tmp").join(alias.strip_prefix("/tmp").expect("literal /tmp alias")),
    );
    let raw_metadata = alias.metadata()?;
    let canonical_metadata = canonical.metadata()?;
    assert_eq!(
        (raw_metadata.dev(), raw_metadata.ino()),
        (canonical_metadata.dev(), canonical_metadata.ino()),
    );
    Ok(directory)
}
