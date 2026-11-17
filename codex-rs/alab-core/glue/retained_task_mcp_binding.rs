// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Opaque host-issued transport binding to an exact Core runtime. Neither an
//! HTTP thread label nor serialized endpoint data can construct this object.
use super::*;
use codex_config::McpServerConfig;
use codex_features::Features;
use codex_protocol::mcp::ClientMcpExtensions;

pub(crate) const NAMESPACE: &str = "codex_tui";

#[derive(Clone)]
pub(crate) struct StartCapture {
    pub(crate) baseline: codex_app_server_protocol::ThreadStartParams,
    pub(crate) app_server_client_name: Option<String>,
    pub(crate) app_server_client_version: Option<String>,
    pub(crate) client_mcp_extensions: ClientMcpExtensions,
}

pub(super) struct BoundThread {
    pub(super) id: ThreadId,
    pub(super) thread: Arc<CodexThread>,
    pub(super) features: Features,
}

pub(crate) struct PreparedChildBinding {
    pub(super) binding: Arc<ThreadBinding>,
}

pub(super) struct ThreadBinding {
    pub(super) key: Uuid,
    pub(super) authorization: String,
    pub(super) config: Value,
    pub(super) host: Weak<HostControl>,
    pub(super) parent: std::sync::RwLock<Option<BoundThread>>,
    pub(super) capture: std::sync::RwLock<Option<ConstructionCapture>>,
    pub(super) leases: std::sync::atomic::AtomicUsize,
    pub(super) fork_provider: std::sync::OnceLock<Arc<dyn codex_mcp::McpToolRequestMetaProvider>>,
}

impl Clone for PreparedChildBinding {
    fn clone(&self) -> Self {
        self.binding
            .leases
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            binding: self.binding.clone(),
        }
    }
}
impl Drop for PreparedChildBinding {
    fn drop(&mut self) {
        if self
            .binding
            .leases
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
            == 1
        {
            self.abandon_before_birth();
        }
    }
}

impl PreparedChildBinding {
    pub(crate) fn config(&self) -> Value {
        self.binding.config.clone()
    }

    pub(crate) fn capture(
        &self,
        host: &Arc<HostControl>,
        capture: impl Into<ConstructionCapture>,
    ) -> io::Result<()> {
        self.check_host(host)?;
        let mut slot = self
            .binding
            .capture
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "task startup inputs already captured",
            ));
        }
        *slot = Some(capture.into());
        Ok(())
    }

    fn check_host(&self, host: &Arc<HostControl>) -> io::Result<()> {
        let actual = self.binding.host.upgrade().ok_or_else(withdrawn)?;
        if !Arc::ptr_eq(host, &actual) {
            return Err(withdrawn());
        }
        host.check_open()?;
        let registry = host.registry.lock().unwrap_or_else(PoisonError::into_inner);
        if !registry
            .bindings
            .get(&self.binding.key)
            .is_some_and(|(binding, _)| Arc::ptr_eq(binding, &self.binding))
        {
            return Err(withdrawn());
        }
        Ok(())
    }

    /// Called inside the shared START body on the actual loaded Config. The
    /// original namespace remains visible until this exact typed owner has
    /// checked collisions; no serialized JSON equality grants ownership.
    pub(crate) async fn configure(
        &self,
        host: &Arc<HostControl>,
        config: &mut Config,
    ) -> io::Result<()> {
        self.check_host(host)?;
        if host
            .manager
            .mcp_manager()
            .configured_servers(config)
            .await
            .contains_key(NAMESPACE)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "configured MCP server owns task namespace",
            ));
        }
        self.check_host(host)?;
        let intended: McpServerConfig = serde_json::from_value(self.config()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid private task server config",
            )
        })?;
        let mut servers = config.mcp_servers.get().clone();
        servers.insert(NAMESPACE.to_owned(), intended.clone());
        config.mcp_servers.set(servers).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "managed MCP policy refused task server",
            )
        })?;
        if config.mcp_servers.get().get(NAMESPACE) != Some(&intended) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "managed MCP policy changed private task registration",
            ));
        }
        self.check_host(host)
    }

    /// The startup owner supplies the exact newly returned child ID and Arc.
    /// Catalog discovery may precede this, but operation admission may not.
    pub(crate) async fn bind_created(
        &self,
        id: ThreadId,
        thread: Arc<CodexThread>,
    ) -> io::Result<()> {
        let host = self.binding.host.upgrade().ok_or_else(withdrawn)?;
        self.check_host(&host)?;
        if thread.has_shutdown_status()
            || !matches!(
                host.manager.thread_registration(id, &thread).await,
                ThreadRegistration::Current
            )
        {
            return Err(withdrawn());
        }
        self.install_fork_origin(&thread).await?;
        let features = thread.config().await.features.get().clone();
        self.check_host(&host)?;
        let inserted = {
            let mut slot = self
                .binding
                .parent
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            match slot.as_ref() {
                Some(current) if current.id == id && Arc::ptr_eq(&current.thread, &thread) => false,
                Some(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "task binding belongs to another runtime",
                    ));
                }
                None => {
                    *slot = Some(BoundThread {
                        id,
                        thread: thread.clone(),
                        features,
                    });
                    true
                }
            }
        };
        if inserted {
            let binding = self.binding.clone();
            let owner = host.clone();
            host.monitors.spawn(async move {
                // This is the public exact-runtime termination observation,
                // not a consuming event reader, sleep or thread-ID probe.
                thread.wait_until_terminated().await;
                owner.retire_binding(&binding);
                owner.retire_operations_for_terminated(&thread).await;
            });
        }
        Ok(())
    }

    pub(crate) fn abandon_before_birth(&self) {
        if self
            .binding
            .parent
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
            && let Some(host) = self.binding.host.upgrade()
        {
            host.retire_binding(&self.binding);
        }
    }
}

impl ThreadBinding {
    pub(super) async fn actual_parent(
        &self,
    ) -> io::Result<(ThreadId, Arc<CodexThread>, ConstructionCapture, Features)> {
        let host = self.host.upgrade().ok_or_else(withdrawn)?;
        host.check_open()?;
        let (id, thread, features) = {
            let slot = self.parent.read().unwrap_or_else(PoisonError::into_inner);
            let bound = slot.as_ref().ok_or_else(|| {
                io::Error::new(io::ErrorKind::WouldBlock, "task runtime not yet bound")
            })?;
            (bound.id, bound.thread.clone(), bound.features.clone())
        };
        if thread.has_shutdown_status()
            || !matches!(
                host.manager.thread_registration(id, &thread).await,
                ThreadRegistration::Current
            )
        {
            return Err(withdrawn());
        }
        host.check_open()?;
        let capture = self
            .capture
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(withdrawn)?;
        Ok((id, thread, capture, features))
    }
}

pub(super) fn withdrawn() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "retained task owner is unavailable",
    )
}

#[path = "retained_task_construction_capture.rs"]
mod construction;
pub(crate) use construction::ConstructionCapture;
pub(crate) use construction::ForkConstruction;
pub(crate) use construction::ResumeConstruction;
