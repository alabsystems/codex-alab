// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The local, post-interaction bootstrap boundary shared with retained hosts.
//!
//! These are the caller's live process-local inputs, after its actual config,
//! authentication, trust and environment preparation. They are not a wire
//! format or evidence that those interactions ran. A backend must independently
//! prepare its own objects from the original selected launch data. Frontend
//! `Arc`s and managers never cross the process boundary.
//!
//! This module does not select a TUI target. In particular it cannot replace
//! the existing interactive startup, accept native admission, reload a running
//! backend, or reconcile an update receiver. Those boundaries must be wired
//! before ordinary OwnedLocal selection can be enabled.

use super::Config;
use codex_alab_core::native_incoming::Incoming;
use codex_app_server_client::EmbeddedNetworkPolicy;
use codex_app_server_client::RetainedRuntimeOwner;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_exec_server::EnvironmentManager;
use codex_feedback::CodexFeedback;
use codex_rollout::StateDbHandle;
use codex_state::log_db::LogDbLayer;
use std::sync::Arc;

/// Original local bootstrap objects, owned by the process that will use them.
///
/// Construct this only from the actual fallible preparation result. There is
/// deliberately no default, deserializer, implicit login, or embedded fallback.
/// The type carries data and local resources; it grants no native permission.
pub struct PreparedLocalInputs {
    pub arg0_paths: Arg0DispatchPaths,
    pub config: Config,
    pub cli_kv_overrides: Vec<(String, toml::Value)>,
    pub loader_overrides: LoaderOverrides,
    pub strict_config: bool,
    pub cloud_config_bundle: CloudConfigBundleLoader,
    pub feedback: CodexFeedback,
    pub log_db: Option<LogDbLayer>,
    pub state_db: Option<StateDbHandle>,
    pub environment_manager: Arc<EnvironmentManager>,
    pub embedded_network_policy: EmbeddedNetworkPolicy,
}

impl PreparedLocalInputs {
    /// Consume the complete prepared inputs into one actual retained runtime.
    ///
    /// The original TUI assembler supplies every identity/capability and startup
    /// field, then the existing client adapter performs its original conversion.
    /// The caller must first bind and retain its real incoming provider in the
    /// admitted backend process. `Incoming` only carries IO custody; neither it
    /// nor a successful runtime start is native authority or effect settlement.
    ///
    /// Returning the owner accepts its inputs; it does not report preparation
    /// success or readiness. Clients must still complete real initialization.
    /// Retain the returned owner across frontend EOF. Its existing `shutdown`
    /// and `wait` observe the one Source worker; dropping it closes Source input
    /// synchronously. Native service replacement remains a separate transaction.
    pub async fn start_retained(
        self,
        incoming: Incoming,
    ) -> color_eyre::Result<RetainedRuntimeOwner> {
        super::start_embedded_app_server_with(
            self.arg0_paths,
            self.config,
            self.cli_kv_overrides,
            self.loader_overrides,
            self.strict_config,
            self.cloud_config_bundle,
            self.feedback,
            self.log_db,
            self.state_db,
            self.environment_manager,
            self.embedded_network_policy,
            |args| std::future::ready(args.start_retained_native_host(incoming)),
        )
        .await
    }
}

impl PreparedLocalInputs {
    /// Use only the backend process's original source builder, after its real
    /// preparation finalized CLI/harness/loader/cloud choices. No live frontend
    /// object crosses this boundary, and this call grants no Native admission.
    pub async fn start_retained_with_worktree_source(
        self,
        incoming: Incoming,
        source: crate::legacy_core::config::ConfigBuilder,
    ) -> color_eyre::Result<RetainedRuntimeOwner> {
        super::start_embedded_app_server_with(
            self.arg0_paths,
            self.config,
            self.cli_kv_overrides,
            self.loader_overrides,
            self.strict_config,
            self.cloud_config_bundle,
            self.feedback,
            self.log_db,
            self.state_db,
            self.environment_manager,
            self.embedded_network_policy,
            |args| {
                std::future::ready(
                    args.start_retained_native_host_with_worktree_source(incoming, source),
                )
            },
        )
        .await
    }
}

#[cfg(test)]
#[path = "owned_local_bootstrap_tests.rs"]
mod tests;

impl PreparedLocalInputs {
    /// Consume every original input exactly once with the actual source builder
    /// and the already-held gate owned by this same backend's Native adapter.
    pub async fn start_retained_with_worktree_source_and_goal_gate(
        self,
        incoming: Incoming,
        source: crate::legacy_core::config::ConfigBuilder,
        gate: codex_alab_core::retained_goal_gate::Gate,
    ) -> color_eyre::Result<RetainedRuntimeOwner> {
        super::start_embedded_app_server_with(
            self.arg0_paths,
            self.config,
            self.cli_kv_overrides,
            self.loader_overrides,
            self.strict_config,
            self.cloud_config_bundle,
            self.feedback,
            self.log_db,
            self.state_db,
            self.environment_manager,
            self.embedded_network_policy,
            |args| {
                std::future::ready(
                    args.start_retained_native_host_with_worktree_source_and_goal_gate(
                        incoming, source, gate,
                    ),
                )
            },
        )
        .await
    }
}

impl PreparedLocalInputs {
    /// Genuine physical-bound ordinary constructor. Move every original field
    /// once through the SAME assembler, alongside the actual builder/plan.
    pub async fn start_retained_ordinary_bound(
        self,
        bound: codex_alab_tui::frontend_control::retained_service::BoundBackend,
        source: crate::legacy_core::config::ConfigBuilder,
        initial: codex_app_server_client::InitialSessionPlan,
    ) -> color_eyre::Result<RetainedRuntimeOwner> {
        super::start_embedded_app_server_with(
            self.arg0_paths,
            self.config,
            self.cli_kv_overrides,
            self.loader_overrides,
            self.strict_config,
            self.cloud_config_bundle,
            self.feedback,
            self.log_db,
            self.state_db,
            self.environment_manager,
            self.embedded_network_policy,
            |args| std::future::ready(args.start_retained_ordinary_bound(bound, source, initial)),
        )
        .await
    }
}
