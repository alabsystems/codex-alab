// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The live upgrade by re-exec (CONTRACT §5 row "live upgrade", tui-life §4): the process
//! half. The autopilot decides when (`codex-rs/alab-autopilot`, `upgrade.rs`); this says
//! whether there is anything to move onto, builds the exec, asks the TUI to leave like
//! `/exit`, and execs once the teardown is done.
//!
//! - **Trigger** ([`launch_names_other_build`]): the build the launch path (`launchArgv0`,
//!   atpkg's twin) names NOW is not the file this image was started from. The launch path
//!   is followed through symlinks, and an atpkg shim (`#!/bin/sh` whose last line is
//!   `exec '<abs>' "$@"`) names the build it execs. Builds compare by file identity
//!   (device, inode), never by version strings (CONTRACT §7). Anything unreadable names no
//!   other build: no upgrade.
//! - **The exec** ([`PreparedExec`]): `execve(<admitted physical image>, resumeArgv, <launch environment>)`
//!   from the launch cwd, `resumeArgv` rebuilt exactly as the session record's is (the
//!   primary thread's rollout must exist). The launch environment is the one captured first
//!   thing in `main`, before arg0 rewrote `PATH` and `.env` was merged; no variable is
//!   added (the new image learns `execGeneration` from its own pid's record).
//!   Native custody stays held through exec and is adopted by the next image's startup hook.
//! - **Leave** ([`request`]): `exec_pending`, then the TUI's own `/exit`.
//! - **Exec** ([`exec_after_teardown`], from `life::at_exit` once the record was rewritten
//!   `status:"upgrading"`): MCP and other children get [`CHILDREN_GRACE`], then the exec
//!   runs anyway; signal dispositions and the mask are reset; a failed exec is `_exit(1)`
//!   with the record kept, so aterm's crash rule relaunches the session.

use crate::resume_argv;
use crate::session::rollout_materialized;
use crate::upgrade_admission::Received;
use crate::upgrade_exec_custody::ExecCustody;
use codex_alab_core::session::Launch;
use codex_alab_core::session::Shared;
use std::ffi::CString;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

#[cfg(test)]
#[path = "upgrade_prepared_tests.rs"]
mod prepared_tests;

/// Native execution resources for owned test fixtures. This backend is absent
/// from normal product builds and performs no signed-package admission.
#[cfg(feature = "test-support")]
pub mod test_support {
    use super::*;
    use std::fs::File;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    /// The cached bit that controls the native header's update indicator.
    pub fn update_is_pending() -> bool {
        super::pending()
    }

    struct Preparation {
        shared: &'static Shared,
        count: AtomicUsize,
    }

    static PREPARATION_FIXTURE: Mutex<Option<Arc<Preparation>>> = Mutex::new(None);

    /// Substitute only package admission for one exact fixture session.
    /// The production build contains neither this registration nor its reader.
    pub struct PreparationGuard(Arc<Preparation>);

    impl PreparationGuard {
        pub fn preparations(&self) -> usize {
            self.0.count.load(Ordering::SeqCst)
        }
    }

    impl Drop for PreparationGuard {
        fn drop(&mut self) {
            let mut fixture = PREPARATION_FIXTURE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if fixture
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &self.0))
            {
                *fixture = None;
                let execution = self.0.shared.take_upgrade_execution();
                drop(fixture);
                drop(execution);
            }
        }
    }

    pub fn scoped_preparation(shared: &'static Shared) -> Result<PreparationGuard, String> {
        let mut fixture = PREPARATION_FIXTURE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if fixture.is_some() {
            return Err("another fixture owns package preparation".into());
        }
        let preparation = Arc::new(Preparation {
            shared,
            count: AtomicUsize::new(0),
        });
        *fixture = Some(Arc::clone(&preparation));
        Ok(PreparationGuard(preparation))
    }

    pub(super) fn prepare_if_registered(shared: &Shared) -> Option<Result<PreparedExec, String>> {
        let preparation = PREPARATION_FIXTURE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|preparation| std::ptr::eq(preparation.shared, shared))
            .cloned()?;
        let prepared = prepare(preparation.shared);
        if prepared.is_ok() {
            preparation.count.fetch_add(1, Ordering::SeqCst);
        }
        Some(prepared)
    }

    pub struct Guard(pub &'static Shared);
    impl Drop for Guard {
        fn drop(&mut self) {
            drop(self.0.take_upgrade_execution());
        }
    }
    pub fn prepare(shared: &'static Shared) -> Result<PreparedExec, String> {
        let plan = exec_plan(shared)?;
        let image = named_build(&plan.path)
            .ok_or("fixture image missing")?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let root = image
            .parent()
            .and_then(Path::parent)
            .ok_or("fixture package root missing")?
            .to_path_buf();
        let (lease, lease_path) = tempfile::NamedTempFile::new_in(&root)
            .map_err(|error| error.to_string())?
            .keep()
            .map_err(|error| error.to_string())?;
        lease.try_lock().map_err(|error| error.to_string())?;
        let image_file = File::open(&image).map_err(|error| error.to_string())?;
        let i = image_file.metadata().map_err(|error| error.to_string())?;
        let l = lease.metadata().map_err(|error| error.to_string())?;
        let received = Received {
            receipt: crate::upgrade_admission::Receipt {
                protocol: 1,
                program: "codex".into(),
                build: 7,
                version: "0.0.0".into(),
                root,
                executable: image,
                tree_root: "a".repeat(64),
                device: i.dev(),
                inode: i.ino(),
                size: i.len(),
                modified_seconds: i.mtime(),
                modified_nanoseconds: i.mtime_nsec(),
                lease_path,
                lease_device: l.dev(),
                lease_inode: l.ino(),
            },
            lease,
            image: image_file,
        };
        PreparedExec::finish(plan, received)
    }

    #[cfg(test)]
    #[test]
    fn scoped_preparation_is_bound_and_cleared() {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let other = Shared::new();
        let guard = scoped_preparation(shared).expect("register isolated fixture");
        assert!(scoped_preparation(shared).is_err());
        assert!(prepare_if_registered(&other).is_none());
        assert!(prepare_if_registered(shared).is_some_and(|result| result.is_err()));
        assert_eq!(guard.preparations(), 0);
        assert!(shared.commit_upgrade_execution(Box::new(String::new), || true));
        drop(guard);
        assert!(prepare_if_registered(shared).is_none());
        assert!(shared.take_upgrade_execution().is_none());
    }
}

