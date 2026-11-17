// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Current captured-Core runtime/history reconciliation over the ORIGINAL
//! Started/Fork objects. This performs no construction or Native permission
//! inference. Side-visible history application remains an explicit App owner.
use super::super::ConstructionOwner;
use super::super::Pending;
use super::super::ProjectionReplay;
use super::super::kind;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::AttachmentMetadata;
use crate::app_server_session::HistoryHydrationScope;
use crate::app_server_session::SidebarFork;
use crate::app_server_session::thread_blocks_direct_input;
use crate::app_server_session::thread_session_state_from_thread_resume_response;
use crate::legacy_core::config::Config;
use crate::session_state::ThreadSessionState;
use codex_alab_tui::frontend_control::FrontendSelectionFrontier;
use codex_app_server_client::source_selection;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadResumeResponse;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use std::sync::Arc;

/// Exact original parameter projection and original UI inputs. No default
/// request or local Config is reconstructed by this consumer. `self` below is
/// the SAME original attachment_view used while preparing this request, so its
/// unread/history boundary survives until take_attachment_metadata.
pub(crate) struct ExistingProjectionInputs<'a> {
    pub(crate) request: ClientRequest,
    pub(crate) local_settings: &'a crate::local_settings::LocalSettings,
    pub(crate) config: &'a Config,
}

pub(crate) struct ReconciledPrimary {
    started: AppServerStartedThread,
    replay: ProjectionReplay,
    attachment: AttachmentMetadata,
}
impl ReconciledPrimary {
    pub(crate) fn into_parts(
        self,
    ) -> (AppServerStartedThread, AttachmentMetadata, ProjectionReplay) {
        (self.started, self.attachment, self.replay)
    }
}

/// The ORIGINAL side object retains its boundary/profile/storage/legacy and
/// original display policy. The whole fresh typed response is kept alongside
/// it, including hydrated turns. App must classify/apply that history to the
/// same real store/companion before finishing replay/paint/UI acknowledgment.
pub(crate) struct ReconciledSide {
    original: SidebarFork,
    current: ThreadResumeResponse,
    attachment: AttachmentMetadata,
    replay: ProjectionReplay,
}
impl ReconciledSide {
    pub(crate) fn into_parts(
        self,
    ) -> (
        SidebarFork,
        ThreadResumeResponse,
        AttachmentMetadata,
        ProjectionReplay,
    ) {
        (self.original, self.current, self.attachment, self.replay)
    }
}

/// Actual attached-side Started plus whole fresh typed history/runtime cut.
/// This is nonclone and carries no fabricated creation boundary/storage/legacy.
/// Caller must consume it in the ORIGINAL Incoming/ChannelMode/update path.
pub(crate) struct ReconciledSideAttachment {
    original: AppServerStartedThread,
    current: ThreadResumeResponse,
    attachment: AttachmentMetadata,
    replay: ProjectionReplay,
}
impl ReconciledSideAttachment {
    pub(crate) fn into_parts(
        self,
    ) -> (
        AppServerStartedThread,
        ThreadResumeResponse,
        AttachmentMetadata,
        ProjectionReplay,
    ) {
        (self.original, self.current, self.attachment, self.replay)
    }
}

