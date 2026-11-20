// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native fixture evidence and tracing for its exact validated child.

pub(super) struct HandshakeWitness<'a> {
    fake: &'a codex_alab_aterm::fake::FakeAterm,
    path: std::path::PathBuf,
    persisted: bool,
}

impl<'a> HandshakeWitness<'a> {
    pub(super) fn new(
        fake: &'a codex_alab_aterm::fake::FakeAterm,
        path: std::path::PathBuf,
    ) -> Self {
        Self {
            fake,
            path,
            persisted: false,
        }
    }

    pub(super) fn persist(&mut self) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.fake.handshake_events())
            .map_err(std::io::Error::other)?;
        std::fs::write(&self.path, bytes)?;
        self.persisted = true;
        Ok(())
    }
}

impl Drop for HandshakeWitness<'_> {
    fn drop(&mut self) {
        if !self.persisted {
            // Preserve setup/collection failures without replacing their original error.
            let _ = self.persist();
        }
    }
}

pub(super) fn init() -> color_eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("off,codex_alab_autopilot=debug")
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .try_init()
        .map_err(|error| {
            color_eyre::eyre::eyre!("could not install isolated launch diagnostics: {error}")
        })
}

/// Select supported ordinary diagnostics before the profile removes scratch.
/// Unknown lines are omitted, not interpreted as artifact or native context data.
pub(super) fn ordinary_child_stderr(stderr: &[u8]) -> String {
    const LIMIT: usize = 8192;
    const TRUNCATED: &str = "\n[ordinary stderr excerpt truncated]";
    let mut excerpt = String::new();
    let mut context = 0;
    let mut omitted = 0_usize;
    for raw in stderr.split(|byte| *byte == b'\n') {
        if raw.len() > 1024 {
            context = 0;
            omitted += 1;
            continue;
        }
        let line = String::from_utf8_lossy(raw);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let heading = line.starts_with("Error:")
            || (line.starts_with("thread '") && line.contains(" panicked at "))
            || (line.starts_with("thread '") && line.ends_with(" has overflowed its stack"))
            || line == "fatal runtime error: stack overflow, aborting"
            || line.starts_with("assertion ")
            || line == "Location:"
            || line == "Caused by:";
        let ordinary_cause = [
            "failed ",
            "could not ",
            "cannot ",
            "missing ",
            "invalid ",
            "called `",
            "no such file ",
            "operation not permitted",
            "permission denied",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
        let mut location = line.rsplitn(3, ':');
        let source_location = matches!(
            (location.next(), location.next(), location.next()),
            (Some(column), Some(row), Some(file)) if file.ends_with(".rs")
                && !row.is_empty() && row.bytes().all(|byte| byte.is_ascii_digit())
                && !column.is_empty() && column.bytes().all(|byte| byte.is_ascii_digit())
        );
        if heading {
            context = 2;
        } else if context == 0 || !(ordinary_cause || source_location) {
            context = 0;
            omitted += 1;
            continue;
        } else {
            context -= 1;
        }
        if line.contains(['{', '}', '='])
            || line.starts_with(['[', '"'])
            || lower.contains("handshake")
            || lower.contains("native_context")
            || lower.contains("native-context")
            || lower.contains("native context")
            || lower.contains("nativefd")
            || lower.contains("native_fd")
            || lower.contains("descriptor")
            || lower.contains("environment:")
            || lower.contains("env:")
            || lower.contains("fd:")
            || lower.starts_with("left:")
            || lower.starts_with("right:")
        {
            context = 0;
            omitted += 1;
            continue;
        }
        if !excerpt.is_empty() {
            excerpt.push('\n');
        }
        for character in line.chars().filter(|character| !character.is_control()) {
            // Reserve room for either bounded omission counts or the truncation marker.
            if excerpt.len() + character.len_utf8() + 96 > LIMIT {
                excerpt.push_str(TRUNCATED);
                return excerpt;
            }
            excerpt.push(character);
        }
    }
    if excerpt.is_empty() {
        excerpt.push_str("(no ordinary child error lines captured)");
    }
    if omitted > 0 {
        excerpt.push_str(&format!("\n[{omitted} non-ordinary stderr lines omitted]"));
    }
    excerpt
}

#[cfg(test)]
mod tests {
    use super::ordinary_child_stderr;

    #[test]
    fn ordinary_child_stderr_keeps_error_panic_and_location() {
        let stderr = b"unrelated trace\nError: could not locate binary codex-code-mode-host\n\nLocation:\n    fixture.rs:42:9\nthread 'child' (123) panicked at fixture.rs:55:3:\nassertion failed: launch completed\n";
        let excerpt = ordinary_child_stderr(stderr);
        assert!(excerpt.contains("Error: could not locate binary codex-code-mode-host"));
        assert!(excerpt.contains("fixture.rs:42:9"));
        assert!(excerpt.contains("thread 'child' (123) panicked at fixture.rs:55:3:"));
        assert!(excerpt.contains("assertion failed: launch completed"));
        assert!(!excerpt.contains("unrelated trace"));
    }

    #[test]
    fn ordinary_child_stderr_omits_opaque_payloads_and_handles_invalid_utf8() {
        let stderr = b"Error: native_context {\"secret\":\"opaque-one\"}\nError: handshake opaque-two\nError: NativeFD opaque-three\nError: ALAB_SECRET=opaque-four\nError: setup failed\n{\"secret\":\"opaque-five\"}\nopaque-six\nError: another failure\nALAB_SECRET: opaque-seven\nError: invalid byte \xff\n";
        let excerpt = ordinary_child_stderr(stderr);
        assert!(excerpt.contains("Error: setup failed"));
        assert!(excerpt.contains("Error: invalid byte \u{fffd}"));
        assert!(!excerpt.contains("opaque-"));
        assert_eq!(
            ordinary_child_stderr(b"{\"secret\":1}\n"),
            "(no ordinary child error lines captured)\n[1 non-ordinary stderr lines omitted]"
        );
    }

    #[test]
    fn ordinary_child_stderr_keeps_fatal_stack_overflow() {
        let stderr = b"thread 'private-child' has overflowed its stack\nfatal runtime error: stack overflow, aborting\nthread 'private-child' has unrelated data\n";
        let excerpt = ordinary_child_stderr(stderr);
        assert!(excerpt.contains("thread 'private-child' has overflowed its stack"));
        assert!(excerpt.contains("fatal runtime error: stack overflow, aborting"));
        assert!(!excerpt.contains("unrelated data"));
    }

    #[test]
    fn ordinary_child_stderr_bounds_output_and_skips_oversized_lines() {
        let oversized = format!("Error: {}\n", "private".repeat(200));
        assert_eq!(
            ordinary_child_stderr(oversized.as_bytes()),
            "(no ordinary child error lines captured)\n[1 non-ordinary stderr lines omitted]"
        );
        let stderr = "Error: ordinary failure \u{fffd}\n".repeat(1000);
        let excerpt = ordinary_child_stderr(stderr.as_bytes());
        assert!(excerpt.len() <= 8192);
        assert!(excerpt.ends_with("[ordinary stderr excerpt truncated]"));
    }
}
