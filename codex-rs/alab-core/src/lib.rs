// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The switch every ALab behavior keys off, and ALab's packaged defaults.
//!
//! codex-alab changes upstream behavior only in the `codex` binary: its `main` calls
//! [`activate`] at entry before request dispatch (`cli/src/main.rs`), and every
//! interface hook asks [`active`]. Explicit credential custody and internal error-message
//! provenance use [`is_alab_binary`] across live vanilla/enhanced changes. These product
//! invariants remain available with the enhanced interface off. Upstream's unit tests never run that `main`, in any crate, so
//! they keep testing upstream's behavior; `alab/check.sh` gates on them. ALab's own tests
//! activate the switch themselves.

mod shell_output_tail;
pub use shell_output_tail::MAX_SHELL_OUTPUT_TAIL_BYTES;
pub use shell_output_tail::SHELL_OUTPUT_PREFIX_BYTES;
pub use shell_output_tail::ShellOutputTail;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use std::cell::Cell;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

static ACTIVE: AtomicBool = AtomicBool::new(false);
// Low bit: enhanced. Upper bits: exact live-mode generation. `u64::MAX`
// permanently fails closed rather than repeating a generation.
static MODE_STATE: AtomicU64 = AtomicU64::new(1);
static LIVE_MODE: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
static MODEL_SOURCE_FENCE: std::sync::Mutex<()> = std::sync::Mutex::new(());
static EXPERIENCE: OnceLock<tokio::sync::watch::Sender<bool>> = OnceLock::new();
type ModeDurabilityStart = Box<dyn Fn(&Path) -> bool + Send + Sync>;
static MODE_DURABILITY_START: OnceLock<ModeDurabilityStart> = OnceLock::new();

thread_local! {
    /// A record flush rebuilds its snapshot synchronously and may rediscover the
    /// same carried source while the outer durable publication owns the fence.
    /// Keep that same-thread recursion inside the outer critical section.
    static MODEL_SOURCE_FENCE_DEPTH: Cell<u32> = const { Cell::new(0) };
}

struct ModelSourceFenceDepth;

impl ModelSourceFenceDepth {
    fn enter() -> Self {
        MODEL_SOURCE_FENCE_DEPTH.with(|depth| {
            let Some(next_depth) = depth.get().checked_add(1) else {
                panic!("model source fence recursion overflow");
            };
            depth.set(next_depth);
        });
        Self
    }
}

impl Drop for ModelSourceFenceDepth {
    fn drop(&mut self) {
        MODEL_SOURCE_FENCE_DEPTH.with(|depth| {
            let Some(next_depth) = depth.get().checked_sub(1) else {
                panic!("model source fence recursion underflow");
            };
            depth.set(next_depth);
        });
    }
}

/// Observe the effective experience without polling or retaining a startup choice.
pub fn experience_changes() -> tokio::sync::watch::Receiver<bool> {
    let _live = LIVE_MODE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    EXPERIENCE
        .get_or_init(|| tokio::sync::watch::channel(active()).0)
        .subscribe()
}

fn publish_experience() {
    if let Some(sender) = EXPERIENCE.get() {
        sender.send_replace(active());
    }
}

fn change_mode(enabled: bool) -> bool {
    loop {
        let current = MODE_STATE.load(Ordering::SeqCst);
        if current == u64::MAX || current & 1 == u64::from(enabled) {
            return false;
        }
        let generation = current >> 1;
        let Some(generation) = generation.checked_add(1) else {
            MODE_STATE.store(u64::MAX, Ordering::SeqCst);
            return true;
        };
        let Some(next) = generation
            .checked_mul(2)
            .and_then(|value| value.checked_add(u64::from(enabled)))
            .filter(|next| *next != u64::MAX)
        else {
            MODE_STATE.store(u64::MAX, Ordering::SeqCst);
            return true;
        };
        if MODE_STATE
            .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return true;
        }
    }
}

