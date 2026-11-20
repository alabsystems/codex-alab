// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact named original UI DATA; no Config/Arc/receipt is reconstructed.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StartedData {
    session: Session,
    turns: Vec<codex_app_server_protocol::Turn>,
    blocks_direct_input: bool,
    task_tools_available: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Session {
    daybreak_enabled: bool,
    windows_sandbox_host: Host,
    thread_id: codex_protocol::ThreadId,
    forked_from_id: Option<codex_protocol::ThreadId>,
    fork_parent_title: Option<String>,
    thread_name: Option<String>,
    model: String,
    model_provider_id: String,
    service_tier: Option<String>,
    approval_policy: codex_app_server_protocol::AskForApproval,
    approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer,
    permission_profile: codex_protocol::models::PermissionProfile,
    active_permission_profile: Option<codex_protocol::models::ActivePermissionProfile>,
    cwd: codex_utils_absolute_path::AbsolutePathBuf,
    runtime_workspace_roots: Vec<codex_utils_absolute_path::AbsolutePathBuf>,
    instruction_source_paths: Vec<codex_utils_path_uri::PathUri>,
    reasoning_effort: Option<codex_protocol::openai_models::ReasoningEffort>,
    collaboration_mode: Option<Box<codex_protocol::config_types::CollaborationMode>>,
    message_history: Option<History>,
    network_proxy: Option<Proxy>,
    rollout_path: Option<std::path::PathBuf>,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum Host {
    Local,
    Mixed,
    Remote,
    Unknown,
}
impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> std::result::Result<Self, D::Error> {
        match String::deserialize(reader)?.as_str() {
            "local" => Ok(Self::Local),
            "mixed" => Ok(Self::Mixed),
            "remote" => Ok(Self::Remote),
            "unknown" => Ok(Self::Unknown),
            _ => Err(serde::de::Error::custom(
                "unknown original Windows host classification",
            )),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct History {
    log_id: u64,
    entry_count: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Proxy {
    http_addr: String,
    socks_addr: String,
}
impl StartedData {
    pub(super) fn capture(original: &AppServerStartedThread) -> Self {
        let s = &original.session;
        Self {
            session: Session {
                daybreak_enabled: s.daybreak_enabled,
                windows_sandbox_host: match s.windows_sandbox_host {
                    crate::app::WindowsSandboxHost::Local => Host::Local,
                    crate::app::WindowsSandboxHost::Mixed => Host::Mixed,
                    crate::app::WindowsSandboxHost::Remote => Host::Remote,
                    crate::app::WindowsSandboxHost::Unknown => Host::Unknown,
                },
                thread_id: s.thread_id,
                forked_from_id: s.forked_from_id,
                fork_parent_title: s.fork_parent_title.clone(),
                thread_name: s.thread_name.clone(),
                model: s.model.clone(),
                model_provider_id: s.model_provider_id.clone(),
                service_tier: s.service_tier.clone(),
                approval_policy: s.approval_policy,
                approvals_reviewer: s.approvals_reviewer,
                permission_profile: s.permission_profile.clone(),
                active_permission_profile: s.active_permission_profile.clone(),
                cwd: s.cwd.clone(),
                runtime_workspace_roots: s.runtime_workspace_roots.clone(),
                instruction_source_paths: s.instruction_source_paths.clone(),
                reasoning_effort: s.reasoning_effort.clone(),
                collaboration_mode: s.collaboration_mode.clone(),
                message_history: s.message_history.as_ref().map(|x| History {
                    log_id: x.log_id,
                    entry_count: x.entry_count,
                }),
                network_proxy: s.network_proxy.as_ref().map(|x| Proxy {
                    http_addr: x.http_addr.clone(),
                    socks_addr: x.socks_addr.clone(),
                }),
                rollout_path: s.rollout_path.clone(),
            },
            turns: original.turns.clone(),
            blocks_direct_input: original.blocks_direct_input,
            task_tools_available: original.task_tools_available,
        }
    }
    pub(super) fn restore(self) -> Result<AppServerStartedThread> {
        let s = self.session;
        Ok(AppServerStartedThread {
            alab_construction: None,
            session: crate::session_state::ThreadSessionState {
                daybreak_enabled: s.daybreak_enabled,
                windows_sandbox_host: match s.windows_sandbox_host {
                    Host::Local => crate::app::WindowsSandboxHost::Local,
                    Host::Mixed => crate::app::WindowsSandboxHost::Mixed,
                    Host::Remote => crate::app::WindowsSandboxHost::Remote,
                    Host::Unknown => crate::app::WindowsSandboxHost::Unknown,
                },
                thread_id: s.thread_id,
                forked_from_id: s.forked_from_id,
                fork_parent_title: s.fork_parent_title,
                thread_name: s.thread_name,
                model: s.model,
                model_provider_id: s.model_provider_id,
                service_tier: s.service_tier,
                approval_policy: s.approval_policy,
                approvals_reviewer: s.approvals_reviewer,
                permission_profile: s.permission_profile,
                active_permission_profile: s.active_permission_profile,
                cwd: s.cwd,
                runtime_workspace_roots: s.runtime_workspace_roots,
                instruction_source_paths: s.instruction_source_paths,
                reasoning_effort: s.reasoning_effort,
                collaboration_mode: s.collaboration_mode,
                message_history: s.message_history.map(|x| {
                    crate::session_state::MessageHistoryMetadata {
                        log_id: x.log_id,
                        entry_count: x.entry_count,
                    }
                }),
                network_proxy: s.network_proxy.map(|x| {
                    crate::session_state::SessionNetworkProxyRuntime {
                        http_addr: x.http_addr,
                        socks_addr: x.socks_addr,
                    }
                }),
                rollout_path: s.rollout_path,
            },
            turns: self.turns,
            blocks_direct_input: self.blocks_direct_input,
            task_tools_available: self.task_tools_available,
        })
    }
}
