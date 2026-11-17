// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The shared START's genuine host binding carrier. Its ordinary/unix-absent
//! form is inert; no role or configuration flag selects a retained service.
use super::*;

#[derive(Clone, Default)]
pub(crate) struct TaskHostSlot {
    #[cfg(unix)]
    host: crate::message_processor::alab_task_mcp::HostSlot,
}
#[derive(Clone, Default)]
pub(crate) struct TaskBindingPlan {
    #[cfg(unix)]
    binding: Option<crate::request_processors::PreparedChildBinding>,
    #[cfg(unix)]
    initial: Option<Arc<super::super::alab_initial_session::Scope>>,
}
impl TaskHostSlot {
    pub(crate) fn prepare(
        &self,
        params: &ThreadStartParams,
        name: &Option<String>,
        version: &Option<String>,
        extensions: &ClientMcpExtensions,
        supplied: TaskBindingPlan,
    ) -> Result<TaskBindingPlan, JSONRPCErrorError> {
        #[cfg(unix)]
        {
            let capture = crate::message_processor::alab_task_mcp::StartCapture {
                baseline: params.clone(),
                app_server_client_name: name.clone(),
                app_server_client_version: version.clone(),
                client_mcp_extensions: extensions.clone(),
            };
            let binding = self
                .host
                .prepare_start(capture, supplied.binding)
                .map_err(|_| internal_error("retained task MCP binding refused"))?;
            Ok(TaskBindingPlan {
                binding,
                initial: supplied.initial,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (params, name, version, extensions, supplied);
            Ok(TaskBindingPlan::default())
        }
    }
}
impl TaskBindingPlan {
    pub(crate) async fn configure(&self, config: &mut Config) -> Result<(), JSONRPCErrorError> {
        #[cfg(unix)]
        if let Some(binding) = &self.binding {
            binding.configure_loaded(config).await.map_err(|_| {
                invalid_request("retained task MCP namespace or managed policy refused")
            })?;
        }
        #[cfg(not(unix))]
        let _ = config;
        Ok(())
    }
    pub(crate) async fn bind_created(
        &self,
        id: ThreadId,
        thread: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        #[cfg(unix)]
        if let Some(binding) = &self.binding {
            binding
                .bind_created(id, thread.clone())
                .await
                .map_err(|_| internal_error("retained task runtime binding withdrew"))?;
            if let Some(initial) = &self.initial {
                initial.record_task_binding(binding.clone())?;
            }
        }
        #[cfg(not(unix))]
        let _ = (id, thread);
        Ok(())
    }
}
impl StartTarget {
    pub(in crate::request_processors) fn task_binding_plan(&self) -> TaskBindingPlan {
        #[cfg(unix)]
        if let Self::Backend { scope, .. } = self {
            return TaskBindingPlan {
                binding: scope.task_binding.clone(),
                initial: None,
            };
        }
        #[cfg(unix)]
        if let Self::Initial(scope) = self {
            return TaskBindingPlan {
                binding: None,
                initial: Some(scope.clone()),
            };
        }
        TaskBindingPlan::default()
    }
}
#[cfg(unix)]
impl ThreadRequestProcessor {
    pub(crate) fn alab_install_task_mcp(
        &self,
        host: &Arc<crate::message_processor::alab_task_mcp::HostControl>,
    ) -> std::io::Result<()> {
        self.alab_task_mcp.host.install(host)
    }
    pub(crate) fn alab_stop_task_mcp(&self) {
        self.alab_task_mcp.host.stop();
    }
}

#[cfg(unix)]
impl TaskHostSlot {
    pub(crate) async fn prepare_fork(
        &self,
        capture: crate::message_processor::alab_task_mcp::ForkConstruction,
        config: &mut Config,
    ) -> Result<crate::request_processors::PreparedChildBinding, JSONRPCErrorError> {
        self.host
            .prepare_fork(capture, config)
            .await
            .map_err(|_| invalid_request("initial fork namespace or managed policy refused"))
    }
}

#[cfg(unix)]
impl TaskHostSlot {
    pub(crate) async fn prepare_resume(
        &self,
        capture: crate::message_processor::alab_task_mcp::ResumeConstruction,
        config: &mut Config,
    ) -> Result<crate::request_processors::PreparedChildBinding, JSONRPCErrorError> {
        self.host
            .prepare_resume(capture, config)
            .await
            .map_err(|_| invalid_request("initial resume namespace or managed policy refused"))
    }
    pub(crate) async fn bound_tools(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<bool, JSONRPCErrorError> {
        self.host
            .bound_tools(id, actual)
            .await
            .map_err(|_| invalid_request("initial resume actual task binding withdrew"))
    }
}
