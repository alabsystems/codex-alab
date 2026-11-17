// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve the complete ORIGINAL Resume policy/history/started conversion,
//! retaining its single genuine ProjectionReplay through the actual result.
//! A real Native FrontendFrontier is a required resource; no selector chooses
//! birth/replacement from argv, labels, cached response or candidate DATA.
use super::*;

/// The original helper still consumes every original policy input. This groups
/// its arguments without rebuilding defaults or reducing the selected policy.
pub(crate) struct OriginalResume<'a> {
    pub(crate) local_settings: &'a crate::local_settings::LocalSettings,
    pub(crate) config: Config,
    pub(crate) thread_id: ThreadId,
    pub(crate) model_settings: ResumeModelSettings,
    pub(crate) permission_overrides: crate::resume_permissions::ResumePermissions,
}

pub(crate) struct ProjectedStartedThread {
    started: AppServerStartedThread,
    replay: ProjectionReplay,
}
impl ProjectedStartedThread {
    pub(crate) fn started(&self) -> &AppServerStartedThread {
        &self.started
    }
    pub(crate) fn into_parts(self) -> (AppServerStartedThread, ProjectionReplay) {
        (self.started, self.replay)
    }
}
impl AppServerSession {
    /// R reconciles only the already selected original Core. Every construction
    /// candidate, including the first Core, uses its distinct Source selection.
    /// Every full original local/settings/model/permission/history input is
    /// passed unchanged to the existing ordinary method. Only its first exact
    /// ThreadResume send is protected; a second fallback may not enter Core.
    pub(crate) async fn alab_resume_retained_frontier_with_original_policy(
        &mut self,
        native: FrontendFrontier,
        original: OriginalResume<'_>,
    ) -> Result<ProjectedStartedThread> {
        // The original helper has a source-exec forest restoration hook BEFORE
        // its typed request. This retained backend path must never enter it.
        if codex_alab_tui::upgrade_state::pending().is_some() {
            bail!("retained projection cannot restore a source-exec forest");
        }
        let pending = Pending::new(self, native)?;
        self.install_pending(pending.clone())?; // Actual owner BEFORE any original policy await.
        let guard = pending.begin_original_project()?;
        let frontier = pending.native.frontier_data().to_owned();
        let thread_id = original.thread_id;
        let (original, receipt) = retained_frontier::scope_original_resume(
            &pending.handle,
            &frontier,
            &thread_id.to_string(),
            None,
            self.resume_thread_with_permission_overrides(
                original.local_settings,
                original.config,
                thread_id,
                original.model_settings,
                original.permission_overrides,
            ),
        )
        .await?;
        let mut started = original?; // Preserve the original typed policy/conversion error.
        let receipt = receipt.ok_or_else(|| {
            eyre!("original policy did not observe its actual retained projection")
        })?;
        if started.session.thread_id != thread_id {
            bail!("original projected result changed its actual primary");
        }
        pending.observe_original_project(kind::Receipt::Runtime(receipt), guard)?;
        let replay = ProjectionReplay(pending);
        replay.apply_task_tool_observation(self, &mut started)?;
        Ok(ProjectedStartedThread { started, replay })
    }
}

#[path = "owned_local_project_existing.rs"]
pub(crate) mod existing;
