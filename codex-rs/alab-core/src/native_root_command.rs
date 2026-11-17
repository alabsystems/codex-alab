// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit command data for native Root admission. Encoding is not admission;
//! the original SDK member and bound descriptor channel are required as well.
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

/// Complete pipe-mode command description. The environment is explicit and
/// replaces inheritance. Stdio descriptions travel separately on the bound
/// socket; this value cannot request inheritance of GUI or controller stdio.
pub struct Command {
    pub program: PathBuf,
    pub arg0: Option<OsString>,
    pub arguments: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
    pub directory: PathBuf,
}

#[derive(serde::Serialize)]
struct Specification<'a> {
    version: u32,
    program: &'a [u8],
    arg0: Option<&'a [u8]>,
    arguments: Vec<&'a [u8]>,
    environment: Vec<(&'a [u8], &'a [u8])>,
    directory: &'a [u8],
    setup: &'static str,
}

impl Command {
    /// Preserve Unix argument and path bytes, including non-UTF8 names.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let specification = Specification {
            version: 1,
            program: self.program.as_os_str().as_bytes(),
            arg0: self.arg0.as_ref().map(|arg0| arg0.as_bytes()),
            arguments: self.arguments.iter().map(|arg| arg.as_bytes()).collect(),
            environment: self
                .environment
                .iter()
                .map(|(key, value)| (key.as_bytes(), value.as_bytes()))
                .collect(),
            directory: self.directory.as_os_str().as_bytes(),
            setup: "pipe",
        };
        let valid = |bytes: &[u8]| !bytes.contains(&0);
        if specification.program.is_empty()
            || !valid(specification.program)
            || specification.arg0.is_some_and(|arg0| !valid(arg0))
            || specification.arguments.len() > 4096
            || specification.arguments.iter().any(|arg| !valid(arg))
            || specification.environment.len() > 4096
            || specification.environment.iter().any(|(key, value)| {
                key.is_empty() || key.contains(&b'=') || !valid(key) || !valid(value)
            })
            || !self.directory.is_absolute()
            || !valid(specification.directory)
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut names = std::collections::BTreeSet::new();
        if specification
            .environment
            .iter()
            .any(|(key, _)| !names.insert(key))
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let payload = serde_json::to_vec(&specification).map_err(io::Error::other)?;
        if payload.len() > 512 * 1024 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(payload)
    }
}

#[cfg(test)]
#[path = "native_root_command_tests.rs"]
mod tests;