/// Durably clear process-wide provenance before the atomic mode publication.
/// No new-generation runtime can mint an old source, and a crash after either
/// live boundary cannot recover one from the predecessor session record.
fn change_mode_with_source_fence(enabled: bool) -> bool {
    with_model_source_lock(|| {
        let state = MODE_STATE.load(Ordering::SeqCst);
        if state == u64::MAX || state & 1 == u64::from(enabled) {
            return false;
        }
        let shared = session::shared();
        shared.retire_all_model_goals_for_mode();
        // Tests and non-final library consumers have no captured launch and can
        // own no session record. The final binary captures its identity before
        // config resolution, so every production transition requires the
        // installed writer/no-record witness.
        if (shared.launch().is_some() || shared.record_flush_installed())
            && !shared.flush_record_required()
        {
            return false;
        }
        change_mode(enabled)
    })
}

fn lock_model_source_fence() -> std::sync::MutexGuard<'static, ()> {
    MODEL_SOURCE_FENCE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Serialize every process-wide model-source mutation. A durable record flush
/// rebuilds its snapshot on the same thread, so recursive discovery shares the
/// outer fence instead of deadlocking on the non-reentrant mutex.
pub fn with_model_source_lock<T>(callback: impl FnOnce() -> T) -> T {
    let nested = MODEL_SOURCE_FENCE_DEPTH.with(|depth| depth.get() != 0);
    let _sources = (!nested).then(lock_model_source_fence);
    let _depth = ModelSourceFenceDepth::enter();
    callback()
}

/// Whether this thread is inside the process-wide model-source critical
/// section. This is diagnostic only; it grants no authority after the caller
/// returns and must not be used instead of [`with_model_source_lock`].
pub fn model_source_lock_owned_by_current_thread() -> bool {
    MODEL_SOURCE_FENCE_DEPTH.with(|depth| depth.get() != 0)
}

/// Run a durable model-source publication only while its exact Enhanced
/// generation remains current. Mode changes and every persisted source mutation
/// are excluded for the whole callback.
pub fn with_model_source_fence<T>(
    expected_mode_epoch: u64,
    callback: impl FnOnce() -> T,
) -> Option<T> {
    let _live = LIVE_MODE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    with_model_source_lock(|| (mode_epoch() == Some(expected_mode_epoch)).then(callback))
}

/// Exact enhanced-mode generation. Vanilla and exhaustion expose no recovery authority.
pub fn mode_epoch() -> Option<u64> {
    let state = MODE_STATE.load(Ordering::SeqCst);
    (state != u64::MAX && state & 1 == 1).then_some(state >> 1)
}

/// Turn ALab's behavior on for this process. The `codex` binary calls it at entry.
pub fn activate() {
    ACTIVE.store(true, Ordering::SeqCst);
    publish_experience();
}

/// Whether this process is the codex-alab binary (or an ALab test that said so).
pub fn active() -> bool {
    is_alab_binary() && mode_epoch().is_some()
}

/// Product identity remains available for the mode command in vanilla mode.
pub fn is_alab_binary() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// All behavioral hooks share the same vanilla/enhanced choice.
pub fn set_enhanced(enabled: bool) -> bool {
    let mut live = LIVE_MODE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *live = true;
    let changed = change_mode_with_source_fence(enabled);
    let current = mode_epoch().is_some();
    drop(live);
    if changed {
        publish_experience();
        session::shared().mark_changed();
    }
    current
}

/// Install the final binary's session-record initializer. Config resolution
/// calls it before an initial Vanilla boundary, while it already knows the
/// effective Codex home.
pub fn install_mode_durability_start(
    start: impl Fn(&Path) -> bool + Send + Sync + 'static,
) -> bool {
    MODE_DURABILITY_START.set(Box::new(start)).is_ok()
}

/// Establish either the session writer or an explicit no-record witness before
/// a mode transition can retire persisted model provenance.
pub fn prepare_mode_durability(home: &Path) -> bool {
    MODE_DURABILITY_START.get().is_some_and(|start| start(home))
}

