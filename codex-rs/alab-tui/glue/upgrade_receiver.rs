// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Destination-only startup on the actual fresh SDK connection. The retained
//! frame identifies data, never a host, constructor token or continuation grant.
use super::AppServerSession;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::backend_id::BackendId;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::ForestBootstrapRequest;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use color_eyre::eyre::eyre;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::OnceCell;

// Clones retain one attempted admission and one decoded global input forest.
// Repeating a lookup reconciles that same operation rather than recreating it.
struct Prepared {
    backend: BackendId,
    request: ForestBootstrapRequest,
    resumed: std::sync::OnceLock<codex_app_server_client::InProcessAppServerRequestHandle>,
    continuation: std::sync::Arc<dyn codex_alab_core::runtime_upgrade::Continuation>,
}
static REQUEST: OnceCell<Prepared> = OnceCell::const_new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) struct AttachedEditor {
    pub(crate) primary: ThreadId,
    backend: BackendId,
    events: tokio::sync::mpsc::WeakUnboundedSender<AppEvent>,
}
impl AttachedEditor {
    pub(crate) fn new(primary: ThreadId, backend: BackendId, actor: &AppEventSender) -> Self {
        Self {
            primary,
            backend,
            events: actor.app_event_tx.downgrade(),
        }
    }
    pub(crate) fn matches(
        &self,
        primary: ThreadId,
        backend: Option<BackendId>,
        actor: &AppEventSender,
    ) -> bool {
        self.primary == primary
            && Some(self.backend) == backend
            && self.events.upgrade().is_some_and(|original| {
                !original.is_closed() && original.same_channel(&actor.app_event_tx)
            })
    }
}
static ATTACHED: OnceCell<AttachedEditor> = OnceCell::const_new();
static ACTIVATION_FAILURE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static ACTIVATING: AtomicBool = AtomicBool::new(false);

pub(crate) async fn selection(
    session: &AppServerSession,
    action: crate::resume_picker::SessionPickerAction,
    primary: ThreadId,
) -> Result<Option<crate::resume_picker::SessionTarget>> {
    if ACTIVE.load(Ordering::Acquire) {
        return Ok(None);
    }
    let Some(pending) = codex_alab_tui::upgrade_state::pending() else {
        return Ok(None);
    };
    pending.validate_primary(&primary.to_string())?;
    if !matches!(action, crate::resume_picker::SessionPickerAction::Resume)
        || !matches!(
            &session.request_handle,
            AppServerRequestHandle::InProcess(_)
        )
    {
        return Err(eyre!(
            "runtime restoration requires its original local session"
        ));
    }
    let prepared = REQUEST
        .get_or_try_init(|| async {
            let continuation = pending.continuation()?;
            let goals =
                codex_alab_tui::upgrade_state::goal_bytes(pending.bytes(), pending.primary())?;
            let request = ForestBootstrapRequest::from_native_bytes(
                primary,
                goals,
                continuation.clone(),
                pending.deadline(),
            )
            .map_err(|error| eyre!(error))?;
            Ok::<_, color_eyre::eyre::Report>(Prepared {
                backend: session.backend_id(),
                request,
                resumed: std::sync::OnceLock::new(),
                continuation,
            })
        })
        .await?;
    if prepared.backend != session.backend_id() {
        return Err(eyre!("runtime selection changed backend"));
    }
    let cwd = prepared
        .request
        .primary_resume_cwd()
        .ok_or_else(|| eyre!("retained primary has no working directory"))?;
    // Upstream may replace its provisional embedded server after reloading
    // workspace policy. Install every held root only on the final connection.
    Ok(Some(crate::resume_picker::SessionTarget {
        backend_id: prepared.backend,
        thread_id: primary,
        cwd: Some(cwd.to_owned()),
        path: None,
        history_mode: None,
    }))
}

/// Prepare the actual restored session before the final native continuation.
/// Ordinary ThreadResume would replace settings and lose private runtime data.
pub(crate) async fn resume(
    session: &mut AppServerSession,
    local_settings: &crate::local_settings::LocalSettings,
    primary: ThreadId,
) -> Result<Option<super::AppServerStartedThread>> {
    if !restoring() {
        return Ok(None);
    }
    let pending = codex_alab_tui::upgrade_state::pending()
        .ok_or_else(|| eyre!("runtime restoration has no retained state"))?;
    pending.validate_primary(&primary.to_string())?;
    let prepared = REQUEST
        .get()
        .ok_or_else(|| eyre!("runtime resume has no selected forest"))?;
    if prepared.backend != session.backend_id() {
        return Err(eyre!("runtime restoration changed the selected backend"));
    }
    let AppServerRequestHandle::InProcess(handle) = &session.request_handle else {
        return Err(eyre!(
            "runtime restoration requires its final embedded server"
        ));
    };
    let handle = handle.clone();
    let restored = handle
        .bootstrap_upgrade_forest(prepared.request.clone())
        .await?;
    let (_, configured) = restored
        .roots
        .iter()
        .find(|(id, _)| *id == primary)
        .map(|(_, configured)| configured)
        .ok_or_else(|| eyre!("restored forest has no primary session"))?;
    let (current, instruction_sources) = restored
        .root_context(primary)
        .await
        .map_err(|error| eyre!(error))?;
    let thread = session.thread_read(primary, true).await?;
    let cwd = current.cwd().clone();
    let mut state = super::thread_session_state_from_thread_response(
        &thread.id,
        crate::windows_sandbox::host_from_environments(thread.environments.as_deref()),
        thread.forked_from_id.clone(),
        thread.name.clone(),
        thread.path.clone(),
        current.model,
        current.model_provider_id,
        current.service_tier,
        current.approval_policy.into(),
        current.approvals_reviewer,
        current.permission_profile,
        current.active_permission_profile,
        cwd,
        current.workspace_roots,
        instruction_sources,
        current.reasoning_effort,
        local_settings,
    )
    .await
    .map_err(|error| eyre!(error))?;
    state.collaboration_mode = Some(Box::new(current.collaboration_mode));
    state.daybreak_enabled = thread.daybreak_enabled.unwrap_or(false);
    state.network_proxy = configured.network_proxy.as_ref().map(|proxy| {
        crate::session_state::SessionNetworkProxyRuntime {
            http_addr: proxy.http_addr.clone(),
            socks_addr: proxy.socks_addr.clone(),
        }
    });
    session.remember_thread_history_mode(primary, thread.history_mode);
    let task_tools_available = session.task_tools_available(primary);
    if task_tools_available {
        session.remember_task_tool_thread(primary);
    }
    // Activation remains held until the actual App installs this session and
    // replays its pending events, rather than merely constructing a response.
    if let Some(subscribed) = &mut session.attachment_subscribed {
        *subscribed = true;
    }
    prepared.resumed.get_or_init(|| handle);
    Ok(Some(super::AppServerStartedThread {
        #[cfg(unix)]
        alab_construction: None,
        session: state,
        turns: thread.turns,
        blocks_direct_input: false,
        task_tools_available,
    }))
}

