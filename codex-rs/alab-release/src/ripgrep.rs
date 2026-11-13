// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Trust-built auxiliary executable, independently bound to the product compiler.

use crate::Release;
use crate::Result;
use crate::product;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub intent: String,
    pub version: String,
    pub binary: PathBuf,
    pub target: PathBuf,
    pub compiler: product::Compiler,
    pub sha256: String,
}

impl Build {
    pub fn check(
        &self,
        repo: &Path,
        release: &Release,
        compiler: &product::Compiler,
    ) -> Result<()> {
        if self.intent != release.id()?
            || self.version != crate::assembly::ripgrep_version(repo, release)?
            || &self.compiler != compiler
            || product::file_hash(&self.binary)? != self.sha256
            || product::observe_compiler(&self.target, &compiler.rustc_vv, &compiler.sysroot)?
                != *compiler
        {
            return Err(
                "ripgrep build differs from its source pin, compiler or observed bytes".into(),
            );
        }
        Ok(())
    }
}

pub fn arguments(version: &str, target: &Path, root: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "--unverified".into(),
        "install".into(),
        "--locked".into(),
        "--features".into(),
        "pcre2".into(),
        format!("ripgrep@{version}").into(),
        "--target-dir".into(),
        target.into(),
        "--root".into(),
        root.into(),
    ]
}

pub fn check_version(output: &std::process::Output, version: &str) -> Result<()> {
    let text = std::str::from_utf8(&output.stdout)?;
    if !output.status.success()
        || !text.lines().next().is_some_and(|line| {
            line == format!("ripgrep {version}") || line.starts_with(&format!("ripgrep {version} "))
        })
        || !text.lines().any(|line| {
            line.strip_prefix("features:").is_some_and(|features| {
                features
                    .split_whitespace()
                    .any(|feature| feature == "+pcre2")
            })
        })
        || !text
            .lines()
            .any(|line| line.starts_with("PCRE2 ") && line.contains("is available"))
    {
        return Err("Trust-built ripgrep does not report the pinned version with PCRE2".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn version_probe_requires_success_exact_version_and_pcre2() {
        let mut out = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"ripgrep 15.2.0 (test)\nfeatures:+pcre2\nPCRE2 10.46 is available\n".to_vec(),
            stderr: vec![],
        };
        assert!(check_version(&out, "15.2.0").is_ok());
        assert!(check_version(&out, "15.2.1").is_err());
        out.status = std::process::ExitStatus::from_raw(256);
        assert!(check_version(&out, "15.2.0").is_err());
        out.status = std::process::ExitStatus::from_raw(0);
        out.stdout = b"ripgrep 15.2.0\nfeatures: -pcre2\n".to_vec();
        assert!(check_version(&out, "15.2.0").is_err());
    }
}