// Canonical conversion owns current runtime fields; apply only those named
// fields. Never replace ThreadSessionState or overwrite its original local
// message-history/proxy/parent-title fields with the temporary mapper's reads.
struct RuntimeFields {
    canonical: ThreadSessionState,
}
impl RuntimeFields {
    fn validate(&self, original: &ThreadSessionState) -> Result<()> {
        if self.canonical.thread_id != original.thread_id
            || self.canonical.forked_from_id != original.forked_from_id
        {
            bail!("current projection changed original thread/construction lineage");
        }
        Ok(())
    }
    fn apply(self, original: &mut ThreadSessionState, primary: bool) {
        let current = self.canonical;
        original.windows_sandbox_host = current.windows_sandbox_host;
        original.thread_name = current.thread_name;
        original.model = current.model;
        original.model_provider_id = current.model_provider_id;
        original.service_tier = current.service_tier;
        original.approval_policy = current.approval_policy;
        original.approvals_reviewer = current.approvals_reviewer;
        original.permission_profile = current.permission_profile;
        original.active_permission_profile = current.active_permission_profile;
        original.cwd = current.cwd;
        original.runtime_workspace_roots = current.runtime_workspace_roots;
        original.instruction_source_paths = current.instruction_source_paths;
        original.reasoning_effort = current.reasoning_effort;
        original.collaboration_mode = current.collaboration_mode;
        original.rollout_path = current.rollout_path;
        if primary {
            original.daybreak_enabled = current.daybreak_enabled;
        }
        // Identity/lineage were validated; original fork_parent_title,
        // message_history, network_proxy and side's explicit Daybreak=false
        // stay with the original object. Permissions above are display DATA.
    }
}

fn apply_current_fields(
    fields: RuntimeFields,
    original: &mut AppServerStartedThread,
    current: &codex_app_server_protocol::Thread,
    primary: bool,
) {
    original.blocks_direct_input = thread_blocks_direct_input(current);
    fields.apply(&mut original.session, primary);
}

