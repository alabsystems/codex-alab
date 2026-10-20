// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The switch every ALab behavior keys off, and ALab's packaged defaults.
//!
//! codex-alab changes upstream behavior only in the `codex` binary: its `main` calls
//! [`activate`] before anything else (`cli/src/main.rs`, one `ALAB(core):` line), and every
//! ALab hook asks [`active`]. Upstream's unit tests never run that `main`, in any crate, so
//! they keep testing upstream's behavior; `alab/check.sh` gates on them. ALab's own tests
//! activate the switch themselves.

use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Turn ALab's behavior on for this process. The `codex` binary calls it first thing.
pub fn activate() {
    ACTIVE.store(true, Ordering::SeqCst);
}

/// Whether this process is the codex-alab binary (or an ALab test that said so).
pub fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

pub mod boxes;
pub mod capacity;
pub mod mail;
pub mod session;

const ALAB_DEFAULTS: &str = include_str!("../defaults.toml");
/// ALab's defaults that hold only inside aterm, merged over [`ALAB_DEFAULTS`] there.
pub const ATERM_DEFAULTS: &str = include_str!("../aterm-defaults.toml");

/// The packaged-defaults layer (`config/src/loader/mod.rs`): upstream's
/// `config/defaults.toml`, with ALab's `alab-core/defaults.toml` merged over it when
/// [`active`], and `alab-core/aterm-defaults.toml` over that when also [`inside_aterm`]. It
/// stays the lowest config layer, so every user, profile, project or `-c`
/// value still wins. Should the merge ever fail (a build error ALab's tests catch), upstream's
/// defaults stand rather than none.
pub fn packaged_defaults(upstream: &'static str) -> &'static str {
    static MERGED: OnceLock<Option<String>> = OnceLock::new();
    if !active() {
        return upstream;
    }
    MERGED
        .get_or_init(|| alab_defaults(upstream, inside_aterm()))
        .as_deref()
        .unwrap_or(upstream)
}

/// `[tui] alab_progress`: codex-alab's OSC 9;4 progress bar inside aterm
/// (`alab-tui/src/progress.rs`), `true` in [`ATERM_DEFAULTS`].
pub const PROGRESS_KEY: &str = "alab_progress";

/// Whether `[tui] <key>` is one of ALab's own keys, which codex-alab reads from the config
/// layers itself (upstream's `Tui` has no such field), in the codex binary only. Upstream's
/// strict validation asks it (`config/src/strict_config.rs`, one `ALAB(progress)` line), so
/// codex-alab's `--strict-config` takes a person's `alab_progress = false`; vendor codex's
/// still refuses it, as it refuses any key it does not know.
pub fn own_tui_key(key: &str) -> bool {
    active() && key == PROGRESS_KEY
}

/// Whether this process runs in an aterm tab (`TERM_PROGRAM=aterm`), the same test the pet
/// drawing path makes: the aterm-only defaults hold exactly where aterm draws them.
pub fn inside_aterm() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|program| is_aterm(&program))
}

fn is_aterm(term_program: &str) -> bool {
    term_program.trim().eq_ignore_ascii_case("aterm")
}

/// Upstream's defaults with ALab's merged over them, and aterm's over those `in_aterm`.
fn alab_defaults(upstream: &str, in_aterm: bool) -> Option<String> {
    let merged = merge_defaults(upstream, ALAB_DEFAULTS)?;
    if in_aterm {
        merge_defaults(&merged, ATERM_DEFAULTS)
    } else {
        Some(merged)
    }
}

/// `overlay` merged over `base`, table by table; any other value is replaced.
fn merge_defaults(base: &str, overlay: &str) -> Option<String> {
    let mut base: toml::Table = toml::from_str(base).ok()?;
    let overlay: toml::Table = toml::from_str(overlay).ok()?;
    merge_tables(&mut base, overlay);
    toml::to_string(&base).ok()
}

fn merge_tables(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base)), toml::Value::Table(overlay)) => {
                merge_tables(base, overlay);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// How often an app-server re-downloads the model catalog when [`active`]: hourly, not
/// every 4.5 minutes per tab (each one ~420 KB, unconditional); a stale cache is refreshed
/// at startup anyway, and new models also arrive with each response.
pub const MODELS_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Whether diagnostics are captured at TRACE level into the in-memory /feedback ring (4 MiB
/// per process, formatting every event) and the SQLite log database. Not when [`active`]:
/// /feedback is off, and the database keeps INFO and up.
pub fn trace_diagnostics() -> bool {
    !active()
}

/// Whether `feedback_tags` events go into the SQLite log database (`state/src/log_db.rs`): the
/// key/value tags a /feedback upload attaches (the whole feature map, the model, request ids;
/// about 4 KB each, one per model request). Not when [`active`]: /feedback is off, and they were
/// the largest share of a real 18.6 MB log database (29%), crowding out its budget. They are
/// also the log's only record of each model request's and response's id and of a 401
/// recovery's request id, cf-ray and error, which go with them; the filter is built without
/// config, so `[feedback] enabled = true` does not bring them back. The in-memory tags a
/// /feedback upload reads are not filtered here.
pub fn feedback_tags_logged() -> bool {
    !active()
}

/// Whether each turn runs `git status` and sends the repository's HEAD, remote URLs and dirty
/// flag to OpenAI in the request metadata (`core/src/turn_metadata.rs`). Not when [`active`]:
/// the model's prompt is unaffected; the metadata only reached OpenAI's servers.
pub fn turn_git_metadata() -> bool {
    !active()
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;
#[cfg(test)]
mod tests;
