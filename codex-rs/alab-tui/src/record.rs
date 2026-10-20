// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session record, `$CODEX_HOME/alab/sessions/<pid>.json`, schema v1 (CONTRACT §4):
//! what aterm reads while the process's identity is live, and the crash witness after.
//! Directory 0700, file 0600, written as `<pid>.json.<nonce>.tmp` then renamed.

use codex_alab_core::session::ProcessIdentity;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub const SCHEMA_VERSION: u32 = 1;

/// The record's directory under a `CODEX_HOME`.
pub fn sessions_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("alab").join("sessions")
}

/// The record's path for `pid`.
pub fn path_for(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{pid}.json"))
}

/// `identity` as the record spells it (upstream's untagged camelCase form).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum Identity {
    Linux {
        boot_id: String,
        start_ticks: u64,
    },
    MacOs {
        boot_id: String,
        unique_id: u64,
        start_seconds: u64,
        start_microseconds: u64,
    },
}

impl From<&ProcessIdentity> for Identity {
    fn from(identity: &ProcessIdentity) -> Self {
        match identity.clone() {
            ProcessIdentity::Linux {
                boot_id,
                start_ticks,
            } => Self::Linux {
                boot_id,
                start_ticks,
            },
            ProcessIdentity::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            } => Self::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            },
        }
    }
}

