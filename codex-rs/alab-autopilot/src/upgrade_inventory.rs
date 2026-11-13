// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The actual manager's native admission owns the updater's final inventory.
use super::*;

impl Autopilot {
    pub(super) async fn prepare_upgrade_inventory(
        &self,
    ) -> Result<codex_core::UpgradeManagerBarrier, String> {
        tokio::time::timeout(Duration::from_secs(5), async {
            let manager = lock(&self.thread_manager)
                .clone()
                .and_then(|weak| weak.upgrade())
                .ok_or("the native thread manager is unavailable")?;
            let primary = self
                .sup
                .shared()
                .view()
                .primary_thread
                .ok_or("the primary thread is unavailable")?;
            let primary = ThreadId::from_string(&primary).map_err(|error| error.to_string())?;
            // Resolve before retaining the manager's registry read guard.
            let thread = manager
                .get_thread(primary)
                .await
                .map_err(|error| error.to_string())?;
            thread
                .prepare_upgrade_manager()
                .await
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "the native thread inventory did not settle".to_owned())?
    }
}

/// The package image is pinned separately. The resume must still name the same
/// conversation and retain its original flags, environment and working directory.
pub(super) fn resume_matches(
    shared: &codex_alab_core::session::Shared,
    expected: &codex_alab_tui::upgrade::ExecPlan,
) -> bool {
    codex_alab_tui::upgrade::exec_plan(shared).is_ok_and(|current| {
        current.argv == expected.argv && current.env == expected.env && current.cwd == expected.cwd
    })
}
