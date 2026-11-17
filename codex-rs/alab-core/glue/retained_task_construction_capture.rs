// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual construction history stays distinct from prospective NEW-task DATA.
//! Every resource is process-local; neither variant is Native/SDK authority.
use super::*;
use crate::config_manager::ConfigManager;
use crate::request_processors::ValidatedProspectiveStart;
use codex_app_server_protocol::ThreadForkParams;
use std::sync::Arc;
use std::sync::Weak;

#[derive(Clone)]
pub(crate) enum ConstructionCapture {
    Start(Arc<StartCapture>),
    Fork(Arc<ForkConstruction>),
    Resume(Arc<ResumeConstruction>),
}
impl From<StartCapture> for ConstructionCapture {
    fn from(actual: StartCapture) -> Self {
        Self::Start(Arc::new(actual))
    }
}
impl From<ForkConstruction> for ConstructionCapture {
    fn from(actual: ForkConstruction) -> Self {
        Self::Fork(Arc::new(actual))
    }
}
/// Capture only after the original fork target resolved its actual Config and
/// the original Core parameter/config/namespace validators accepted prospective
/// data. Birth binding separately captures the exact returned Core Arc.
pub(crate) struct ForkConstruction {
    pub(crate) _original: ThreadForkParams,
    pub(crate) prospective: ValidatedProspectiveStart,
    pub(crate) _resolved: Arc<Config>,
    pub(crate) _config_manager: ConfigManager,
    pub(crate) _auth_manager: Arc<codex_login::AuthManager>,
    // A strong manager here would cycle Core->MCP binding->capture->manager.
    pub(crate) _manager: Weak<ThreadManager>,
    pub(crate) app_server_client_name: Option<String>,
    pub(crate) app_server_client_version: Option<String>,
    pub(crate) client_mcp_extensions: ClientMcpExtensions,
}
impl From<ResumeConstruction> for ConstructionCapture {
    fn from(actual: ResumeConstruction) -> Self {
        Self::Resume(Arc::new(actual))
    }
}
pub(crate) struct ResumeConstruction {
    pub(crate) _original: codex_app_server_protocol::ThreadResumeParams,
    pub(crate) prospective: ValidatedProspectiveStart,
    pub(crate) _resolved: Arc<Config>,
    pub(crate) _config_manager: ConfigManager,
    pub(crate) _auth_manager: Arc<codex_login::AuthManager>,
    pub(crate) _manager: Weak<ThreadManager>,
    pub(crate) app_server_client_name: Option<String>,
    pub(crate) app_server_client_version: Option<String>,
    pub(crate) client_mcp_extensions: ClientMcpExtensions,
}
impl ConstructionCapture {
    /// The original real START request, or the existing UI projection for a
    /// prospective NEW child of the fork. Fork is never relabelled as START.
    pub(crate) fn prospective_create(&self) -> codex_app_server_protocol::ThreadStartParams {
        match self {
            Self::Start(actual) => actual.baseline.clone(),
            Self::Fork(actual) => actual.prospective.requested.clone(),
            Self::Resume(actual) => actual.prospective.requested.clone(),
        }
    }
    pub(crate) fn client_name(&self) -> Option<String> {
        match self {
            Self::Start(actual) => actual.app_server_client_name.clone(),
            Self::Fork(actual) => actual.app_server_client_name.clone(),
            Self::Resume(actual) => actual.app_server_client_name.clone(),
        }
    }
    pub(crate) fn client_version(&self) -> Option<String> {
        match self {
            Self::Start(actual) => actual.app_server_client_version.clone(),
            Self::Fork(actual) => actual.app_server_client_version.clone(),
            Self::Resume(actual) => actual.app_server_client_version.clone(),
        }
    }
    pub(crate) fn extensions(&self) -> ClientMcpExtensions {
        match self {
            Self::Start(actual) => actual.client_mcp_extensions.clone(),
            Self::Fork(actual) => actual.client_mcp_extensions.clone(),
            Self::Resume(actual) => actual.client_mcp_extensions.clone(),
        }
    }
}