pub use codex_alab_core::session::UpgradeRequest;

/// Any native App/server event can change a loaded thread, goal or background task.
/// Invalidate an in-flight asynchronous scan before dispatching it.
pub fn native_event() {
    if crate::session::active() {
        codex_alab_core::session::shared().invalidate_upgrade();
    }
}

/// What the update monitor last established. Kept separately from activity and account
/// status: a pending update never replaces "Working" or hides a connection failure.
#[derive(Clone, Default)]
struct UpdateStatus {
    state: Option<String>,
    pending: bool,
    watch_error: Option<String>,
}

static UPDATE_STATUS: Mutex<UpdateStatus> = Mutex::new(UpdateStatus {
    state: None,
    pending: false,
    watch_error: None,
});

/// Publish an observed update state; this reports no release or installation authority.
pub fn update_state(message: impl Into<String>) {
    publish_update(message.into(), false);
}

/// An available update waiting for a safe handoff. This is a cached fact for rendering.
pub fn update_pending(message: impl Into<String>) {
    publish_update(message.into(), true);
}

fn publish_update(message: String, pending: bool) {
    let changed = {
        let mut status = UPDATE_STATUS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = status.pending != pending;
        status.state = Some(message);
        status.pending = pending;
        changed
    };
    if changed && crate::session::active() {
        codex_alab_core::session::shared().wake();
    }
}

pub(crate) fn pending() -> bool {
    UPDATE_STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .pending
}

/// Report degraded notification delivery while the independent minute check keeps running.
pub fn update_watch_error(message: impl Into<String>) {
    UPDATE_STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .watch_error = Some(message.into());
}

/// Read-only details for `/alab update`, available with either presentation setting.
/// The running version is compiled into this image; an installed package's version is
/// reported separately and only when its manifest names the resolved executable.
pub fn update_report(source: &str) -> String {
    let status = UPDATE_STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let target = codex_alab_core::session::shared()
        .launch()
        .and_then(launch_target);
    let report = status_report(
        crate::session::version(),
        target.as_ref().map(|target| target.summary.as_str()),
        target.as_ref().and_then(|target| target.source.as_deref()),
        &status,
    );
    let local = target
        .as_ref()
        .and_then(|target| target.local_report.as_deref())
        .unwrap_or("Local channel last report: unavailable");
    format!(
        "{report}\n{local}\n{}",
        source_details(
            source,
            codex_alab_core::version::running_version(env!("CARGO_PKG_VERSION")),
            codex_alab_core::version::upstream_release(),
            codex_alab_core::version::upstream_commit(),
        )
    )
}

struct LaunchTarget {
    summary: String,
    source: Option<String>,
    local_report: Option<String>,
}

fn launch_target(launch: &Launch) -> Option<LaunchTarget> {
    launch.upgrade_path().map(|argv0| match named_build(argv0) {
        Some(named) => {
            let package = package_details(&named);
            let version = package
                .as_ref()
                .map_or("version unavailable", |(_, version)| version.as_str());
            let source = package
                .as_ref()
                .and_then(|(root, version)| build_source(root, version));
            LaunchTarget {
                summary: format!("{version} ({})", named.display()),
                local_report: package.as_ref().and_then(|(root, version)| {
                    local_channel_report(root, version, source.as_deref()?)
                }),
                source,
            }
        }
        None => LaunchTarget {
            summary: "unavailable; the current session keeps running".to_owned(),
            source: None,
            local_report: None,
        },
    })
}

const CHANNEL_STATUS: &str = ".alab-local-update-status.json";

#[derive(serde::Deserialize)]
struct ChannelStatus {
    schema: u32,
    when: String,
    phase: String,
    last_phase: String,
    complete: bool,
    exit: Option<u8>,
    initial_package: String,
    initial_source: String,
    initial_version: String,
    target_source: Option<String>,
}

fn source_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn report_text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && value.bytes().all(|byte| byte.is_ascii_graphic())
}

/// The producer writes UTC ISO timestamps, optionally with subsecond digits.
/// Their age remains visible; no timestamp establishes current process activity.
fn report_time(value: &str) -> bool {
    let Some(utc) = value.strip_suffix("+00:00") else {
        return false;
    };
    let (seconds, fraction) = utc
        .split_once('.')
        .map_or((utc, None), |(seconds, fraction)| (seconds, Some(fraction)));
    seconds.len() == 19
        && seconds
            .bytes()
            .enumerate()
            .all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                10 => byte == b'T',
                13 | 16 => byte == b':',
                _ => byte.is_ascii_digit(),
            })
        && fraction.is_none_or(|digits| {
            !digits.is_empty() && digits.len() <= 9 && digits.bytes().all(|b| b.is_ascii_digit())
        })
}

fn work_phase(phase: &str) -> bool {
    matches!(phase, "fetching" | "checking" | "building" | "selecting")
}

impl ChannelStatus {
    fn valid(&self) -> bool {
        let package = Path::new(&self.initial_package);
        self.schema == 1
            && report_time(&self.when)
            && report_text(&self.initial_package, 255)
            && !self.initial_package.starts_with('.')
            && !self.initial_package.contains('/')
            && !self.initial_package.contains('\\')
            && matches!(
                package.components().next(),
                Some(std::path::Component::Normal(_))
            )
            && source_id(&self.initial_source)
            && report_text(&self.initial_version, 128)
            && self.target_source.as_deref().is_none_or(source_id)
            && work_phase(&self.last_phase)
            && if self.complete {
                self.phase == "completed" && self.exit.is_some()
            } else {
                self.phase == self.last_phase && self.exit.is_none()
            }
    }
}

