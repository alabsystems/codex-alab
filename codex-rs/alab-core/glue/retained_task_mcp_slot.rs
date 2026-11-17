// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One actual retained-host association, shared only by processor clones.
//! A missing uninstalled host preserves ordinary embedded/server behavior. A
//! withdrawn installed host is an error, never permission for local fallback.
use super::*;
use std::sync::OnceLock;

#[derive(Clone, Default)]
pub(crate) struct HostSlot(Arc<OnceLock<Weak<HostControl>>>);
impl HostSlot {
    pub(crate) fn install(&self, host: &Arc<HostControl>) -> io::Result<()> {
        self.0.set(Arc::downgrade(host)).map_err(|_| {
            io::Error::new(io::ErrorKind::AlreadyExists, "task host already installed")
        })
    }
    fn actual(&self) -> io::Result<Option<Arc<HostControl>>> {
        let Some(owner) = self.0.get() else {
            return Ok(None);
        };
        let host = owner.upgrade().ok_or_else(binding::withdrawn)?;
        host.check_open()?;
        Ok(Some(host))
    }
    pub(crate) fn stop(&self) {
        if let Some(owner) = self.0.get().and_then(Weak::upgrade) {
            owner.stop_admission();
        }
    }
    pub(crate) fn prepare_start(
        &self,
        capture: StartCapture,
        supplied: Option<PreparedChildBinding>,
    ) -> io::Result<Option<PreparedChildBinding>> {
        let Some(host) = self.actual()? else {
            if supplied.is_some() {
                return Err(binding::withdrawn());
            }
            return Ok(None);
        };
        let binding = match supplied {
            Some(binding) => binding,
            None => host.prepare_child_binding()?,
        };
        binding.capture(&host, capture)?;
        Ok(Some(binding))
    }
}

impl PreparedChildBinding {
    pub(crate) async fn configure_loaded(&self, config: &mut Config) -> io::Result<()> {
        let host = self.binding.host.upgrade().ok_or_else(binding::withdrawn)?;
        self.configure(&host, config).await
    }
}

impl super::MessageProcessor {
    pub(crate) fn alab_make_task_mcp_host(
        self: &Arc<Self>,
        listener: TcpListener,
        address: std::net::SocketAddr,
        incoming: Incoming,
    ) -> (OwnedTaskMcpHost, io::Result<()>) {
        let host = OwnedTaskMcpHost::from_listener(
            listener,
            address,
            incoming.clone(),
            self.thread_processor.thread_manager.clone(),
            self.alab_backend_create_executor(incoming.clone()),
            BackendReadExecutor::new(self.thread_processor.clone(), incoming.clone()),
            BackendManagementExecutor::new(self.thread_processor.clone(), incoming),
        );
        // Refusal returns the ACTUAL owned host too, so ordinary retained
        // dispatch cleanup joins it instead of hiding a partially started task.
        let installed = host
            .control
            .initial_session
            .set(self.thread_processor.alab_initial_slot())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "task host already has an actual construction owner",
                )
            })
            .and_then(|()| self.thread_processor.alab_install_task_mcp(&host.control));
        (host, installed)
    }
    pub(crate) fn alab_stop_task_mcp(&self) {
        self.thread_processor.alab_stop_task_mcp();
    }
}

impl super::MessageProcessor {
    pub(crate) fn alab_install_worktree_source(
        &self,
        host: &OwnedTaskMcpHost,
        source: codex_alab_task_worktree::SourceConfig,
    ) -> io::Result<()> {
        if !Arc::ptr_eq(&self.thread_processor.thread_manager, &host.control.manager) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "worktree source belongs to another processor",
            ));
        }
        host.control.check_open()?;
        host.control
            .worktrees
            .set(
                crate::request_processors::retained_task_worktree::BackendWorktreeExecutor::new(
                    self.thread_processor.clone(),
                    host.control.incoming.clone(),
                    source,
                ),
            )
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "worktree source already installed",
                )
            })
    }
}

impl HostSlot {
    pub(crate) async fn prepare_fork(
        &self,
        mut capture: ForkConstruction,
        config: &mut Config,
    ) -> io::Result<PreparedChildBinding> {
        let host = self.actual()?.ok_or_else(binding::withdrawn)?;
        let prepared = host.prepare_child_binding()?;
        // The exact constrained setter validates both prospective NEW-task
        // config and the actual original fork config. No JSON equality grants
        // a namespace or reuses a parent's private endpoint/identity.
        prepared
            .configure_loaded(&mut capture.prospective.config)
            .await?;
        prepared.configure_loaded(config).await?;
        capture._resolved = Arc::new(config.clone());
        prepared.capture(&host, capture)?;
        Ok(prepared)
    }
}

impl HostSlot {
    pub(crate) async fn prepare_resume(
        &self,
        mut capture: ResumeConstruction,
        config: &mut Config,
    ) -> io::Result<PreparedChildBinding> {
        let host = self.actual()?.ok_or_else(binding::withdrawn)?;
        let prepared = host.prepare_child_binding()?;
        prepared
            .configure_loaded(&mut capture.prospective.config)
            .await?;
        prepared.configure_loaded(config).await?;
        capture._resolved = Arc::new(config.clone());
        prepared.capture(&host, capture)?;
        Ok(prepared)
    }
    /// Source capability observation only. Match an actual bound Arc in THIS
    /// host's registry; config labels/URLs/bearers never create ownership.
    pub(crate) async fn bound_tools(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> io::Result<bool> {
        let Some(host) = self.actual()? else {
            return Ok(false);
        };
        if actual.has_shutdown_status()
            || !matches!(
                host.manager.thread_registration(id, actual).await,
                ThreadRegistration::Current
            )
        {
            return Err(binding::withdrawn());
        }
        let found = {
            let registry = host.registry.lock().unwrap_or_else(PoisonError::into_inner);
            if registry.closed {
                return Err(binding::withdrawn());
            }
            registry.bindings.values().any(|(binding, _)| {
                binding
                    .parent
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_ref()
                    .is_some_and(|bound| bound.id == id && Arc::ptr_eq(&bound.thread, actual))
            })
        };
        host.check_open()?;
        if actual.has_shutdown_status()
            || !matches!(
                host.manager.thread_registration(id, actual).await,
                ThreadRegistration::Current
            )
            || actual.has_shutdown_status()
        {
            return Err(binding::withdrawn());
        }
        host.check_open()?;
        Ok(found)
    }
}