impl From<Identity> for ProcessIdentity {
    fn from(identity: Identity) -> Self {
        match identity {
            Identity::Linux {
                boot_id,
                start_ticks,
            } => Self::Linux {
                boot_id,
                start_ticks,
            },
            Identity::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            } => Self::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Busy,
    Idle,
    Waiting,
    Upgrading,
    Exited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Working,
    Idle,
    Blocked,
    Done,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Autopilot {
    Off,
    Passive,
    Active,
}

/// Schema v1, field for field and in CONTRACT §4's order. `null` where unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub v: u32,
    pub pid: u32,
    pub proc_start: Option<String>,
    pub identity: Option<Identity>,
    pub session_id: Option<String>,
    pub displayed_thread_id: Option<String>,
    pub cwd: Option<String>,
    pub version: String,
    pub exe: Option<String>,
    pub launch_argv0: Option<String>,
    pub exec_generation: u64,
    pub resume_argv: Option<Vec<String>>,
    pub env: BTreeMap<String, String>,
    pub status: Status,
    pub exit: Option<String>,
    pub state: Option<State>,
    pub blocked_kind: Option<BlockedKind>,
    pub draft_empty: bool,
    pub last_human_input_at: Option<u64>,
    pub autopilot: Autopilot,
    pub claim: Option<String>,
    pub status_updated_at: u64,
    pub updated_at: u64,
    pub kind: String,
    pub entrypoint: String,
    /// How many relaunch carry-ons in a row led here with no person's turn and no done since
    /// (CONTRACT §5 carry-on). Added after v1's first fields: a record without it reads 0, and
    /// aterm ignores it.
    #[serde(default)]
    pub carried_on: u32,
}

impl Record {
    /// Equal but for `updatedAt` (a rewrite with nothing new is skipped).
    pub fn same_content(&self, other: &Self) -> bool {
        let mut other = other.clone();
        other.updated_at = self.updated_at;
        *self == other
    }
}

/// Create the record directory (0700, parents too) and tighten it if it already exists.
pub fn ensure_dir(dir: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Write `record` atomically: a 0600 temporary file beside it, synced, then renamed.
pub fn write_atomic(dir: &Path, record: &Record) -> io::Result<()> {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    let target = path_for(dir, record.pid);
    let nonce = format!(
        "{:x}{:x}",
        codex_alab_core::session::now_ms(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let temporary = dir.join(format!("{}.json.{nonce}.tmp", record.pid));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Read and parse a record (`None` if it is missing or not a v1 record).
pub fn read(path: &Path) -> Option<Record> {
    let bytes = fs::read(path).ok()?;
    let record: Record = serde_json::from_slice(&bytes).ok()?;
    (record.v == SCHEMA_VERSION).then_some(record)
}

/// Remove `pid`'s record; a missing one is fine.
pub fn remove(dir: &Path, pid: u32) -> io::Result<()> {
    match fs::remove_file(path_for(dir, pid)) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The pid a record file (`<pid>.json`) or its temporary (`<pid>.json.<nonce>.tmp`) belongs to.
fn owner_pid(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".json").or_else(|| {
        name.strip_suffix(".tmp")?
            .split_once(".json.")
            .map(|(pid, _)| pid)
    })?;
    if stem.is_empty() || !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

/// Remove the records (and stray temporaries) of dead identities, any pid but `own_pid`,
/// last written before `before_ms` (this process's start). A record whose identity is live
/// is kept, and so is one written after this process started. Returns how many it removed.
pub fn sweep(
    dir: &Path,
    own_pid: u32,
    before_ms: u64,
    is_live: impl Fn(u32, Option<&ProcessIdentity>) -> bool,
) -> usize {
    sweep_with(dir, own_pid, before_ms, is_live, |_| Sweep::Take, |_, _| {})
}

/// What the start-up sweep does with one dead record ([`sweep_with`], [`judge`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sweep {
    /// This process's own predecessor: removed, and handed on (its crash witness).
    Take,
    /// A relaunch another process may still be owed: left for that process's start.
    Leave,
    /// Nobody's any more: removed.
    Remove,
}

/// The start-up sweep's verdict on one dead record, for a process started at `started_ms`
/// with `own_argv`. aterm relaunches each tab by typing its record's `resumeArgv`, so the
/// record whose `resumeArgv` is exactly this process's argv is this process's own
/// predecessor; one naming another line is another tab's (when aterm relaunches several
/// codex-alab tabs together, each start must find its own, not the first start take them
/// all), and stands until its own relaunch takes it or until it is older than `horizon`
/// (no relaunch is owed from it any more: [`codex_alab_core::session::RELAUNCH_HORIZON`]).
/// A record with no `resumeArgv` names no relaunch, and neither does one whose end aterm reads
/// as orderly ([`relaunch_owed`]): another tab's such record is removed at once, not kept for a
/// day (it would only keep its package from the pruner). This process's own is taken whatever
/// it says (its witness is the carry-on's to judge).
pub fn judge(record: &Record, own_argv: &[OsString], started_ms: u64, horizon: u64) -> Sweep {
    let Some(argv) = &record.resume_argv else {
        return Sweep::Remove;
    };
    let own = argv.len() == own_argv.len()
        && argv
            .iter()
            .zip(own_argv)
            .all(|(want, got)| got.to_str() == Some(want.as_str()));
    if own {
        Sweep::Take
    } else if relaunch_owed(record) && started_ms.saturating_sub(record.updated_at) <= horizon {
        Sweep::Leave
    } else {
        Sweep::Remove
    }
}

/// Whether aterm may still type a relaunch line for `record` (CONTRACT §4): no orderly end
/// is written, so the crash rule may relaunch it: `exit` absent (a panic, SIGKILL, power
/// loss), `signal:15` (aterm's stall or memory remedy, which may fire at an idle point too) or
/// `signal:1` (aterm's restore after its own crash relaunches it), and `status` not `exited`.
/// A `fatal` exit owes none: the crash rule reads it as orderly, and the Codex lane that read
/// its status 1 as a crash and relaunched it under its back-off is retired (aterm
/// 83bd43c206; CONTRACT §4 as amended 2026-10-17), so its record is removed at the next start
/// like any orderly end's (the review of 363789134 kept it a day for that lane). A SIGINT (a
/// person's) or any other signal aterm never sends is orderly: no relaunch is owed. Whether
/// that relaunch would also carry on (a busy predecessor, no chain) does not matter here:
/// aterm types the line either way, and the pruner must keep the package it names.
pub fn relaunch_owed(record: &Record) -> bool {
    record.status != Status::Exited
        && matches!(
            record.exit.as_deref(),
            None | Some("signal:15" | "signal:1")
        )
}

/// [`sweep`], asking `verdict` what to do with each dead record that could be read
/// ([`judge`]; one that cannot be read is removed), and handing each one it took to
/// `on_taken` (with its pid). Returns how many it removed.
pub fn sweep_with(
    dir: &Path,
    own_pid: u32,
    before_ms: u64,
    is_live: impl Fn(u32, Option<&ProcessIdentity>) -> bool,
    verdict: impl Fn(&Record) -> Sweep,
    mut on_taken: impl FnMut(u32, &Record),
) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(owner_pid) else {
            continue;
        };
        if pid == own_pid {
            continue;
        }
        let path = entry.path();
        let record = read(&path);
        let written_ms = record.as_ref().map(|record| record.updated_at).or_else(|| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            let elapsed = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
            u64::try_from(elapsed.as_millis()).ok()
        });
        if written_ms.is_none_or(|written| written >= before_ms) {
            continue;
        }
        let identity = record
            .as_ref()
            .and_then(|record| record.identity.clone())
            .map(ProcessIdentity::from);
        if is_live(pid, identity.as_ref()) {
            continue;
        }
        let verdict = record.as_ref().map_or(Sweep::Remove, &verdict);
        if verdict == Sweep::Leave {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            removed += 1;
            if let (Sweep::Take, Some(record)) = (verdict, &record) {
                on_taken(pid, record);
            }
        }
    }
    removed
}