/// Whether a person's live choice has superseded config-layer mode selection.
pub fn mode_choice_is_live() -> bool {
    *LIVE_MODE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Config reloads may initialize the mode but never overwrite a person's live choice.
/// Returns `false` only when a required durable transition could not commit.
pub fn configure_enhanced(enabled: bool) -> bool {
    let live = LIVE_MODE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *live {
        return true;
    }
    let already_selected = mode_epoch().is_some() == enabled;
    let changed = !already_selected && change_mode_with_source_fence(enabled);
    let committed = already_selected || changed;
    drop(live);
    if changed {
        publish_experience();
        session::shared().mark_changed();
    }
    committed
}

pub mod account_handoff;
pub mod assignment;
pub mod assignment_service;
pub mod auth_lock;
pub mod bound_picker;
pub mod boxes;
pub mod capacity;
pub mod compaction;
pub mod context_host;
pub mod diagnostic_id;
pub mod exec_wall;
pub mod execution;
pub mod goal_chronology_fence;
pub mod goal_clock;
pub mod goal_dispatch;
pub mod goal_persistence;
pub mod goal_recovery;
pub mod goal_stops;
pub mod home;
pub mod https_fallback;
pub mod log_health;
pub mod mail;
pub mod managed_auth;
pub mod manager_catalog;
pub mod manager_source_epoch;
pub mod model_goal_invocation;
pub mod model_goal_stops;
pub mod native_backend_owner;
pub mod native_backend_startup;
#[cfg(unix)]
pub mod native_fd_custody;
#[cfg(unix)]
pub mod native_owned_control;
pub mod native_preload;
pub mod native_reader_retirement;
pub mod native_root;
pub mod native_source;
pub mod native_source_launch;
pub mod native_source_lifetime;
pub mod native_source_retirement;
pub mod native_source_use;
pub mod native_target;
pub mod quota;
pub mod recovery_clock;
pub mod renewal;
pub mod request_error;
pub mod retained_future;
pub mod retained_goal_gate;
pub mod retained_side_cleanup;
pub mod runtime_upgrade;
pub mod session;
#[cfg(unix)]
pub mod shell_log_store;
pub mod upgrade_barrier;
pub mod upgrade_completion;
pub mod upgrade_startup_hold;
pub mod version;
pub mod wake;
pub mod wall_kind;
pub mod workspace_routing;

const ALAB_DEFAULTS: &str = include_str!("../defaults.toml");
const UPSTREAM_DEFAULTS: &str = include_str!("../../config/defaults.toml");
/// ALab's defaults that hold only inside aterm, merged over [`ALAB_DEFAULTS`] there.
pub const ATERM_DEFAULTS: &str = include_str!("../aterm-defaults.toml");

/// The compiled packaged-defaults layer without a selected built-in profile.
/// This diagnostic reads no user, project, managed or cloud configuration and does
/// not resolve authentication. Its only process inputs are [`active`] and [`inside_aterm`].
pub fn compiled_packaged_defaults() -> &'static str {
    packaged_defaults(UPSTREAM_DEFAULTS)
}

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

/// ALab's built-in Profile V2 defaults, beneath the user's selected profile file.
pub fn selected_packaged_profile(profile: &str) -> Option<toml::Table> {
    if active() {
        packaged_profile_defaults(profile)
    } else {
        None
    }
}

fn packaged_profile_defaults(profile: &str) -> Option<toml::Table> {
    let defaults: toml::Table = toml::from_str(ALAB_DEFAULTS).ok()?;
    defaults.get("profiles")?.get(profile)?.as_table().cloned()
}

/// `[tui] alab_progress`: codex-alab's OSC 9;4 progress bar inside aterm
/// (`alab-tui/src/progress.rs`), `true` in [`ATERM_DEFAULTS`].
pub const PROGRESS_KEY: &str = "alab_progress";

/// The Vanilla/Enhanced experience choice. Cruise/Land/Off is a separate preference.
pub const INTERFACE_KEY: &str = "alab_interface";

