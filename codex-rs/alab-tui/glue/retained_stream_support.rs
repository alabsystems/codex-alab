// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Shared closed native streaming DATA helpers; no renderer or resource factory.
use serde::Deserialize;
use std::io;
pub(crate) fn invalid(why: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}
pub(crate) fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<T, D::Error> {
    T::deserialize(d)
}
pub(crate) fn boundary(source: &str, index: usize) -> io::Result<()> {
    if source.is_char_boundary(index) {
        Ok(())
    } else {
        Err(invalid("streaming source boundary outside original UTF-8"))
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct PathData(Vec<u8>);
impl PathData {
    pub(crate) fn capture(path: &std::path::Path) -> Self {
        use std::os::unix::ffi::OsStrExt;
        Self(path.as_os_str().as_bytes().to_vec())
    }
    pub(crate) fn restore(self) -> io::Result<std::path::PathBuf> {
        use std::os::unix::ffi::OsStringExt;
        if self.0.contains(&0) {
            return Err(invalid("streaming cwd contains NUL"));
        }
        Ok(std::ffi::OsString::from_vec(self.0).into())
    }
}
