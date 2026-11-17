// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Backend-process construction over the existing complete embedded arguments.
//! This adapter adds no task, process, listener, connection, SDK permission or
//! frontend ownership. The caller retains the actual bound native provider.

use crate::InProcessClientStartArgs;
use codex_alab_core::native_incoming::Incoming;
use codex_app_server::in_process::RetainedRuntimeOwner;

impl InProcessClientStartArgs {
    /// Client declaration DATA from the same exhaustive original mapping.
    pub fn native_frontend_declaration(
        &self,
    ) -> (codex_app_server_protocol::InitializeParams, usize) {
        (self.initialize_params(), self.channel_capacity)
    }

    /// Initialize one actual Native-delivered frontend stream with the original
    /// complete caller declaration. This constructs no runtime, AuthManager or
    /// task server; the selected target separately preserves local semantics.
    pub async fn connect_native_frontend(
        self,
        stream: tokio::net::UnixStream,
    ) -> std::io::Result<crate::RemoteAppServerClient> {
        crate::RemoteAppServerClient::connect_native_stream_with_initialize(
            stream,
            self.initialize_params(),
            self.channel_capacity,
        )
        .await
    }

    /// Start the retained host in the already-admitted backend process.
    ///
    /// The backend main retains the returned owner. A TUI frontend instead
    /// receives its own physically delivered stream and initializes a client.
    /// `incoming` is the actual already-bound native input; these arguments and
    /// this constructor grant no native admission or effect-settlement receipt.
    ///
    /// Delegate the original conversion so every runtime input, InitializeParams
    /// mapping and concrete local thread-loader occurrence stays unchanged.
    pub fn start_retained_native_host(
        self,
        incoming: Incoming,
    ) -> std::io::Result<RetainedRuntimeOwner> {
        codex_app_server::in_process::start_retained(self.into_runtime_start_args(), incoming)
    }
    /// Same complete conversion, with the backend's actual original source
    /// builder. This object is never serialized or borrowed from a frontend.
    pub fn start_retained_native_host_with_worktree_source(
        self,
        incoming: Incoming,
        source: codex_core::config::ConfigBuilder,
    ) -> std::io::Result<RetainedRuntimeOwner> {
        codex_app_server::in_process::start_retained_with_worktree_source(
            self.into_runtime_start_args(),
            incoming,
            source,
        )
    }
}

impl InProcessClientStartArgs {
    /// Same complete original conversion; gate is an inhibitor, never a grant.
    pub fn start_retained_native_host_with_worktree_source_and_goal_gate(
        self,
        incoming: Incoming,
        source: codex_core::config::ConfigBuilder,
        gate: codex_alab_core::retained_goal_gate::Gate,
    ) -> std::io::Result<RetainedRuntimeOwner> {
        codex_app_server::in_process::start_retained_with_worktree_source_and_goal_gate(
            self.into_runtime_start_args(),
            incoming,
            source,
            gate,
        )
    }
}

impl InProcessClientStartArgs {
    /// Preserve the complete original conversion and actual initial selection owner.
    pub fn start_retained_ordinary(
        self,
        incoming: Incoming,
        source: codex_core::config::ConfigBuilder,
        gate: codex_alab_core::retained_goal_gate::Gate,
        initial: codex_app_server::in_process::InitialSessionPlan,
    ) -> std::io::Result<RetainedRuntimeOwner> {
        codex_app_server::in_process::start_retained_ordinary(
            self.into_runtime_start_args(),
            incoming,
            source,
            gate,
            initial,
        )
    }
}

impl InProcessClientStartArgs {
    /// Same original exhaustive conversion, now coupled to the real bind owner.
    pub fn start_retained_ordinary_bound(
        self,
        bound: codex_app_server::in_process::RetainedBoundBackend,
        source: codex_core::config::ConfigBuilder,
        initial: codex_app_server::in_process::InitialSessionPlan,
    ) -> std::io::Result<RetainedRuntimeOwner> {
        codex_app_server::in_process::start_retained_ordinary_bound(
            self.into_runtime_start_args(),
            bound,
            source,
            initial,
        )
    }
}