/// Source provenance beside a manifest that names the resolved executable. A channel
/// journal's target never substitutes for an absent or mismatched build record.
fn build_source(root: &Path, version: &str) -> Option<String> {
    let build: serde_json::Value = serde_json::from_str(&crate::life::read_managed_text(
        &root.join("alab-build.json"),
    )?)
    .ok()?;
    let selected_source = build.get("source")?.as_str()?;
    if !source_id(selected_source) || build.get("version")?.as_str()? != version {
        return None;
    }
    Some(selected_source.to_owned())
}

/// One fixed optional file beside the already verified launch package. JSON never
/// supplies a path to open, and this observation changes no update or handoff state.
/// `selected_source` is the same validated build observation the launch report uses.
fn local_channel_report(root: &Path, version: &str, selected_source: &str) -> Option<String> {
    let channel = root.parent()?;
    if channel.file_name()? != "codex" {
        return None;
    }
    let unavailable = || Some("Local channel last report: unavailable".to_owned());
    let Some(text) = crate::life::read_managed_text(&channel.join(CHANNEL_STATUS)) else {
        return unavailable();
    };
    let Ok(record) = serde_json::from_str::<ChannelStatus>(&text) else {
        return unavailable();
    };
    if !record.valid() {
        return unavailable();
    }
    let same_selection = root.file_name().and_then(|name| name.to_str())
        == Some(record.initial_package.as_str())
        && selected_source == record.initial_source
        && version == record.initial_version;
    let context = if same_selection {
        "initial selection still matches the launch target"
    } else {
        "historical channel attempt; selection has changed"
    };
    let phase = if let Some(exit) = record.exit {
        format!("completed (exit {exit}; last phase {})", record.last_phase)
    } else {
        format!(
            "{}; completion not recorded; current activity unverified",
            record.phase
        )
    };
    let target = record.target_source.as_deref().unwrap_or("not reported");
    let target_context = match record.target_source.as_deref() {
        Some(target) if target == selected_source => "Target matches current launch source",
        Some(_) => "Target differs from current launch source",
        None => "Target source was not reported",
    };
    Some(format!(
        "Local channel last report: {phase}\nReported at: {}\nTarget source: {target}\n{context}\n{target_context}",
        record.when
    ))
}

/// The exact identities behind the readable running line: the ALab source, the client
/// version upstream's code and OpenAI's servers are told (upstream's numbering, never
/// codex-alab's own line: Decision 8), and the upstream base.
fn source_details(
    source: &str,
    client_version: &str,
    upstream_release: Option<&str>,
    upstream_commit: Option<&str>,
) -> String {
    format!(
        "Running ALab source: {source}\nCodex client version: {client_version}\nOpenAI Codex base: {}\nOpenAI source: {}",
        upstream_release.unwrap_or("unreleased"),
        upstream_commit.unwrap_or("unavailable"),
    )
}

