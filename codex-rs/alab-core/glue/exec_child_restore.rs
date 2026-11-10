// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cold-load captured descendants into their replacement parent's local runtime.
//! This prepares an idle runtime. Original-turn execution needs separate live admission.
use crate::CodexThread;
use crate::CodexThreadSettingsOverrides;
use crate::ExecRecoveryOptions;
use crate::ExecThreadSnapshot;
use crate::ExecTreeSnapshot;
use crate::PreparedExecIdleInput;
use crate::PreparedExecInputBatch;
use crate::StartThreadOptions;
use crate::session::input_queue::exec_input_state;
use codex_alab_core::quota::HeadlessExec;
use codex_extension_api::ExtensionDataInit;
use codex_history::InitialHistory;
use codex_history::ResumedHistory;
use codex_protocol::ThreadId;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

#[path = "exec_child_identity.rs"]
mod identity;

/// Volatile provenance for a cold-loaded descendant in an exec host.
/// This marker does not authorize execution or bypass original-turn admission.
#[derive(Clone, Copy)]
pub struct ExecRestoredMember;

type RestoreResult = Result<Arc<ExecRestoredChild>, String>;

/// The loaded child is still idle. A caller retains this pin through activation,
/// then releases it so ordinary residency management can resume.
pub struct ExecRestoredChild {
    thread: Arc<CodexThread>,
    recovery: Option<ExecRecoveryOptions>,
    idle: Mutex<Option<PreparedExecIdleInput>>,
    idle_outcome: OnceLock<tokio::sync::watch::Receiver<Option<Result<(), String>>>>,
    pin: Mutex<Option<tokio::sync::OwnedRwLockReadGuard<()>>>,
}
impl ExecRestoredChild {
    pub fn thread(&self) -> &Arc<CodexThread> {
        &self.thread
    }
    pub fn recovery(&self) -> Option<&ExecRecoveryOptions> {
        self.recovery.as_ref()
    }
    /// Wake ordinary mailbox delivery only after the tree's launch is released.
    pub fn activate_idle_inputs(&self) -> Result<(), String> {
        let outcome = self
            .idle_outcome
            .get()
            .ok_or("idle input was not restored")?;
        match outcome.borrow().as_ref() {
            Some(Ok(())) => exec_input_state::activate_idle(&self.thread.session),
            _ => Err("idle input restoration did not succeed".into()),
        }
    }
    /// Restore an idle member's retained mail after fresh native admission.
    /// Does not wake delivery or release residency; bootstrap must install
    /// every idle queue before activating children and releasing their pins.
    /// A cancelled waiter cannot abandon an admitted installation; retries of this
    /// operation report its recorded result without enqueuing the mail again.
    pub async fn restore_idle_inputs(
        self: &Arc<Self>,
        admit: impl FnOnce(
            ThreadId,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
        > + Send
        + 'static,
    ) -> Result<(), String> {
        let owner = Arc::clone(self);
        let mut outcome = self
            .idle_outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                tokio::spawn(async move {
                    let result = async {
                        if owner.recovery.is_some()
                            || owner
                                .idle
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .is_none()
                        {
                            return Err("child has no retained idle input".into());
                        }
                        let _pin = Arc::clone(&owner.thread.residency_gate).read_owned().await;
                        let state = owner
                            .thread
                            .session
                            .services
                            .local_agent_runtime
                            .upgrade()
                            .map_err(|e| e.to_string())?;
                        if !state
                            .threads
                            .read()
                            .await
                            .get(&owner.thread.session.thread_id)
                            .is_some_and(|current| Arc::ptr_eq(current, &owner.thread))
                            || !owner.thread.is_running()
                        {
                            return Err("idle input owner is no longer registered".into());
                        }
                        admit(owner.thread.session.thread_id).await?;
                        let input = owner
                            .idle
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take()
                            .ok_or("idle input was already taken")?;
                        exec_input_state::install_idle(&owner.thread.session, input).await?;
                        Ok(())
                    }
                    .await;
                    let _ = send.send_replace(Some(result));
                });
                receive
            })
            .clone();
        loop {
            if let Some(result) = outcome.borrow_and_update().clone() {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| "idle restoration owner exited without a result".to_owned())?;
        }
    }
    pub fn release_residency_pin(&self) {
        self.pin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

pub struct ExecPreparedChildRestore {
    plan: Mutex<Option<Plan>>,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<RestoreResult>>>,
}
struct Plan {
    parent: Arc<CodexThread>,
    root: ThreadId,
    turn: String,
    node: ExecThreadSnapshot,
    fresh: CodexThreadSettingsOverrides,
    roots: Vec<SelectedCapabilityRoot>,
    init: ExtensionDataInit,
    idle: Option<PreparedExecIdleInput>,
}

impl CodexThread {
    pub async fn verify_exec_restored_snapshot(
        &self,
        node: &ExecThreadSnapshot,
        startup: &codex_protocol::protocol::ThreadSettingsSnapshot,
    ) -> Result<(), String> {
        if self.session.thread_id != node.thread() {
            return Err("restored snapshot changed thread identity".into());
        }
        self.flush_rollout()
            .await
            .map_err(|error| error.to_string())?;
        let history = self
            .exec_recovery_history()
            .await
            .map_err(|error| error.to_string())?;
        verify_loaded_history(
            node,
            &history,
            &[startup.clone(), self.thread_settings_snapshot().await],
        )
    }

    /// Stop a failed destination and join all members of its local runtime.
    pub async fn shutdown_exec_tree(&self) -> Result<(), String> {
        let shutdown = self.session.services.local_agent_runtime.request_shutdown();
        let root = self
            .shutdown_and_wait()
            .await
            .map_err(|error| error.to_string());
        let members = shutdown.wait().await.map_err(|error| error.to_string());
        root.and(members)
    }

    pub fn exec_thread_id(&self) -> ThreadId {
        self.session.thread_id
    }

    /// Fresh parent bindings for validating a captured child's settings.
    pub async fn exec_child_restore_bindings(
        &self,
    ) -> (CodexThreadSettingsOverrides, Vec<SelectedCapabilityRoot>) {
        (
            self.restorable_thread_settings().await,
            self.session.services.selected_capability_roots.clone(),
        )
    }

    /// Prepare one child from an internally validated tree while the replacement
    /// root still holds its pending original input. Nothing starts a model turn.
    pub async fn prepare_exec_child_restore(
        &self,
        tree: &ExecTreeSnapshot,
        child: ThreadId,
        fresh: CodexThreadSettingsOverrides,
        fresh_roots: Vec<SelectedCapabilityRoot>,
        inputs: &mut PreparedExecInputBatch,
    ) -> Result<ExecPreparedChildRestore, String> {
        tree.validate(tree.root(), tree.turn())?;
        let node = tree
            .threads()
            .iter()
            .find(|node| node.thread() == child)
            .ok_or("restored child is absent from the captured tree")?
            .clone();
        if child == tree.root() || node.parent() != Some(self.session.thread_id) {
            return Err("restored child does not belong to this parent".into());
        }
        let runtime = &self.session.services.local_agent_runtime;
        let state = runtime.upgrade().map_err(|e| e.to_string())?;
        let parent = {
            let threads = state.threads.read().await;
            let parent = threads
                .get(&self.session.thread_id)
                .filter(|parent| std::ptr::eq(parent.as_ref(), self))
                .cloned()
                .ok_or("replacement parent is no longer registered")?;
            validate_owner(&parent, &threads, tree.root(), tree.turn())?;
            if threads.contains_key(&child) {
                return Err("restored child is already loaded".into());
            }
            parent
        };
        node.restore_settings(&fresh, &fresh_roots)?;
        // Do all fallible preparation before taking a frame from the shared table.
        let current = parent.session.get_config().await;
        node.configuration()
            .restore(&current)
            .await
            .map_err(|e| e.to_string())?;
        let (init, idle) = if let Some(recovery) = node.recovery() {
            let options = inputs.prepare_start(
                StartThreadOptions::new((*current).clone()),
                child,
                recovery.turn(),
            )?;
            (options.thread_extension_init, None)
        } else {
            (ExtensionDataInit::new(), Some(inputs.take_idle(child)?))
        };
        Ok(ExecPreparedChildRestore {
            plan: Mutex::new(Some(Plan {
                parent,
                root: tree.root(),
                turn: tree.turn().into(),
                node,
                fresh,
                roots: fresh_roots,
                init,
                idle,
            })),
            outcome: OnceLock::new(),
        })
    }
}

fn validate_owner(
    parent: &Arc<CodexThread>,
    threads: &std::collections::HashMap<ThreadId, Arc<CodexThread>>,
    root: ThreadId,
    turn: &str,
) -> Result<(), String> {
    let registered = threads
        .get(&parent.session.thread_id)
        .is_some_and(|current| Arc::ptr_eq(current, parent));
    let root_thread = threads.get(&root).ok_or("replacement root is not loaded")?;
    let runtime = &parent.session.services.local_agent_runtime;
    if !registered
        || !parent.is_running()
        || !root_thread.is_running()
        || !Arc::ptr_eq(
            &runtime.registry,
            &root_thread.session.services.local_agent_runtime.registry,
        )
        || exec_input_state::pending_origin(&root_thread.session) != Some((root, turn.into()))
    {
        return Err("replacement tree has no matching pending original-root input".into());
    }
    Ok(())
}

// Apply the same owner checks used by normal V2 child reload. Captured ready
// configurations are evidence, never authority over a replacement executor.
async fn bound_environments(
    parent: &CodexThread,
    settings: &mut CodexThreadSettingsOverrides,
) -> Result<(), String> {
    use crate::config::PermissionProfileSnapshot;
    use codex_protocol::protocol::EnvironmentConfigState;
    let Some(requests) = settings.environments.as_mut() else {
        return Ok(());
    };
    let owners = parent
        .session
        .inherited_environment_snapshot()
        .await
        .refresh_readiness();
    for request in &mut requests.environment_requests {
        let owner = owners
            .turn_environments()
            .find(|owner| {
                owner.selection.environment_id == request.environment_id
                    && owner.selection.cwd == request.cwd
                    && owner.selection.workspace_roots == request.workspace_roots
            })
            .ok_or("restored child environment has no fresh owner binding")?;
        let current = owner.config();
        let child = match &request.config {
            EnvironmentConfigState::FromThread => {
                request.config = EnvironmentConfigState::Ready(current.clone());
                continue;
            }
            EnvironmentConfigState::Ready(child) => child,
            _ => return Err("restored child environment is not ready".into()),
        };
        let mut bounded = child.clone();
        bounded.permission_profile = current.permission_profile.clone();
        if bounded != *current {
            return Err("restored child environment differs from its fresh owner".into());
        }
        if child.permission_profile == current.permission_profile {
            continue;
        }
        if owner.environment().is_remote() {
            return Err("restored remote environment permissions require fresh validation".into());
        }
        let cwd = request.cwd.to_abs_path().map_err(|e| e.to_string())?;
        let roots = owner
            .workspace_roots()
            .iter()
            .map(codex_utils_path_uri::PathUri::to_abs_path)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let authority = owner
            .permission_profile()
            .clone()
            .materialize_project_roots_with_workspace_roots(&roots);
        let requested = child
            .permission_profile
            .permission_profile()
            .clone()
            .materialize_project_roots_with_workspace_roots(&roots);
        let permissions =
            codex_protocol::intersect_effective_permission_profiles(&authority, &requested, &cwd)
                .map_err(|e| e.to_string())?;
        bounded.permission_profile = PermissionProfileSnapshot::legacy(permissions);
        request.config = EnvironmentConfigState::Ready(bounded);
    }
    Ok(())
}

// Resume can append effective-settings records before a turn is admitted. The
// captured prefix stays exact; only this runtime's observed settings may follow.
fn verify_loaded_history(
    node: &ExecThreadSnapshot,
    history: &codex_thread_store::StoredThreadHistory,
    settings: &[codex_protocol::protocol::ThreadSettingsSnapshot],
) -> Result<(), String> {
    let count = node.history()?.items.len();
    let Some(prefix) = history.items.get(..count) else {
        return Err("child startup lost captured history".into());
    };
    node.verify_history(&codex_thread_store::StoredThreadHistory {
        thread_id: history.thread_id,
        items: prefix.to_vec(),
        revision: None,
    })?;
    for item in &history.items[count..] {
        match item {
            codex_history::RolloutItem::EventMsg(
                codex_protocol::protocol::EventMsg::ThreadSettingsApplied(applied),
            ) if applied.thread_id == Some(node.thread())
                && settings.contains(&applied.thread_settings) => {}
            _ => return Err("child startup appended unexpected history".into()),
        }
    }
    Ok(())
}

impl ExecPreparedChildRestore {
    /// Once started, a dropped waiter does not abandon a half-created child.
    /// Repeated calls observe the same result; they never create another runtime.
    pub async fn start(&self) -> RestoreResult {
        let mut outcome = self
            .outcome
            .get_or_init(|| {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                let plan = self
                    .plan
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                tokio::spawn(async move {
                    let result = match plan {
                        Some(plan) => plan.run().await.map(Arc::new),
                        None => Err("child restoration plan was already taken".into()),
                    };
                    let _ = sender.send_replace(Some(result));
                });
                receiver
            })
            .clone();
        loop {
            let result = { outcome.borrow_and_update().clone() };
            if let Some(result) = result {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| "child restoration owner exited without a result".to_owned())?;
        }
    }
}

impl Plan {
    async fn run(self) -> Result<ExecRestoredChild, String> {
        let runtime = &self.parent.session.services.local_agent_runtime;
        let _start = runtime.admit_exec_tree_start().map_err(|e| e.to_string())?;
        let membership = runtime.admit_start().map_err(|e| e.to_string())?;
        let state = runtime.upgrade().map_err(|e| e.to_string())?;
        let _parent_pin = Arc::clone(&self.parent.residency_gate).read_owned().await;
        {
            let threads = state.threads.read().await;
            validate_owner(&self.parent, &threads, self.root, &self.turn)?;
            if threads.contains_key(&self.node.thread()) {
                return Err("restored child is already loaded".into());
            }
        }
        if self.roots.iter().any(|root| {
            !self
                .parent
                .session
                .services
                .selected_capability_roots
                .contains(root)
        }) {
            return Err("restored child requests a capability outside its new parent".into());
        }
        let parent_config = self.parent.session.get_config().await;
        let mut config = self
            .node
            .configuration()
            .restore(&parent_config)
            .await
            .map_err(|e| e.to_string())?;
        if !crate::exec_policy::child_uses_parent_exec_policy(&parent_config, &config) {
            return Err("restored child no longer shares its parent's execution policy".into());
        }
        let mut settings = self.node.restore_settings(&self.fresh, &self.roots)?;
        bound_environments(&self.parent, &mut settings).await?;
        let parent_settings = self.parent.config_snapshot().await;
        let authority = parent_settings
            .permission_profile
            .materialize_project_roots_with_workspace_roots(&parent_settings.workspace_roots);
        let requested = settings
            .permission_profile
            .as_ref()
            .unwrap_or(config.permissions.permission_profile())
            .clone()
            .materialize_project_roots_with_workspace_roots(&config.workspace_roots);
        let bounded = codex_protocol::intersect_effective_permission_profiles(
            &authority,
            &requested,
            config.cwd.as_path(),
        )
        .map_err(|e| e.to_string())?;
        if settings.permission_profile.as_ref() != Some(&bounded) {
            settings.active_permission_profile = None;
        }
        config
            .permissions
            .set_permission_profile(bounded.clone())
            .map_err(|e| e.to_string())?;
        settings.permission_profile = Some(bounded);
        settings.sandbox_policy = None;
        let available_tools = self.parent.session.dynamic_tools().await;
        if self
            .node
            .dynamic_tools()
            .iter()
            .any(|tool| !available_tools.contains(tool))
        {
            return Err("captured dynamic tools are absent from the replacement parent".into());
        }
        let mut options = StartThreadOptions::new(config.clone());
        options.thread_extension_init = self.init;
        if let Some(memory) = self.node.ephemeral_history() {
            options = match self.node.recovery() {
                Some(recovery) => {
                    memory
                        .clone()
                        .prepare_start(options, self.node.thread(), recovery.turn())?
                }
                None => memory
                    .clone()
                    .prepare_idle_start(options, self.node.thread())?,
            };
        } else {
            let stored = state
                .read_stored_thread(codex_thread_store::ReadThreadParams {
                    thread_id: self.node.thread(),
                    include_archived: true,
                    include_history: false,
                })
                .await
                .map_err(|e| e.to_string())?;
            if stored.archived_at.is_some()
                || stored.parent_thread_id != Some(self.parent.session.thread_id)
                || stored.source
                    != match self.node.source() {
                        SessionSource::Internal(
                            codex_protocol::protocol::InternalSessionSource::Review,
                        ) => SessionSource::SubAgent(SubAgentSource::Review),
                        source => source.clone(),
                    }
            {
                return Err("stored child identity or parent changed before restoration".into());
            }
            let evidence = self
                .parent
                .session
                .services
                .thread_store
                .load_latest_turn_history(codex_thread_store::LoadThreadHistoryParams {
                    thread_id: self.node.thread(),
                    include_archived: false,
                })
                .await
                .map_err(|e| e.to_string())?;
            self.node
                .verify_history(&evidence)
                .map_err(|error| format!("before startup: {error}"))?;
            let history = crate::agent::history::load_agent_model_context(
                &state,
                self.node.thread(),
                stored.history_mode,
            )
            .await
            .map_err(|e| e.to_string())?
            .ok_or("restored child has no model context")?;
            options.initial_history = InitialHistory::Resumed(ResumedHistory {
                conversation_id: self.node.thread(),
                history: Arc::new(history.items),
                history_revision: history.revision,
                rollout_path: stored.rollout_path,
            });
            options.history_mode = Some(stored.history_mode);
        }
        options.session_source = Some(self.node.source().clone());
        if matches!(
            self.node.source(),
            SessionSource::Internal(codex_protocol::protocol::InternalSessionSource::Review)
        ) {
            crate::thread_manager::alab_exec_review::bind_restore_parent(
                &mut options.thread_extension_init,
                &self.parent.session,
            );
        }
        options.environments = settings
            .environments
            .as_ref()
            .map(|envs| envs.environment_requests.clone());
        options.inherited_environments =
            Some(self.parent.session.inherited_environment_snapshot().await);
        options.dynamic_tools = self.node.dynamic_tools().to_vec();
        options.disabled_plugin_ids = settings.disabled_plugin_ids.clone();
        options.thread_extension_init.insert(HeadlessExec);
        options.thread_extension_init.insert(ExecRestoredMember);
        options.turn_extension_init.insert(HeadlessExec);
        let control = runtime.control(self.parent.session.session_id());
        let is_v2 = matches!(
            self.node.source(),
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })
        ) && self.parent.multi_agent_version() == Some(MultiAgentVersion::V2);
        let slot = if is_v2 {
            identity::restore(
                &runtime.registry,
                self.node.thread(),
                self.node.source(),
                self.node.ephemeral_history().is_some(),
            )?;
            Some(
                control
                    .reserve_v2_residency_slot(
                        &state,
                        &config,
                        &membership,
                        Some(self.node.thread()),
                    )
                    .await
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let started = state
            .spawn_child_thread(
                options,
                control,
                Some(self.parent.session.thread_id),
                Some(Arc::clone(&self.parent.session.services.exec_policy)),
            )
            .await
            .map_err(|e| e.to_string())?;
        let child = started.thread;
        crate::thread_manager::alab_exec_review::watch_private_lifetime(&state, &child);
        let checked = async {
            if child.session.thread_id != self.node.thread()
                || !Arc::ptr_eq(
                    &runtime.registry,
                    &child.session.services.local_agent_runtime.registry,
                )
            {
                return Err("child startup changed its identity or runtime".to_owned());
            }
            let startup_settings = child.thread_settings_snapshot().await;
            child
                .restore_thread_settings(settings)
                .await
                .map_err(|e| e.to_string())?;
            if child.session.services.selected_capability_roots != self.roots {
                return Err("child startup changed capability selection".into());
            }
            child.flush_rollout().await.map_err(|e| e.to_string())?;
            let current = child
                .exec_recovery_history()
                .await
                .map_err(|e| e.to_string())?;
            verify_loaded_history(
                &self.node,
                &current,
                &[startup_settings, child.thread_settings_snapshot().await],
            )?;
            if self.node.recovery().is_none() {
                crate::session::alab_exec_recovery_options::restore_idle_status(
                    &child.session,
                    &current.items,
                )
                .await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = checked {
            discard_failed_child(&state, &child).await;
            return Err(error);
        }
        let pin = if is_v2 {
            match runtime.pin_v2_residency(&child).await {
                Ok(pin) => pin,
                Err(error) => {
                    discard_failed_child(&state, &child).await;
                    return Err(error.to_string());
                }
            }
        } else {
            None
        };
        if let Some(slot) = slot {
            slot.commit(child.session.thread_id);
        }
        Ok(ExecRestoredChild {
            thread: child,
            recovery: self.node.recovery().cloned(),
            idle: Mutex::new(self.idle),
            idle_outcome: OnceLock::new(),
            pin: Mutex::new(pin),
        })
    }
}

async fn discard_failed_child(
    state: &crate::thread_manager::ThreadManagerState,
    child: &Arc<CodexThread>,
) {
    let _ = child.shutdown_and_wait().await;
    let mut threads = state.threads.write().await;
    if threads
        .get(&child.session.thread_id)
        .is_some_and(|loaded| Arc::ptr_eq(loaded, child))
    {
        threads.remove(&child.session.thread_id);
        child.session.services.mcp_runtime.revoke_ema();
    }
}

#[cfg(test)]
#[path = "exec_child_restore_tests.rs"]
mod tests;