/// Called after actual primary-channel attachment and history/event replay.
/// Settlement failures after committed activation must not tear down live work.
pub(crate) async fn attached(
    primary: ThreadId,
    displayed: Option<ThreadId>,
    backend: Option<BackendId>,
    actor: &AppEventSender,
    restore: impl FnOnce(&codex_alab_tui::upgrade_state::Pending) -> Result<()>,
) -> Result<()> {
    if !restoring() {
        return Ok(());
    }
    let pending = codex_alab_tui::upgrade_state::pending()
        .ok_or_else(|| eyre!("runtime attachment lost its retained state"))?;
    pending.validate_primary(&primary.to_string())?;
    if displayed != Some(primary) {
        return Err(eyre!(
            "runtime restoration did not attach its original primary"
        ));
    }
    let prepared = REQUEST
        .get()
        .ok_or_else(|| eyre!("runtime attachment has no prepared forest"))?;
    if prepared.resumed.get().is_none() {
        return Err(eyre!("runtime attachment has no restored primary session"));
    }
    AttachedEditor::install(&ATTACHED, primary, prepared.backend, backend, actor, || {
        restore(pending)
    })
    .await
}

impl AttachedEditor {
    /// Only this exact destination may install the original editor. Publication follows the
    /// fallible install so a retry can recover without overwriting an already attached widget.
    pub(crate) async fn install(
        attachment: &OnceCell<Self>,
        primary: ThreadId,
        expected: BackendId,
        backend: Option<BackendId>,
        actor: &AppEventSender,
        restore: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        if backend != Some(expected) {
            return Err(eyre!("runtime attachment changed the bootstrap backend"));
        }
        let owner = attachment
            .get_or_try_init(|| async {
                if actor.app_event_tx.is_closed() {
                    return Err(eyre!("runtime attachment lost its actor receiver"));
                }
                restore()?;
                Ok::<_, color_eyre::eyre::Report>(Self::new(primary, expected, actor))
            })
            .await?;
        if !owner.matches(primary, backend, actor) {
            return Err(eyre!(
                "runtime attachment changed its original connection or primary"
            ));
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn editor_before_paint() -> Result<Option<(&'static AttachedEditor, bool)>> {
    if let Some(error) = ACTIVATION_FAILURE.get() {
        return Err(eyre!("runtime editor activation failed: {error}"));
    }
    Ok(ATTACHED
        .get()
        .map(|owner| (owner, ACTIVE.load(Ordering::Acquire))))
}

pub(crate) fn restoring() -> bool {
    !ACTIVE.load(Ordering::Acquire) && codex_alab_tui::upgrade_state::pending().is_some()
}

/// Only the actual restored editor's completed layout calls this. Native SDK
/// admission still comes from the retained request handle, never the UI witness.
pub(crate) fn activate_after_layout(
    owner: &AttachedEditor,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    if !ATTACHED
        .get()
        .is_some_and(|attached| std::ptr::eq(attached, owner))
    {
        return;
    }
    let primary = owner.primary;
    let Some(prepared) = REQUEST.get() else {
        return;
    };
    let Some(handle) = prepared.resumed.get() else {
        return;
    };
    if ACTIVATING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        if let Err(error) = handle.activate_upgrade_forest(primary).await {
            let error = ACTIVATION_FAILURE.get_or_init(|| error.to_string());
            done(Err(error.clone()));
            return;
        }
        ACTIVE.store(true, Ordering::Release);
        done(Ok(()));
        settle(prepared.continuation.clone()).await;
    });
}

async fn settle(continuation: std::sync::Arc<dyn codex_alab_core::runtime_upgrade::Continuation>) {
    let mut delay = Duration::from_millis(100);
    loop {
        match continuation.settle().await {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                tracing::debug!(
                    "native settlement could not be confirmed after the SDK connection ended"
                );
                break;
            }
            Err(_) => {}
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}
