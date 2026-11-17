// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private Source-call correlation, supplied only by Core's real call hook.
//! It confers no Native role, SDK authority or effects/currentness receipt.
use super::*;
use codex_mcp::McpServerSource;
use codex_mcp::McpToolRequestMetaHook;
use codex_mcp::McpToolRequestMetaProvider;
use futures::future::BoxFuture;
use std::time::Duration;
use std::time::Instant;

pub(super) const META_KEY: &str = "x-alab-task-fork-invocation";
const PENDING_LIFETIME: Duration = Duration::from_secs(300);

pub(super) struct ForkOrigin {
    pub(super) binding: Weak<ThreadBinding>,
    pub(super) binding_id: Uuid,
    pub(super) parent_id: ThreadId,
    pub(super) parent: Arc<CodexThread>,
    pub(super) turn: String,
    pub(super) call: String,
    issued: Instant,
}
pub(super) struct ForkEntry {
    pub(super) origin: Arc<ForkOrigin>,
    pub(super) accepted: Option<(
        crate::request_processors::retained_task_fork::ForkArguments,
        Arc<fork::AcceptedFork>,
    )>,
}
impl ForkEntry {
    pub(super) fn pending_expired(&self, now: Instant) -> bool {
        self.accepted.is_none()
            && now.saturating_duration_since(self.origin.issued) >= PENDING_LIFETIME
    }
    pub(super) fn retired_owner(&self) -> bool {
        self.accepted
            .as_ref()
            .is_none_or(|(_, accepted)| accepted.definite_for_retirement())
    }
}

struct Provider {
    binding: Weak<ThreadBinding>,
}
impl McpToolRequestMetaProvider for Provider {
    fn value<'a>(
        &'a self,
        thread_id: ThreadId,
        turn_id: &'a str,
        call_id: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Value>> {
        Box::pin(async move {
            let binding = self.binding.upgrade().ok_or_else(binding::withdrawn)?;
            let host = binding.host.upgrade().ok_or_else(binding::withdrawn)?;
            let (id, parent, _, _) = binding.actual_parent().await?;
            anyhow::ensure!(
                id == thread_id && !turn_id.is_empty() && !call_id.is_empty(),
                "fork issuing Core context disagrees with actual binding"
            );
            let now = Instant::now();
            let mut registry = host.registry.lock().unwrap_or_else(PoisonError::into_inner);
            let retired = expire_pending(&mut registry, now);
            let result = (|| -> io::Result<Value> {
                validate_bound(&host, &registry, &binding, id, &parent)?;
                if let Some((claim, _)) = registry.fork_calls.iter().find(|(_, entry)| {
                    entry.origin.binding_id == binding.key
                        && entry.origin.turn == turn_id
                        && entry.origin.call == call_id
                        && Arc::ptr_eq(&entry.origin.parent, &parent)
                }) {
                    return Ok(Value::String(claim.to_string()));
                }
                if registry.fork_calls.len() >= MAX_ACCEPTED {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "fork invocation capacity exhausted",
                    ));
                }
                let claim = Uuid::new_v4();
                registry.fork_calls.insert(
                    claim,
                    ForkEntry {
                        origin: Arc::new(ForkOrigin {
                            binding: Arc::downgrade(&binding),
                            binding_id: binding.key,
                            parent_id: id,
                            parent,
                            turn: turn_id.to_owned(),
                            call: call_id.to_owned(),
                            issued: now,
                        }),
                        accepted: None,
                    },
                );
                Ok(Value::String(claim.to_string()))
            })();
            drop(registry);
            drop(retired);
            result.map_err(anyhow::Error::from)
        })
    }
}

fn expire_pending(registry: &mut Registry, now: Instant) -> Vec<ForkEntry> {
    registry
        .fork_calls
        .extract_if(|_, entry| entry.pending_expired(now))
        .map(|(_, entry)| entry)
        .collect()
}

pub(super) fn validate_bound(
    host: &HostControl,
    registry: &Registry,
    binding: &Arc<ThreadBinding>,
    id: ThreadId,
    parent: &Arc<CodexThread>,
) -> io::Result<()> {
    if registry.closed
        || host.incoming.is_stopped()
        || host.shutdown.is_cancelled()
        || parent.has_shutdown_status()
        || !registry
            .bindings
            .get(&binding.key)
            .is_some_and(|(actual, _)| Arc::ptr_eq(actual, binding))
    {
        return Err(binding::withdrawn());
    }
    let bound = binding
        .parent
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    if !bound
        .as_ref()
        .is_some_and(|bound| bound.id == id && Arc::ptr_eq(&bound.thread, parent))
    {
        return Err(binding::withdrawn());
    }
    Ok(())
}

impl PreparedChildBinding {
    /// Installs on the actual new runtime before binding publication/initial
    /// turn. A foreign typed hook is retained and causes refusal, never replaced.
    pub(super) async fn install_fork_origin(&self, thread: &Arc<CodexThread>) -> io::Result<()> {
        let actual = thread.config().await;
        let intended: codex_config::McpServerConfig = serde_json::from_value(self.config())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid private fork registration",
                )
            })?;
        if actual.mcp_servers.get().get(binding::NAMESPACE) != Some(&intended) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fork registration changed before binding",
            ));
        }
        let provider = self
            .binding
            .fork_provider
            .get_or_init(|| {
                Arc::new(Provider {
                    binding: Arc::downgrade(&self.binding),
                }) as Arc<dyn McpToolRequestMetaProvider>
            })
            .clone();
        let expected_transport = intended.transport.clone();
        let hook = McpToolRequestMetaHook {
            server_name: binding::NAMESPACE.into(),
            source: McpServerSource::Config,
            transport: intended.transport,
            tool_name: "fork_thread".into(),
            key: META_KEY.into(),
            provider: provider.clone(),
        };
        if !thread.thread_extension_data().insert_if(
            hook,
            |existing: Option<&McpToolRequestMetaHook>| {
                existing.is_none_or(|existing| {
                    existing.server_name == binding::NAMESPACE
                        && existing.source == McpServerSource::Config
                        && existing.transport == expected_transport
                        && existing.tool_name == "fork_thread"
                        && existing.key == META_KEY
                        && Arc::ptr_eq(&existing.provider, &provider)
                })
            },
        ) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "another MCP call hook owns runtime",
            ));
        }
        Ok(())
    }
}

impl HostControl {
    pub(super) fn retire_fork_owner(&self, binding_id: Uuid) {
        let retired = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .fork_calls
                .extract_if(|_, entry| {
                    entry.origin.binding_id == binding_id && entry.retired_owner()
                })
                .map(|(_, entry)| entry)
                .collect::<Vec<_>>()
        };
        // Work/binding destruction can re-enter the actual registry.
        drop(retired);
    }
    pub(super) fn retire_expired_fork_calls(&self) {
        let retired = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            expire_pending(&mut registry, Instant::now())
        };
        drop(retired);
    }
}
