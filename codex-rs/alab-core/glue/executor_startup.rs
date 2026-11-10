// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Executor startup keeps configuration and credential preparation on its own
//! task, behind a typed future boundary. Native source admission must not embed
//! its full async type graph in the command's transport/shutdown state machine.

use codex_core::config::Config;
use codex_login::CodexAuth;
use codex_utils_cli::CliConfigOverrides;
use std::future::Future;
use std::pin::Pin;

type Startup<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

pub(super) fn load_exec_server_config<'a>(
    overrides: &'a CliConfigOverrides,
    strict: bool,
    cloud: bool,
    auth: Option<&'a CodexAuth>,
) -> Startup<'a, Config> {
    Box::pin(crate::config::load_exec_server_config(
        overrides, strict, cloud, auth,
    ))
}

pub(super) fn load_exec_server_remote_auth_provider<'a>(
    config: &'a Config,
    base_url: &'a str,
    agent_identity: bool,
) -> Startup<'a, codex_api::SharedAuthProvider> {
    Box::pin(crate::config::load_exec_server_remote_auth_provider(
        config,
        base_url,
        agent_identity,
    ))
}