fn status_report(
    running: &str,
    target: Option<&str>,
    source: Option<&str>,
    status: &UpdateStatus,
) -> String {
    let mut running_line = format!("Running codex-alab {running}");
    if running == "0.0.0" {
        running_line.push_str(" (unversioned development build)");
    }
    let mut lines = vec![running_line];
    if let Some(target) = target {
        lines.push(format!("Launch target: {target}"));
        lines.push(format!(
            "Launch source: {}",
            source.unwrap_or("unavailable")
        ));
    }
    lines.push(format!(
        "Last check: {}",
        status
            .state
            .as_deref()
            .unwrap_or("The aterm update monitor has not checked this session yet")
    ));
    if let Some(error) = &status.watch_error {
        lines.push(format!(
            "Notifications: {error}; checking the launch target every minute"
        ));
    }
    lines.push("aterm installs package updates; this session resumes on the new build when its idle checks permit it.".into());
    lines.join("\n")
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn version_details_keep_exact_source_identities_when_primary_labels_are_readable() {
        let alab = "a180bdb0c2d3aa744d4d3d59d1de6b5879f7ce36";
        let upstream = "e8aa7c1678caa9a368243d72bf0e9b7859d97f15";
        let report = source_details(
            alab,
            "0.164.0-dev.12479+g07a25d86a",
            Some("0.163.0"),
            Some(upstream),
        );
        assert!(report.contains(&format!("Running ALab source: {alab}")));
        assert!(report.contains("Codex client version: 0.164.0-dev.12479+g07a25d86a"));
        assert!(report.contains("OpenAI Codex base: 0.163.0"));
        assert!(report.contains(&format!("OpenAI source: {upstream}")));
        let unknown = source_details("dev", "0.0.0", None, None);
        assert!(unknown.contains("Running ALab source: dev"));
        assert!(unknown.contains("OpenAI Codex base: unreleased"));
        assert!(unknown.contains("OpenAI source: unavailable"));
    }

    /// A freshly installed package is not the image that runs this session. The report
    /// keeps those versions distinct while exposing why the safe handoff remains pending.
    #[test]
    fn an_installed_update_does_not_change_the_reported_running_version() {
        let status = UpdateStatus {
            pending: false,
            state: Some("Update pending: thread t1 has an active goal".into()),
            watch_error: Some("the launch watcher stopped".into()),
        };
        let running = "0.164.0-dev.12479+g07a25d86a";
        let installed = "0.164.0-dev.12521+g3d3eb12d8";
        let report = status_report(
            running,
            Some("0.164.0-dev.12521+g3d3eb12d8 (/owned/package/bin/codex)"),
            None,
            &status,
        );
        let mut lines = report.lines();
        assert!(
            lines
                .next()
                .is_some_and(|line| line.contains(running) && !line.contains(installed))
        );
        assert!(
            lines
                .next()
                .is_some_and(|line| line.contains(installed) && !line.contains(running))
        );
        assert!(report.contains("thread t1 has an active goal"));
        assert!(report.contains("the launch watcher stopped"));
        let development = status_report("0.0.0", Some(installed), None, &status);
        assert!(
            development
                .lines()
                .next()
                .is_some_and(|line| line.contains("0.0.0")
                    && line.contains("unversioned")
                    && !line.contains(installed))
        );

        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let a = dir.path().join("a/bin/codex");
        let b = dir.path().join("b/bin/codex");
        for (exe, version) in [(&a, running), (&b, installed)] {
            let bin = exe.parent().unwrap_or_else(|| panic!("a bin directory"));
            let root = bin
                .parent()
                .unwrap_or_else(|| panic!("a package directory"));
            std::fs::create_dir_all(bin).unwrap_or_else(|err| panic!("{err}"));
            std::fs::write(exe, "image identity; never executed")
                .unwrap_or_else(|err| panic!("{err}"));
            std::fs::write(
                root.join("codex-package.json"),
                format!(r#"{{"layoutVersion":1,"entrypoint":"bin/codex","version":"{version}"}}"#),
            )
            .unwrap_or_else(|err| panic!("{err}"));
        }
        let route = dir.path().join("managed-codex");
        let launch = Launch {
            argv0: Some(a.clone()),
            current_exe: Some(a),
            managed_argv0: Some(route.clone()),
            ..Launch::default()
        };
        assert_eq!(
            launch_target(&launch)
                .as_ref()
                .map(|target| target.summary.as_str()),
            Some("unavailable; the current session keeps running")
        );
        std::os::unix::fs::symlink(&b, route).unwrap_or_else(|err| panic!("{err}"));
        let target = launch_target(&launch).unwrap_or_else(|| panic!("a managed target"));
        assert!(target.summary.starts_with(installed));
        let report = status_report(
            running,
            Some(&target.summary),
            target.source.as_deref(),
            &status,
        );
        assert!(
            report
                .lines()
                .next()
                .is_some_and(|line| line.contains(running) && !line.contains(installed))
        );
        assert!(report.contains(&format!("Launch target: {installed}")));

        let c_root = dir.path().join("c");
        std::fs::create_dir_all(c_root.join("bin")).unwrap_or_else(|err| panic!("{err}"));
        let c_exe = c_root.join("bin/codex");
        std::fs::write(&c_exe, "image identity").unwrap_or_else(|err| panic!("{err}"));
        std::os::unix::fs::symlink(
            dir.path().join("b/codex-package.json"),
            c_root.join("codex-package.json"),
        )
        .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(package_version(&c_exe), None);
        std::fs::remove_file(c_root.join("codex-package.json"))
            .unwrap_or_else(|err| panic!("{err}"));
        std::fs::write(
            c_root.join("codex-package.json"),
            format!(r#"{{"layoutVersion":1,"entrypoint":"bin/alias","version":"{installed}"}}"#),
        )
        .unwrap_or_else(|err| panic!("{err}"));
        std::os::unix::fs::symlink(&c_exe, c_root.join("bin/alias"))
            .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(package_version(&c_exe), None);
        std::os::unix::fs::symlink(c_root.join("bin"), c_root.join("alias-dir"))
            .unwrap_or_else(|err| panic!("{err}"));
        std::fs::write(
            c_root.join("codex-package.json"),
            format!(
                r#"{{"layoutVersion":1,"entrypoint":"alias-dir/codex","version":"{installed}"}}"#
            ),
        )
        .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(package_version(&c_exe), None);
    }

    /// The channel report is optional historical data, not the running image, a
    /// process probe, or permission to select or re-exec a package.
    #[test]
    fn local_channel_reports_keep_selection_context_and_reject_special_or_partial_files() {
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let channel = dir.path().join("codex");
        let a_source = "a".repeat(40);
        let b_source = "b".repeat(40);
        let a_version = "0.164.0-dev.1+gaaaaaaaaa";
        let b_version = "0.164.0-dev.2+gbbbbbbbbb";
        let package = |name: &str, source: &str, version: &str| {
            let root = channel.join(name);
            std::fs::create_dir_all(root.join("bin")).unwrap_or_else(|err| panic!("{err}"));
            let exe = root.join("bin/codex");
            std::fs::write(&exe, "private image identity; never executed")
                .unwrap_or_else(|err| panic!("{err}"));
            std::fs::write(
                root.join("codex-package.json"),
                serde_json::json!({"layoutVersion":1,"entrypoint":"bin/codex","version":version})
                    .to_string(),
            )
            .unwrap_or_else(|err| panic!("{err}"));
            std::fs::write(
                root.join("alab-build.json"),
                serde_json::json!({"source":source,"version":version}).to_string(),
            )
            .unwrap_or_else(|err| panic!("{err}"));
            exe
        };
        let a = package("a", &a_source, a_version);
        let b = package("b", &b_source, b_version);
        let rebuilt = package("a-rebuilt", &a_source, a_version);
        let route = dir.path().join("managed-codex");
        std::os::unix::fs::symlink(&a, &route).unwrap_or_else(|err| panic!("{err}"));
        let launch = Launch {
            argv0: Some(a.clone()),
            current_exe: Some(a),
            managed_argv0: Some(route.clone()),
            ..Launch::default()
        };
        let render = || {
            let target = launch_target(&launch).unwrap_or_else(|| panic!("a managed target"));
            format!(
                "{}\n{}",
                status_report(
                    a_version,
                    Some(&target.summary),
                    target.source.as_deref(),
                    &UpdateStatus::default()
                ),
                target.local_report.unwrap_or_default(),
            )
        };
        let path = channel.join(CHANNEL_STATUS);
        assert!(render().contains("Local channel last report: unavailable"));
        assert!(render().contains(&format!("Launch source: {a_source}")));
        let mut record = serde_json::json!({
            "schema":1,"when":"2000-01-01T00:00:00+00:00",
            "phase":"building","last_phase":"building","complete":false,"exit":null,
            "initial_package":"a","initial_source":a_source,"initial_version":a_version,
            "target_source":b_source,
            "reason":"secret diagnostic must never be shown",
            "log":"/not/a/path/to/open"
        });
        let write = |record: &serde_json::Value| {
            std::fs::write(&path, record.to_string()).unwrap_or_else(|err| panic!("{err}"));
        };
        write(&record);
        let incomplete = render();
        assert!(incomplete.starts_with(&format!("Running codex-alab {a_version}")));
        assert!(incomplete.contains("Launch target: 0.164.0-dev.1"));
        assert!(
            incomplete.contains("building; completion not recorded; current activity unverified")
        );
        assert!(incomplete.contains("2000-01-01T00:00:00+00:00"));
        assert!(incomplete.contains(&b_source));
        assert!(!incomplete.contains("secret"));
        assert!(!incomplete.contains("historical channel attempt"));

        record["phase"] = "completed".into();
        record["complete"] = true.into();
        record["exit"] = 1.into();
        write(&record);
        assert!(render().contains("completed (exit 1; last phase building)"));
        std::fs::remove_file(&route).unwrap_or_else(|err| panic!("{err}"));
        std::os::unix::fs::symlink(&b, &route).unwrap_or_else(|err| panic!("{err}"));
        record["exit"] = 0.into();
        record["last_phase"] = "selecting".into();
        write(&record);
        let changed = render();
        assert!(changed.starts_with(&format!("Running codex-alab {a_version}")));
        assert!(changed.contains(&format!("Launch target: {b_version}")));
        assert!(changed.contains(&format!("Launch source: {b_source}")));
        assert!(changed.contains("historical channel attempt; selection has changed"));
        assert!(changed.contains("completed (exit 0; last phase selecting)"));
        assert!(changed.contains("Target matches current launch source"));
        std::fs::remove_file(&route).unwrap_or_else(|err| panic!("{err}"));
        std::os::unix::fs::symlink(&rebuilt, &route).unwrap_or_else(|err| panic!("{err}"));
        let rebuilt_report = render();
        assert!(rebuilt_report.contains(&format!("Launch source: {a_source}")));
        assert!(!rebuilt_report.contains(&format!("Launch source: {b_source}")));
        assert!(rebuilt_report.contains("historical channel attempt; selection has changed"));
        assert!(rebuilt_report.contains("Target differs from current launch source"));

        for (field, value) in [
            ("schema", serde_json::json!(2)),
            (
                "when",
                serde_json::json!("2000-01-01T00:00:00+00:00\nforged"),
            ),
            ("initial_package", serde_json::json!("../a")),
            ("initial_source", serde_json::json!("not a source")),
            ("initial_version", serde_json::json!("version\u{1b}[0m")),
            ("target_source", serde_json::json!("/not/a/path/to/open")),
            ("last_phase", serde_json::json!("unknown")),
            ("complete", serde_json::json!(false)),
            ("exit", serde_json::Value::Null),
        ] {
            let mut bad = record.clone();
            bad[field] = value;
            write(&bad);
            assert!(
                render().contains("Local channel last report: unavailable"),
                "{field}"
            );
        }
        let mut oversized = record.clone();
        oversized["padding"] = "x".repeat(64 * 1024).into();
        for bytes in [b"{".to_vec(), oversized.to_string().into_bytes()] {
            std::fs::write(&path, bytes).unwrap_or_else(|err| panic!("{err}"));
            assert!(render().contains("Local channel last report: unavailable"));
        }
        std::fs::remove_file(&path).unwrap_or_else(|err| panic!("{err}"));
        let outside = dir.path().join("outside-record");
        std::fs::write(&outside, record.to_string()).unwrap_or_else(|err| panic!("{err}"));
        std::os::unix::fs::symlink(&outside, &path).unwrap_or_else(|err| panic!("{err}"));
        assert!(render().contains("Local channel last report: unavailable"));
        std::fs::remove_file(&path).unwrap_or_else(|err| panic!("{err}"));
        let fifo = CString::new(path.as_os_str().as_bytes()).unwrap_or_else(|err| panic!("{err}"));
        // SAFETY: fifo is a live NUL-terminated private path; no writer is created.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(render().contains("Local channel last report: unavailable"));
        std::fs::remove_file(&path).unwrap_or_else(|err| panic!("{err}"));
        std::fs::create_dir(&path).unwrap_or_else(|err| panic!("{err}"));
        assert!(render().contains("Local channel last report: unavailable"));
        std::fs::remove_dir(&path).unwrap_or_else(|err| panic!("{err}"));
        write(&record);
        let build_path = channel.join("a-rebuilt/alab-build.json");
        for bytes in [
            "{".to_owned(),
            serde_json::json!({"source":"wrong","version":a_version}).to_string(),
            serde_json::json!({"source":a_source,"version":b_version}).to_string(),
        ] {
            std::fs::write(&build_path, bytes).unwrap_or_else(|err| panic!("{err}"));
            let unknown = render();
            assert!(unknown.contains(&format!("Launch target: {a_version}")));
            assert!(unknown.contains("Launch source: unavailable"));
            assert!(!unknown.contains("Target matches current launch source"));
            assert!(launch_target(&launch).is_some_and(|target| target.local_report.is_none()));
        }
        std::fs::remove_file(&build_path).unwrap_or_else(|err| panic!("{err}"));
        let missing = render();
        assert!(missing.contains(&format!("Launch target: {a_version}")));
        assert!(missing.contains("Launch source: unavailable"));
        assert!(!missing.contains("Target matches current launch source"));
        assert_eq!(
            std::fs::read_to_string(&outside).ok(),
            Some(record.to_string())
        );
    }
}

/// How long children (MCP servers, mostly) get to leave after the teardown.
pub const CHILDREN_GRACE: Duration = Duration::from_secs(2);
/// A stopped event loop must not retain an unacknowledged handoff indefinitely.
const HANDOFF_ACK_BUDGET: Duration = Duration::from_secs(60);

/// A build: the file a path finally names, by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildId {
    pub dev: u64,
    pub ino: u64,
}

/// The identity of the regular file `path` names (symlinks followed).
pub fn build_id(path: &Path) -> Option<BuildId> {
    let metadata = std::fs::metadata(path).ok()?;
    metadata.is_file().then(|| BuildId {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

/// The binary an atpkg shim execs: the last line `exec '<absolute path>' "$@"` of a
/// `#!/bin/sh` script. Anything else names nothing.
pub fn shim_target(script: &str) -> Option<PathBuf> {
    // Match atpkg's exact interpreter line, not a prefix naming another interpreter.
    if !script.starts_with("#!/bin/sh\n") {
        return None;
    }
    let last = script.lines().rev().find(|line| !line.trim().is_empty())?;
    let quoted = last.trim().strip_prefix("exec ")?.strip_suffix(" \"$@\"")?;
    let path = crate::life::shell_value(quoted)?;
    if !path.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(path))
}

/// The build `launch` names now: the file it resolves to, or the binary it execs when that
/// file is an atpkg shim.
pub fn named_build(launch: &Path) -> Option<PathBuf> {
    const LIMIT: u64 = 64 * 1024;
    let resolved = std::fs::canonicalize(launch).ok()?;
    let head = read_head(&resolved, LIMIT + 1)?;
    if head.starts_with(b"#!") {
        if head.len() > LIMIT as usize {
            return None;
        }
        let target = shim_target(std::str::from_utf8(&head).ok()?)?;
        return std::fs::canonicalize(target).ok();
    }
    Some(resolved)
}

fn read_head(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    // A changed launch path must not park the monitor (or `/alab update`) on a FIFO.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut head = Vec::new();
    file.take(limit).read_to_end(&mut head).ok()?;
    Some(head)
}

#[cfg(test)]
pub(crate) fn package_version(exe: &Path) -> Option<String> {
    package_details(exe).map(|(_, version)| version)
}

fn package_details(exe: &Path) -> Option<(PathBuf, String)> {
    let exe = std::fs::canonicalize(exe).ok()?;
    exe.ancestors().skip(1).take(2).find_map(|root| {
        let text = crate::life::read_managed_text(&root.join("codex-package.json"))?;
        let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
        if manifest.get("layoutVersion")?.as_u64()? != 1 {
            return None;
        }
        let entry = Path::new(manifest.get("entrypoint")?.as_str()?);
        let entry_path = root.join(entry);
        if !crate::life::regular_entry_under(root, entry)
            || std::fs::canonicalize(entry_path).ok()? != exe
        {
            return None;
        }
        let version = manifest.get("version")?.as_str()?;
        report_text(version, 128).then(|| (root.to_path_buf(), version.to_owned()))
    })
}

/// Whether `launch` now names a build other than `current` (a file that exists and is
/// executable). `false` whenever anything cannot be read.
pub fn names_other_build(launch: &Path, current: BuildId) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(named) = named_build(launch) else {
        return false;
    };
    let executable =
        std::fs::metadata(&named).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    executable && build_id(&named).is_some_and(|id| id != current)
}

/// The trigger, for this process: its launch path names another build than it runs.
pub fn launch_names_other_build(launch: &Launch) -> bool {
    match (launch.upgrade_path(), launch.exe_id) {
        (Some(argv0), Some((dev, ino))) => names_other_build(argv0, BuildId { dev, ino }),
        _ => false,
    }
}

/// Everything the exec needs, decided before the teardown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecPlan {
    /// Initially the launch path; preparation pins this to the admitted image.
    pub path: PathBuf,
    pub argv: Vec<OsString>,
    /// The launch environment, verbatim and in order.
    pub env: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
}

/// Immutable resume arguments and exact native-admitted image/custody. A launch
/// shim changing after this preparation can never substitute another executable.
pub struct PreparedExec {
    plan: ExecPlan,
    admitted: Received,
    custody: ExecCustody,
    _preparation: Option<tokio::sync::SemaphorePermit<'static>>,
}

static PREPARATION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

impl PreparedExec {
    fn prepare(plan: ExecPlan, admitted: Received, deadline: Instant) -> Result<Self, String> {
        admitted
            .probe(deadline)
            .map_err(|error| error.to_string())?;
        Self::finish(plan, admitted)
    }

    fn finish(mut plan: ExecPlan, admitted: Received) -> Result<Self, String> {
        if plan.argv.first().map(OsString::as_os_str) != Some(plan.path.as_os_str()) {
            return Err("the resume argv does not start with the original launch path".into());
        }
        plan.path = admitted
            .receipt()
            .executable
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let custody = admitted
            .prepare_exec_custody()
            .map_err(|error| error.to_string())?;
        let prepared = Self {
            plan,
            admitted,
            custody,
            _preparation: None,
        };
        prepared.validate()?;
        Ok(prepared)
    }

    /// Install the final source snapshot before teardown. Data custody supplies
    /// no native execution admission and does not commit source producers.
    pub fn with_state(mut self, state: &[u8]) -> Result<Self, String> {
        self.validate()?;
        self.custody = self
            .admitted
            .prepare_exec_custody_with_state(Some(state))
            .map_err(|error| error.to_string())?;
        Ok(self)
    }

    pub fn plan(&self) -> &ExecPlan {
        &self.plan
    }

    pub fn validate(&self) -> Result<(), String> {
        self.admitted
            .validate(
                &self.admitted.receipt().executable,
                self.admitted.receipt().build,
            )
            .map_err(|error| error.to_string())?;
        if self.plan.path
            != self
                .admitted
                .receipt()
                .executable
                .canonicalize()
                .map_err(|error| error.to_string())?
        {
            return Err("the admitted physical executable changed".into());
        }
        check_launch_cwd(self.plan.cwd.as_deref())
    }

    fn execute(self) -> String {
        if let Err(why) = self.validate() {
            return why;
        }
        exec(&self.plan, Some(&self.custody))
    }
}

/// Prepare through the authenticated native control host, without consulting
/// PATH or changing the managed selection. Cancellation retains original child
/// ownership in its worker; the complete preparation slot follows the probe.
pub async fn prepare_admitted(
    shared: &Shared,
    client: &codex_alab_aterm::Client,
    deadline: Instant,
) -> Result<PreparedExec, String> {
    #[cfg(feature = "test-support")]
    if let Some(prepared) = test_support::prepare_if_registered(shared) {
        return prepared;
    }
    let permit = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        PREPARATION.acquire(),
    )
    .await
    .map_err(|_| "another target preparation is still running".to_owned())?
    .map_err(|error| error.to_string())?;
    let host = crate::upgrade_host::Host::capture(client)?;
    let image = shared
        .launch()
        .and_then(Launch::upgrade_path)
        .and_then(named_build)
        .ok_or("the launch path no longer identifies a candidate")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let root = image
        .parent()
        .filter(|bin| bin.file_name() == Some(std::ffi::OsStr::new("bin")))
        .and_then(Path::parent)
        .ok_or("the candidate is not a native package entrypoint")?;
    let build = root
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|name| name.parse::<u64>().ok())
        .filter(|build| *build > 0)
        .ok_or("the candidate does not identify an exact published build")?;
    let admitted =
        crate::upgrade_admission::admit(host.path().to_path_buf(), image, build, deadline)
            .await
            .map_err(|error| error.to_string())?;
    host.validate(client)?;
    let plan = exec_plan(shared)?;
    tokio::task::spawn_blocking(move || {
        let mut prepared = PreparedExec::prepare(plan, admitted, deadline)?;
        prepared._preparation = Some(permit);
        Ok(prepared)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The exec for `launch` resuming `thread`, its argv rebuilt from codex's own clap
/// `Command` as the record's `resumeArgv` is. `Err(why)` when it cannot be exact.
pub fn plan_for(
    launch: &Launch,
    command: fn() -> clap::Command,
    thread: &str,
) -> Result<ExecPlan, String> {
    let argv0 = launch.upgrade_path().ok_or("the launch path is unknown")?;
    let argv0_str = argv0.to_str().ok_or("the launch path is not UTF-8")?;
    let argv = resume_argv::rebuild(command(), &launch.args, argv0_str, thread)
        .ok_or("the launch command line cannot be rewritten to resume")?;
    if argv.first().map(String::as_str) != Some(argv0_str) {
        return Err("the resume argv does not start with the launch path".to_owned());
    }
    Ok(ExecPlan {
        path: argv0.to_path_buf(),
        argv: argv.into_iter().map(OsString::from).collect(),
        env: launch.vars.clone(),
        cwd: launch.cwd.clone(),
    })
}

/// The exec for this process now: its launch, codex's clap definition, and the primary
/// thread, whose rollout must be on disk (a resume cannot find it otherwise).
pub fn exec_plan(shared: &Shared) -> Result<ExecPlan, String> {
    let launch = shared.launch().ok_or("no launch snapshot")?;
    check_launch_cwd(launch.cwd.as_deref())?;
    let command = resume_argv::command().ok_or("codex's command line was not captured")?;
    let view = shared.view();
    let thread = view.primary_thread.as_deref().ok_or("no primary thread")?;
    let materialized = view
        .primary()
        .and_then(|facts| facts.rollout_path.as_deref())
        .is_some_and(rollout_materialized);
    if !materialized {
        return Err("the primary thread's rollout is not on disk yet".to_owned());
    }
    plan_for(launch, command, thread)
}

/// Check without changing this process's directory: an unavailable launch directory
/// holds the update while the existing session can still run. Search permission is
/// enough for chdir; a directory need not be readable to remain a valid working directory.
fn check_launch_cwd(cwd: Option<&Path>) -> Result<(), String> {
    let cwd = cwd.ok_or("the launch working directory is unknown")?;
    let unavailable = |err| {
        format!(
            "the launch working directory {} is unavailable: {err}",
            cwd.display()
        )
    };
    let metadata = std::fs::metadata(cwd).map_err(unavailable)?;
    if !metadata.is_dir() {
        return Err(format!(
            "the launch working directory {} is not a directory",
            cwd.display()
        ));
    }
    let path = cstring(cwd.as_os_str().as_bytes())?;
    // SAFETY: path is a live NUL-terminated string; access does not mutate process state.
    if unsafe { libc::access(path.as_ptr(), libc::X_OK) } != 0 {
        return Err(unavailable(std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Leave like `/exit` so the exit path execs. `false` (and nothing pending) when the TUI
/// never said how to leave.
#[cfg(test)]
pub fn request(shared: &Shared) -> bool {
    shared.set_exec_pending(true);
    if shared.leave() {
        true
    } else {
        shared.set_exec_pending(false);
        false
    }
}

/// Queue the upgrade's conditional event and wait for the TUI to accept it using its
/// current native state. A cancelled waiter withdraws the queued request; its caller
/// also invalidates the native generation before restoring any suspended act.
pub(crate) async fn request_checked(
    shared: &Shared,
    commit: impl FnOnce() -> bool + Send + 'static,
) -> bool {
    let (answer, accepted) = tokio::sync::oneshot::channel();
    let (request, cancellation) =
        codex_alab_core::session::UpgradeRequest::new(move |native_ready| {
            let committed = native_ready && commit();
            let _ = answer.send(committed);
            committed
        });
    if !shared.queue_upgrade(request) {
        return false;
    }
    let result = matches!(
        tokio::time::timeout(HANDOFF_ACK_BUDGET, accepted).await,
        Ok(Ok(true))
    );
    drop(cancellation);
    result
}

/// Install the exact owned plan only at the TUI's successful native commit.
/// Cancellation or refusal drops custody and leaves the running session intact.
pub async fn request_admitted(
    shared: &'static Shared,
    prepared: PreparedExec,
    commit: impl FnOnce() -> bool + Send + 'static,
) -> bool {
    let Some(launch) = shared
        .launch()
        .and_then(Launch::upgrade_path)
        .map(Path::to_path_buf)
    else {
        return false;
    };
    request_checked(shared, move || {
        if prepared.validate().is_err()
            || named_build(&launch)
                .and_then(|path| path.canonicalize().ok())
                .as_ref()
                != Some(&prepared.plan.path)
        {
            return false;
        }
        shared.commit_upgrade_execution(Box::new(move || prepared.execute()), commit)
    })
    .await
}

/// The exit path of an upgrade: exec the plan, or `_exit(1)` with the record kept.
pub fn exec_after_teardown(shared: &Shared) -> ! {
    use std::io::Write;
    let prepared = shared.take_upgrade_execution();
    wait_for_children(CHILDREN_GRACE);
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    let why = match prepared {
        Some(prepared) => prepared(),
        None => "no admitted execution plan was committed".to_owned(),
    };
    let _ = writeln!(
        std::io::stderr(),
        "codex-alab: the upgrade could not re-exec ({why}); aterm relaunches this session"
    );
    // SAFETY: _exit has no preconditions; it ends the process without unwinding.
    unsafe { libc::_exit(1) }
}

/// Wait up to `grace` for this process's children to go (reaping the ones that did).
pub fn wait_for_children(grace: Duration) -> usize {
    let deadline = Instant::now() + grace;
    loop {
        // SAFETY: waitpid with WNOHANG on any child only reaps; a null status is allowed.
        while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
        let left = live_children();
        if left == 0 || Instant::now() >= deadline {
            return left;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "macos")]
fn live_children() -> usize {
    let mut pids = [0 as libc::pid_t; 256];
    let size = i32::try_from(std::mem::size_of_val(&pids)).unwrap_or(i32::MAX);
    // SAFETY: the buffer is valid for `size` bytes; the call writes at most that many.
    let n = unsafe {
        libc::proc_listchildpids(
            libc::getpid(),
            pids.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    usize::try_from(n).unwrap_or(0)
}

#[cfg(not(target_os = "macos"))]
fn live_children() -> usize {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return 0;
    };
    tasks
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .map(|children| children.split_whitespace().count())
        .sum()
}

fn cstring(bytes: &[u8]) -> Result<CString, String> {
    CString::new(bytes).map_err(|_| "an argument holds a NUL byte".to_owned())
}

/// `execve` the plan; returns only why it failed.
fn exec(plan: &ExecPlan, custody: Option<&ExecCustody>) -> String {
    let path = match cstring(plan.path.as_os_str().as_bytes()) {
        Ok(path) => path,
        Err(why) => return why,
    };
    let argv: Result<Vec<CString>, String> = plan
        .argv
        .iter()
        .map(|arg| cstring(arg.as_bytes()))
        .collect();
    let env: Result<Vec<CString>, String> = plan
        .env
        .iter()
        .map(|(key, value)| {
            let mut entry = key.as_bytes().to_vec();
            entry.push(b'=');
            entry.extend_from_slice(value.as_bytes());
            cstring(&entry)
        })
        .collect();
    let (argv, env) = match (argv, env) {
        (Ok(argv), Ok(env)) => (argv, env),
        (Err(why), _) | (_, Err(why)) => return why,
    };
    let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null());
    let mut env_ptrs: Vec<*const libc::c_char> = env.iter().map(|e| e.as_ptr()).collect();
    env_ptrs.push(std::ptr::null());
    if let Some(cwd) = &plan.cwd
        && let Err(err) = std::env::set_current_dir(cwd)
    {
        return format!("chdir {}: {err}", cwd.display());
    }
    reset_signals();
    if let Some(custody) = custody
        && let Err(error) = custody.inherit()
    {
        return format!("inherit original executable custody: {error}");
    }
    // SAFETY: every pointer is a NUL-terminated string kept alive above, and both arrays
    // end with a null pointer, as execve requires.
    unsafe { libc::execve(path.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr()) };
    format!(
        "execve {}: {}",
        plan.path.display(),
        std::io::Error::last_os_error()
    )
}

#[cfg(test)]
mod cwd_tests {
    use super::*;

    /// A directory can disappear after the preflight, while the TUI tears down.
    /// The failed chdir must stop before exec, preserving the current process directory.
    #[test]
    fn a_directory_lost_during_teardown_never_falls_back_to_another_working_directory() {
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let cwd = dir.path().join("work");
        std::fs::create_dir(&cwd).unwrap_or_else(|err| panic!("{err}"));
        assert!(check_launch_cwd(Some(&cwd)).is_ok());
        std::fs::remove_dir(&cwd).unwrap_or_else(|err| panic!("{err}"));
        let before = std::env::current_dir().unwrap_or_else(|err| panic!("{err}"));
        let why = exec(
            &ExecPlan {
                path: dir.path().join("unreached-executable"),
                argv: vec!["unreached-executable".into()],
                env: Vec::new(),
                cwd: Some(cwd.clone()),
            },
            None,
        );
        assert!(
            why.starts_with(&format!("chdir {}:", cwd.display())),
            "{why}"
        );
        assert_eq!(
            std::env::current_dir().ok().as_deref(),
            Some(before.as_path())
        );
    }
}

/// The new image starts as a launched process does: no signal blocked, and the signals a
/// runtime or a TUI ignores back at their default (an ignored disposition survives exec).
fn reset_signals() {
    // SAFETY: sigemptyset/sigprocmask/signal with valid arguments are always sound.
    unsafe {
        let mut set = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        libc::sigemptyset(set.as_mut_ptr());
        libc::sigprocmask(libc::SIG_SETMASK, set.as_ptr(), std::ptr::null_mut());
        for signal in [
            libc::SIGPIPE,
            libc::SIGINT,
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGQUIT,
            libc::SIGTSTP,
            libc::SIGTTIN,
            libc::SIGTTOU,
            libc::SIGCHLD,
            libc::SIGWINCH,
        ] {
            libc::signal(signal, libc::SIG_DFL);
        }
    }
}