struct ReconciliationGuard {
    pending: Arc<Pending>,
    complete: bool,
}
impl Drop for ReconciliationGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.pending.unknown();
        }
    }
}
struct CapturedProjection {
    current: ThreadResumeResponse,
    attachment: AttachmentMetadata,
}
struct CurrentProjection {
    current: ThreadResumeResponse,
    attachment: AttachmentMetadata,
    replay: ProjectionReplay,
}
impl AppServerSession {
    /// A real initial/later owner already retained by the caller before its
    /// original mutation remains in Pending BEFORE the first projection await.
    /// The returned carrier must enter the original App attachment path with
    /// its real ChannelMode/SubscriptionUpdates; it creates no UI-ready flag.
    pub(crate) async fn alab_reconcile_existing_source_primary(
        &mut self,
        native: FrontendSelectionFrontier,
        owner: ConstructionOwner,
        mut original: AppServerStartedThread,
        inputs: ExistingProjectionInputs<'_>,
    ) -> Result<ReconciledPrimary> {
        let projected = self
            .reconcile_existing_source(native, owner, &mut original, inputs, true)
            .await?;
        // Use the actual freshly hydrated vector, not a second Started object
        // or the stale construction snapshot. Original App installation keeps
        // later SubscriptionUpdates/live prefix precedence unchanged.
        original.turns = projected.current.thread.turns;
        Ok(ReconciledPrimary {
            started: original,
            replay: projected.replay,
            attachment: projected.attachment,
        })
    }
    pub(crate) async fn alab_reconcile_existing_source_side(
        &mut self,
        native: FrontendSelectionFrontier,
        owner: ConstructionOwner,
        mut original: SidebarFork,
        inputs: ExistingProjectionInputs<'_>,
    ) -> Result<ReconciledSide> {
        if original.boundary.is_err()
            || original.alab_retained_replay.is_some()
            || original.thread.session.daybreak_enabled
        {
            bail!("side reconciliation lost its original successful boundary/presentation");
        }
        let projected = self
            .reconcile_existing_source(native, owner, &mut original.thread, inputs, false)
            .await?;
        Ok(ReconciledSide {
            original,
            current: projected.current,
            attachment: projected.attachment,
            replay: projected.replay,
        })
    }
    async fn reconcile_existing_source(
        &mut self,
        native: FrontendSelectionFrontier,
        owner: ConstructionOwner,
        original: &mut AppServerStartedThread,
        inputs: ExistingProjectionInputs<'_>,
        primary: bool,
    ) -> Result<CurrentProjection> {
        let prepared = owner
            .prepared_offer()
            .ok_or_else(|| eyre!("existing projection lost actual construction reservation"))?;
        let pending = Pending::new_source(self, native, owner)?;
        self.install_pending(pending.clone())?;
        let mut outcome = ReconciliationGuard {
            pending: pending.clone(),
            complete: false,
        };
        let project = pending.begin_original_project()?;
        let replay = ProjectionReplay(pending.clone());
        let captured = self
            .alab_project_existing_in_pending(
                pending, project, original, inputs, primary, &prepared,
            )
            .await?;
        outcome.complete = true;
        Ok(CurrentProjection {
            current: captured.current,
            attachment: captured.attachment,
            replay,
        })
    }
    /// Continue the SAME already-owned projection after the original Resume
    /// policy completed its exact parameter/history/maintenance preparation.
    async fn alab_project_existing_in_pending(
        &mut self,
        pending: Arc<Pending>,
        project: super::super::ProjectGuard,
        original: &mut AppServerStartedThread,
        inputs: ExistingProjectionInputs<'_>,
        primary: bool,
        prepared: &codex_app_server_client::ClientConstructionOffer,
    ) -> Result<CapturedProjection> {
        if codex_alab_tui::upgrade_state::pending().is_some() {
            bail!("Source projection cannot restore a source-exec forest");
        }
        let thread_id = original.session.thread_id;
        match &inputs.request {
            ClientRequest::ThreadResume { params, .. }
                if params.thread_id == thread_id.to_string() => {}
            _ => bail!("existing projection requires exact original ThreadResume target/params"),
        }
        if self.attachment_subscribed != Some(false) || !pending.current_on(self) {
            bail!("existing projection lost its same unused attachment view");
        }
        let kind::Native::Source(native) = &pending.native else {
            bail!("existing projection changed actual Native resource");
        };
        let (mut response, receipt) =
            source_selection::project_existing(&pending.handle, native, prepared, inputs.request)
                .await?
                .into_parts();
        if response.thread.id != thread_id.to_string()
            || receipt.thread_id_data() != thread_id.to_string()
            || receipt.is_primary_presentation() != primary
            || !pending.current_on(self)
        {
            bail!("current projection changed original thread/presentation/delivery");
        }
        let complete_memory = receipt.complete_memory_history();
        pending.observe_original_project(kind::Receipt::Source(receipt), project)?;
        // EXACT original policy mapper, with genuine local inputs. Its new
        // local history metadata is deliberately not written over the original.
        let canonical = thread_session_state_from_thread_resume_response(
            &response,
            inputs.local_settings,
            self.thread_params_mode(),
        )
        .await
        .map_err(|error| eyre!(error))?;
        let fields = RuntimeFields { canonical };
        fields.validate(&original.session)?;
        // Only the successful protected response establishes the real listener
        // subscription. This flag is the original hydrator's boundary marker,
        // never Native admission or UI application evidence.
        if let Some(subscribed) = &mut self.attachment_subscribed {
            *subscribed = true;
        }
        if complete_memory {
            self.alab_install_complete_memory_history(&response.thread)?;
        } else {
            self.hydrate_initial_thread_history(
                &mut response.thread,
                response.turns_backwards_cursor.clone(),
                response.items_backwards_cursor.clone(),
                Some(inputs.config),
                Some(inputs.local_settings),
                HistoryHydrationScope::Initial,
            )
            .await?;
        }
        if !pending.current_on(self) {
            bail!("existing projection physical delivery changed during hydration");
        }
        // All fallible mapping/hydration/identity checks precede original field
        // mutation. Exact false replaces any old true on THIS physical tenure.
        pending.apply_task_tool_observation(self, original)?;
        apply_current_fields(fields, original, &response.thread, primary);
        let attachment = self.take_attachment_metadata(thread_id);
        Ok(CapturedProjection {
            current: response,
            attachment,
        })
    }
}

#[cfg(test)]
#[path = "owned_local_project_existing_tests.rs"]
mod tests;

#[path = "owned_local_projected_memory.rs"]
mod memory;

#[path = "owned_local_existing_original.rs"]
pub(crate) mod original;
