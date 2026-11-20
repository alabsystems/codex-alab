// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A single explicitly selected interactive resume may restrict authentication to the
//! workspace and user whose current login aterm confirmed. This cannot select a credential.

use sha2::Digest;
use sha2::Sha256;
use std::io;
use std::sync::OnceLock;

static EXPECTED: OnceLock<String> = OnceLock::new();

/// The picker marker is the first root argument forwarded to the package. Its managed
/// launcher can prepend its single approval/sandbox default. Do not search positional values.
pub fn capture(arguments: impl Iterator<Item = std::ffi::OsString>) -> io::Result<()> {
    if let Some(raw) = launch_argument(arguments)? {
        install(raw.to_str().ok_or_else(unavailable)?)?;
    }
    Ok(())
}

fn launch_argument(
    mut arguments: impl Iterator<Item = std::ffi::OsString>,
) -> io::Result<Option<std::ffi::OsString>> {
    use std::ffi::OsStr;
    let _program = arguments.next();
    let mut first = arguments.next();
    if first.as_deref() == Some(OsStr::new("--dangerously-bypass-approvals-and-sandbox")) {
        first = arguments.next();
    }
    if first.as_deref() != Some(OsStr::new("--aterm-bound-picker-v1")) {
        return Ok(None);
    }
    arguments.next().map(Some).ok_or_else(unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn selected_launch_reads_only_the_forwarded_first_root_option() {
        let flag = "--aterm-bound-picker-v1";
        let default = "--dangerously-bypass-approvals-and-sandbox";
        let read = |words: &[&str]| launch_argument(words.iter().map(OsString::from)).unwrap();
        assert_eq!(read(&["codex", flag, "digest"]), Some("digest".into()));
        assert_eq!(
            read(&["codex", default, flag, "digest"]),
            Some("digest".into())
        );
        for prefix in ["--", "resume", "--model", "exec"] {
            assert!(read(&["codex", prefix, flag, "digest"]).is_none());
        }
    }
}

pub fn install(expected: &str) -> io::Result<()> {
    if !crate::managed_auth::valid_hex(expected, 64) || EXPECTED.set(expected.to_owned()).is_err() {
        return Err(unavailable());
    }
    Ok(())
}

pub fn expected() -> Option<&'static str> {
    EXPECTED.get().map(String::as_str)
}

pub fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        // Native admission discovers this value-free diagnostic in the selected package.
        "the confirmed subscription is unavailable (CODEX_ALAB_BOUND_PICKER_AUTH_V1)",
    )
}

pub fn principal(workspace: &str, user: &str) -> String {
    fn component(domain: &[u8], value: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(domain);
        digest.update(value.as_bytes());
        format!("{:x}", digest.finalize())
    }
    let mut digest = Sha256::new();
    digest.update(b"aterm/codex-manual-picker/principal/v1\0");
    digest.update(component(b"aterm/codex-renewal/account/v1\0", workspace));
    digest.update(component(b"aterm/codex-renewal/user/v1\0", user));
    format!("{:x}", digest.finalize())
}
