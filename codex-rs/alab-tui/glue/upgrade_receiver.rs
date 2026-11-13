// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Destination-only startup on the actual fresh SDK connection. The retained
//! frame identifies data, never a host, constructor token or continuation grant.
use super::AppServerSession;
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
    request: ForestBootstrapRequest,
    handle: codex_app_server_client::InProcessAppServerRequestHandle,
    continuation: std::sync::Arc<dyn codex_alab_core::runtime_upgrade::Continuation>,
}
static REQUEST: OnceCell<Prepared> = OnceCell::const_new();
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) async fn before_lookup(
    session: &AppServerSession,
    action: crate::resume_picker::SessionPickerAction,
    primary: ThreadId,
) -> Result<()> {
    if ACTIVE.load(Ordering::Acquire) || codex_alab_tui::upgrade_state::pending().is_none() {
        return Ok(());
    }
    if !matches!(action, crate::resume_picker::SessionPickerAction::Resume) {
        return Err(eyre!(
            "runtime restoration requires its original local session"
        ));
    }
    bootstrap(session, primary).await?;
    Ok(())
}

async fn bootstrap(
    session: &AppServerSession,
    primary: ThreadId,
) -> Result<codex_app_server_client::BootstrappedForest> {
    let pending = codex_alab_tui::upgrade_state::pending()
        .ok_or_else(|| eyre!("runtime restoration has no retained state"))?;
    pending.validate_primary(&primary.to_string())?;
    let AppServerRequestHandle::InProcess(handle) = &session.request_handle else {
        return Err(eyre!(
            "runtime restoration requires its fresh embedded runtime"
        ));
    };
    let request = REQUEST
        .get_or_try_init(|| async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            let host = codex_alab_core::native_source_launch::runtime_upgrade()?;
            let continuation =
                tokio::time::timeout_at(deadline, host.load(pending.primary(), pending.bytes()))
                    .await
                    .map_err(|_| eyre!("native runtime load deadline elapsed"))??;
            let goals =
                codex_alab_tui::upgrade_state::goal_bytes(pending.bytes(), pending.primary())?;
            let request = ForestBootstrapRequest::from_native_bytes(
                primary,
                goals,
                continuation.clone(),
                deadline,
            )
            .map_err(|error| eyre!(error))?;
            Ok::<_, color_eyre::eyre::Report>(Prepared {
                request,
                handle: handle.clone(),
                continuation,
            })
        })
        .await?;
    Ok(handle
        .bootstrap_upgrade_forest(request.request.clone())
        .await?)
}

/// Prepare the actual restored session before the final native continuation.
/// Ordinary ThreadResume would replace settings and lose private runtime data.
pub(crate) async fn resume(
    session: &mut AppServerSession,
    local_settings: &crate::local_settings::LocalSettings,
    primary: ThreadId,
) -> Result<Option<super::AppServerStartedThread>> {
    if ACTIVE.load(Ordering::Acquire) || codex_alab_tui::upgrade_state::pending().is_none() {
        return Ok(None);
    }
    let restored = bootstrap(session, primary).await?;
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
    Ok(Some(super::AppServerStartedThread {
        session: state,
        turns: thread.turns,
        blocks_direct_input: false,
        task_tools_available,
    }))
}

/// Called after actual primary-channel attachment and history/event replay.
/// Settlement failures after committed activation must not tear down live work.
pub(crate) async fn attached(primary: ThreadId, displayed: Option<ThreadId>) -> Result<()> {
    if ACTIVE.load(Ordering::Acquire) || codex_alab_tui::upgrade_state::pending().is_none() {
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
    prepared.handle.activate_upgrade_forest(primary).await?;
    ACTIVE.store(true, Ordering::Release);
    // This owned cleanup retains the constructor's exact stream. Neither a
    // timeout nor a lost reply is permission to synthesize another claim.
    let continuation = prepared.continuation.clone();
    tokio::spawn(async move {
        let mut delay = Duration::from_millis(100);
        loop {
            match continuation.settle().await {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                    tracing::debug!(
                        "native settlement connection ended; recovery custody remains retained"
                    );
                    break;
                }
                Err(_) => {}
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(30));
        }
    });
    Ok(())
}
