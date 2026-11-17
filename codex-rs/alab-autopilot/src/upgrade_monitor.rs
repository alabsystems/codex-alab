// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Launch-path notifications are hints. Reconcile independently even when the watcher
//! cannot start, misses a downstream symlink change, or closes. A delayed check schedules
//! from the current time, so waking a sleeping machine never replays the missed minutes.

use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

pub(crate) fn watch_paths(
    directories: &[std::path::PathBuf],
) -> Vec<codex_file_watcher::WatchPath> {
    directories
        .iter()
        .map(|path| codex_file_watcher::WatchPath {
            path: path.clone(),
            recursive: false,
        })
        .collect()
}

pub(crate) const RECONCILE_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    Hint,
    Reconcile,
    WatchClosed,
}

pub(crate) struct Monitor {
    next: Instant,
}

impl Monitor {
    pub(crate) fn new() -> Self {
        Self {
            next: Instant::now(),
        }
    }

    /// `hint` returns false only when its watcher closes. The caller then substitutes a
    /// pending future; a closed channel must never turn the monitor into a busy loop.
    pub(crate) async fn wait(&mut self, hint: impl Future<Output = bool>) -> Wake {
        tokio::select! {
            event = hint => {
                if event { Wake::Hint } else { Wake::WatchClosed }
            }
            _ = tokio::time::sleep_until(self.next) => {
                self.next = Instant::now() + RECONCILE_AFTER;
                Wake::Reconcile
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;

    #[tokio::test]
    async fn downstream_selection_rename_wakes_the_real_launch_watcher() {
        use std::os::unix::fs::symlink;
        // macOS excludes .ubd trees from FSEvents. This fixture tests events.
        // The gate allocates a private non-.ubd scratch root; its sandbox does
        // not grant writes to the rest of /tmp.
        let watch_root = std::env::var_os("ALAB_TEST_SCRATCH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
        let temporary = tempfile::Builder::new()
            .prefix("codex-launch-watch-")
            .tempdir_in(watch_root)
            .unwrap();
        let root = temporary.path().canonicalize().unwrap();
        for directory in ["launch", "route", "store"] {
            std::fs::create_dir(root.join(directory)).unwrap();
        }
        for image in ["old", "new"] {
            std::fs::write(root.join("store").join(image), b"image").unwrap();
        }
        symlink("../store/old", root.join("route/current")).unwrap();
        symlink("../route/current", root.join("launch/codex")).unwrap();
        let watcher = std::sync::Arc::new(codex_file_watcher::FileWatcher::new().unwrap());
        let (subscriber, mut events) = watcher.add_subscriber();
        let directories = codex_alab_tui::upgrade::watch_directories(&root.join("launch/codex"));
        let _registration = subscriber.register_paths(watch_paths(&directories));
        assert!(_registration.registration_succeeded());
        symlink("../store/new", root.join("route/next")).unwrap();
        std::fs::rename(root.join("route/next"), root.join("route/current")).unwrap();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let event = events.recv().await.expect("native watcher remains open");
                if event
                    .paths
                    .iter()
                    .any(|path| path.starts_with(root.join("route")))
                {
                    break;
                }
            }
        })
        .await
        .expect("downstream rename notification, without the minute fallback");
        assert_eq!(
            codex_alab_tui::upgrade::named_build(&root.join("launch/codex")),
            Some(root.join("store/new")),
        );
    }

    /// No notification is required, including after a lost event or failed watcher.
    /// A hint does not postpone the independent deadline; a closed watcher is disabled by
    /// the caller and the same deadline still fires. Virtual time contains no latency budget.
    #[tokio::test(start_paused = true)]
    async fn missed_and_closed_watches_leave_reconciliation_running() {
        let mut monitor = Monitor::new();
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        let due = Instant::now() + RECONCILE_AFTER;
        tokio::time::advance(RECONCILE_AFTER / 2).await;
        assert_eq!(monitor.wait(async { true }).await, Wake::Hint);
        assert_eq!(monitor.wait(async { false }).await, Wake::WatchClosed);
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        assert_eq!(Instant::now(), due);
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        assert_eq!(Instant::now(), due + RECONCILE_AFTER);
    }

    #[tokio::test(start_paused = true)]
    async fn a_delayed_check_does_not_replay_every_missed_minute() {
        let mut monitor = Monitor::new();
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        tokio::time::advance(RECONCILE_AFTER * 1_000).await;
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        let checked = Instant::now();
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        assert_eq!(Instant::now(), checked + RECONCILE_AFTER);
    }

    /// A stable launch link can hide another link's rename outside its watched parent.
    /// Exercise the monitor with the actual build reader and no delivered notification.
    #[tokio::test(start_paused = true)]
    async fn a_new_build_behind_an_unchanged_launch_link_is_found_without_a_hint() {
        use codex_alab_tui::upgrade::build_id;
        use codex_alab_tui::upgrade::names_other_build;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        for path in [&old, &new] {
            std::fs::write(path, b"captured build; never executed")
                .unwrap_or_else(|err| panic!("{err}"));
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .unwrap_or_else(|err| panic!("{err}"));
        }
        let downstream = dir.path().join("current");
        std::os::unix::fs::symlink(&old, &downstream).unwrap_or_else(|err| panic!("{err}"));
        let parent = dir.path().join("agents");
        std::fs::create_dir(&parent).unwrap_or_else(|err| panic!("{err}"));
        let launch = parent.join("codex");
        std::os::unix::fs::symlink(&downstream, &launch).unwrap_or_else(|err| panic!("{err}"));
        let running = build_id(&old).unwrap_or_else(|| panic!("old build has an identity"));
        let mut monitor = Monitor::new();
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        assert!(!names_other_build(&launch, running));
        let replacement = dir.path().join("next");
        std::os::unix::fs::symlink(&new, &replacement).unwrap_or_else(|err| panic!("{err}"));
        std::fs::rename(&replacement, &downstream).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(monitor.wait(pending()).await, Wake::Reconcile);
        assert!(names_other_build(&launch, running));
    }
}
