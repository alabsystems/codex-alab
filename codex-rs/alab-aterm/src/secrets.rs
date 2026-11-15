// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THIS MAC'S SECRETS REGISTRY AND NEED LEDGER, through aterm's own CLI: codex-alab links none of
//! aterm's crates, so it runs `aterm secrets ls --json` (each entry's name, file pointer and
//! description, never a value) and `aterm secrets need <want>` (a request the lab's Macs answer)
//! as subprocesses. Each call is bounded ([`LS_TIMEOUT`], [`NEED_TIMEOUT`]); its child is killed
//! where the bound passes. What a call prints is never logged: a failure names its exit and the
//! first line of stderr only.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;

/// The longest `aterm secrets ls --json` may take.
pub const LS_TIMEOUT: Duration = Duration::from_secs(15);

/// The longest `aterm secrets need <want>` may take (it also runs a pass over the lab's table).
pub const NEED_TIMEOUT: Duration = Duration::from_secs(60);

/// The most bytes of a failure's stderr line kept.
const WHY_BYTES: usize = 160;

/// One registry entry, value-free: its name, where its file lands (`~/…`), what it is for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Entry {
    /// The entry's name in the registry.
    pub name: String,
    /// Where its file lands, `None` for an entry with no file.
    #[serde(default)]
    pub pointer: Option<String>,
    /// What it is for.
    #[serde(rename = "description", default)]
    pub purpose: String,
}

#[derive(Deserialize)]
struct Listing {
    entries: Vec<Entry>,
}

/// The entries `aterm secrets ls --json` printed.
///
/// # Errors
/// Output that is not the listing.
pub fn parse_listing(out: &[u8]) -> Result<Vec<Entry>, String> {
    serde_json::from_slice::<Listing>(out)
        .map(|listing| listing.entries)
        .map_err(|e| format!("`aterm secrets ls --json` printed no listing ({e})"))
}

/// THE REGISTRY'S ENTRIES, read by `program secrets ls --json`.
///
/// # Errors
/// The program did not run, failed, outlived [`LS_TIMEOUT`] or printed no listing.
pub async fn registry(program: &Path) -> Result<Vec<Entry>, String> {
    let out = run(program, &["secrets", "ls", "--json"], LS_TIMEOUT).await?;
    parse_listing(&out)
}

/// FILE A NEED for `want` by hand: `program secrets need <want>`.
///
/// # Errors
/// The program did not run, failed or outlived [`NEED_TIMEOUT`].
pub async fn need(program: &Path, want: &str) -> Result<(), String> {
    run(program, &["secrets", "need", want], NEED_TIMEOUT)
        .await
        .map(|_| ())
}

async fn run(program: &Path, args: &[&str], limit: Duration) -> Result<Vec<u8>, String> {
    let shown = format!("`aterm {}`", args.join(" "));
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("{shown} did not start ({e})"))?;
    let out = tokio::time::timeout(limit, child.wait_with_output())
        .await
        .map_err(|_| format!("{shown} took over {}s", limit.as_secs()))?
        .map_err(|e| format!("{shown} failed ({e})"))?;
    if out.status.success() {
        return Ok(out.stdout);
    }
    let first = String::from_utf8_lossy(&out.stderr)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string();
    let mut why: String = first.chars().filter(|c| !c.is_control()).collect();
    if why.len() > WHY_BYTES {
        let mut cut = WHY_BYTES;
        while !why.is_char_boundary(cut) {
            cut -= 1;
        }
        why.truncate(cut);
        why.push('…');
    }
    let code = out
        .status
        .code()
        .map_or_else(|| "a signal".to_string(), |c| format!("exit {c}"));
    Err(if why.is_empty() {
        format!("{shown}: {code}")
    } else {
        format!("{shown}: {code}: {why}")
    })
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
