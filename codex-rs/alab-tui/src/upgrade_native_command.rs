// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Decode the native host's canonical command as bounded data. Never run a
//! shell or infer constructor admission from these bytes. Execution is created
//! only by the original source-exec operation's authenticated Go response.
use crate::upgrade::ExecPlan;
use crate::upgrade_admission::Receipt;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::PathBuf;

const LIMIT: usize = 64 * 1024;
const LAUNCH: &str = codex_alab_core::native_source::LAUNCH_ENV;

pub(crate) fn decode(
    encoded: &str,
    expected: &ExecPlan,
    admitted: &Receipt,
) -> Result<ExecPlan, String> {
    let bytes = unhex(encoded, LIMIT)?;
    let command = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let mut fields = BTreeMap::new();
    for word in command.split_ascii_whitespace() {
        let (key, value) = word.split_once('=').ok_or_else(invalid)?;
        if value.is_empty() || fields.insert(key, value).is_some() {
            return Err(invalid());
        }
    }
    if fields.len() != 10
        || fields.get("heal") != Some(&"-")
        || fields.get("dialect") != Some(&"zsh")
    {
        return Err(invalid());
    }
    let get = |name| fields.get(name).copied().ok_or_else(invalid);
    let image = PathBuf::from(text(get("exec-path")?)?);
    let program = PathBuf::from(text(get("program")?)?);
    if image != admitted.executable.canonicalize().map_err(|_| invalid())?
        || program != image
        || image != expected.path
        || get("exec-dev")? != admitted.device.to_string()
        || get("exec-ino")? != admitted.inode.to_string()
    {
        return Err(invalid());
    }
    let argv: Vec<String> = json(get("argv")?)?;
    let env: Vec<(String, String)> = json(get("env")?)?;
    let unset: Option<Vec<String>> = json(get("unset")?)?;
    let cwd = match get("cwd")? {
        "-" => None,
        value => Some(PathBuf::from(text(value)?)),
    };
    if unset.is_some()
        || cwd != expected.cwd
        || argv.len() > 256
        || env.len() > 256
        || argv.len() + 1 != expected.argv.len()
        || argv
            .iter()
            .zip(&expected.argv[1..])
            .any(|(actual, expected)| OsStr::new(actual) != expected)
    {
        return Err(invalid());
    }
    let mut launch = None;
    let mut seen = std::collections::BTreeSet::new();
    for (key, value) in &env {
        if !seen.insert(key.as_str()) || key.contains('\0') || value.contains('\0') {
            return Err(invalid());
        }
        if key == LAUNCH {
            if value.len() != 32
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(invalid());
            }
            launch = Some(value);
        } else if !matches!(
            key.as_str(),
            "HOME" | "CODEX_HOME" | "CODEX_AUTH_PATH" | "CODEX_AUTH_SLOT"
        ) || !expected.env.iter().any(|(name, original)| {
            name.as_os_str() == OsStr::new(key) && original.as_os_str() == OsStr::new(value)
        }) {
            return Err(invalid());
        }
    }
    let launch = launch.ok_or_else(invalid)?;
    // The command must carry every selected source export, not quietly drop a
    // home/credential-slot selection when replacing its one constructor token.
    for key in ["HOME", "CODEX_HOME", "CODEX_AUTH_PATH", "CODEX_AUTH_SLOT"] {
        if expected
            .env
            .iter()
            .any(|(name, _)| name.as_os_str() == OsStr::new(key))
            != seen.contains(key)
        {
            return Err(invalid());
        }
    }
    let mut result = expected.clone();
    result
        .env
        .retain(|(name, _)| name.as_os_str() != OsStr::new(LAUNCH));
    result.env.push((LAUNCH.into(), launch.as_str().into()));
    Ok(result)
}

fn json<T: serde::de::DeserializeOwned + serde::Serialize>(value: &str) -> Result<T, String> {
    let text = text(value)?;
    let decoded = serde_json::from_str(&text).map_err(|_| invalid())?;
    // The host stores a canonical command, not alternate JSON representations.
    if serde_json::to_string(&decoded).map_err(|_| invalid())? != text {
        return Err(invalid());
    }
    Ok(decoded)
}
fn text(value: &str) -> Result<String, String> {
    let value = String::from_utf8(unhex(value, LIMIT)?).map_err(|_| invalid())?;
    if value.contains('\0') {
        return Err(invalid());
    }
    Ok(value)
}
pub(crate) fn unhex(value: &str, limit: usize) -> Result<Vec<u8>, String> {
    if value.len() > limit.saturating_mul(2) || !value.len().is_multiple_of(2) {
        return Err(invalid());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            Ok(digit(pair[0]).ok_or_else(invalid)? * 16 + digit(pair[1]).ok_or_else(invalid)?)
        })
        .collect()
}
fn invalid() -> String {
    "native runtime execution command does not match the admitted invocation".into()
}