/// Whether `[tui] <key>` is one of ALab's own keys, which codex-alab reads from the config
/// layers itself (upstream's `Tui` has no such field), in the codex binary only. Upstream's
/// strict validation asks it (`config/src/strict_config.rs`, one `ALAB(progress)` line), so
/// codex-alab's `--strict-config` takes a person's `alab_progress = false`; vendor codex's
/// still refuses it, as it refuses any key it does not know.
pub fn own_tui_key(key: &str) -> bool {
    is_alab_binary() && matches!(key, PROGRESS_KEY | INTERFACE_KEY)
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

/// How often an app-server polls the model catalog when [`active`]: hourly, not every 4.5
/// minutes per tab. A poll downloads the catalog (~420 KB) only when the cache the tabs share
/// is older than [`models_cache_ttl`], so together they download it about once an hour; new
/// models also arrive with each response.
pub const MODELS_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// How long a downloaded model catalog is trusted when [`active`]: [`MODELS_REFRESH_INTERVAL`],
/// not upstream's five minutes, after which nearly every launch downloaded it again. Each
/// response's models etag still refetches a changed catalog at once and renews an unchanged one.
pub fn models_cache_ttl(upstream: Duration) -> Duration {
    if active() {
        MODELS_REFRESH_INTERVAL
    } else {
        upstream
    }
}

/// Whether the app-server's skills watcher watches skills root `root` (`ALAB(skills-watch)` in
/// `app-server/src/skills_watcher.rs`). A missing root is watched through its nearest existing
/// ancestor, and on macOS that is an FSEvents stream carrying every file event beneath the
/// ancestor (notify drops the deeper ones in its callback). For `~/.agents/skills`, absent on most
/// Macs, the ancestor is the home directory: about 18,000 events a minute while a build runs,
/// costing each watching process about 0.04 s of CPU a minute, as much as codex-alab at rest
/// (measured 2026-10-23). Not when [`active`] and the nearest existing ancestor is the home
/// directory or above it. A root created there mid-session then sends no `SkillsChanged`: the
/// skills list reloads at the next thread start (`/new`, resume, fork), skills-cache clear (a
/// config write, the manage-skills popup closing) or launch, and after `/import`, which can
/// create `~/.agents/skills` itself (the `ALAB(skills-watch)` hook in
/// `tui/src/app/app_server_events.rs`). The model's next turn rescans the roots either way.
pub fn watches_skill_root(root: &Path) -> bool {
    watches_skill_root_when(active(), root, std::env::var_os("HOME").map(PathBuf::from))
}

fn watches_skill_root_when(active: bool, root: &Path, home: Option<PathBuf>) -> bool {
    if !active || root.exists() {
        return true;
    }
    let (Some(home), Some(ancestor)) = (home, root.ancestors().skip(1).find(|path| path.is_dir()))
    else {
        return true;
    };
    !home.starts_with(ancestor)
}

/// Whether diagnostics are captured at TRACE level into the in-memory /feedback ring (4 MiB
/// per process, formatting every event) and the SQLite log database. Not when [`active`]:
/// /feedback is off, and the database keeps INFO and up.
pub fn trace_diagnostics() -> bool {
    !active()
}

/// Whether `codex_http_client::custom_ca`'s INFO rows go into the SQLite log database: which
/// trust store each HTTP client was built with. Each client built logs one, and a usage read
/// builds one, so with no CA override set they were most of a long codex-alab session's rows
/// (1,000 of 1,001 over 24.6 h, 2026-10-23), each saying only that no override was selected.
/// Not when [`active`] unless `CODEX_CA_CERTIFICATE` or `SSL_CERT_FILE` is set (non-empty):
/// then they say which variable won, the bundle's path and its certificate count, which is what
/// a person behind a TLS-intercepting proxy needs when a request fails. Its failures log at
/// WARN either way.
pub fn ca_choice_logged() -> bool {
    ca_choice_logged_when(active(), |name| std::env::var_os(name))
}

fn ca_choice_logged_when(active: bool, var: impl Fn(&str) -> Option<OsString>) -> bool {
    !active
        || ["CODEX_CA_CERTIFICATE", "SSL_CERT_FILE"]
            .into_iter()
            .any(|name| var(name).is_some_and(|value| !value.is_empty()))
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
mod kimi_tests;
#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;
#[cfg(test)]
mod tests;

#[cfg(unix)]
mod native_root_io;

#[cfg(unix)]
pub mod native_root_command;

#[cfg(unix)]
pub mod native_root_child;

#[cfg(unix)]
pub mod native_incoming;

pub mod connection_resource; // ALAB(owned-local): opaque accepted-resource custody, never permission.
