// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use anyhow::Result;
use anyhow::ensure;
use portable_pty::cmdbuilder::CommandBuilder;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

pub(super) fn resolve_current_directory(cmd: &CommandBuilder) -> Result<Option<Vec<u16>>> {
    let dir = match cmd.get_cwd() {
        Some(path) => {
            ensure!(!path.is_empty(), "empty PTY working directory");
            ensure!(
                !path.encode_wide().any(|unit| unit == 0),
                "NUL in PTY working directory"
            );
            path.as_os_str()
        }
        None => match cmd
            .get_env("USERPROFILE")
            .filter(|path| Path::new(path).is_dir())
        {
            Some(home) => home,
            None => return Ok(None),
        },
    };
    // CreateProcessW expects a full path. Preserve absolute paths as supplied and
    // resolve Windows drive-relative paths without checking that the directory exists.
    let absolute: OsString;
    let path = if Path::new(dir).is_absolute() {
        dir
    } else {
        absolute = std::path::absolute(dir)?.into_os_string();
        &absolute
    };
    Ok(Some(path.encode_wide().chain(std::iter::once(0)).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;

    #[test]
    fn explicit_directory_reaches_create_process_unchanged() -> Result<()> {
        let fixture = tempfile::tempdir()?;
        let mut command = CommandBuilder::new("cmd.exe");
        command.env_clear();
        assert!(resolve_current_directory(&command)?.is_none());
        command.env("USERPROFILE", fixture.path());
        let as_path = |wide: Vec<u16>| OsString::from_wide(wide.strip_suffix(&[0]).unwrap());
        assert_eq!(
            as_path(resolve_current_directory(&command)?.unwrap()),
            fixture.path().as_os_str()
        );
        let file = fixture.path().join("file");
        std::fs::write(&file, b"fixture")?;
        for path in [fixture.path().join("missing"), file] {
            command.cwd(&path);
            assert_eq!(
                as_path(resolve_current_directory(&command)?.unwrap()),
                path.as_os_str()
            );
        }
        for bad in [
            OsString::new(),
            OsString::from_wide(&[b'.' as u16, 0, b'x' as u16]),
        ] {
            command.cwd(bad);
            assert!(resolve_current_directory(&command).is_err());
        }
        Ok(())
    }
}
