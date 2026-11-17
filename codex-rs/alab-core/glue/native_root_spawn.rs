// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Core's process selection; native child custody never becomes a local PID.
use super::*;

pub(crate) enum Child {
    Local(tokio::process::Child),
    #[cfg(unix)]
    Native(Box<codex_alab_core::native_root_child::Child>),
}

impl From<tokio::process::Child> for Child {
    fn from(child: tokio::process::Child) -> Self {
        Self::Local(child)
    }
}

pub(super) async fn spawn(request: &SpawnChildRequest<'_>) -> std::io::Result<Option<Child>> {
    #[cfg(unix)]
    {
        // Detect the actual retained constructor before selecting a local path.
        // A withdrawn owner is an error, never permission to fall back.
        if !codex_alab_core::native_backend_owner::has_tool_owner()? {
            return Ok(None);
        }
        if !matches!(request.stdio_policy, StdioPolicy::RedirectForShellTool) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "native tool route requires explicit pipe stdio",
            ));
        }
        let mut environment = request.env.clone();
        environment.retain(|name, _| !is_non_inheritable_env_var(name));
        if let Some(network) = request.network {
            network.apply_to_env(&mut environment);
        }
        if !request.network_sandbox_policy.is_enabled() {
            environment.insert(CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR.into(), "1".into());
        }
        let command = codex_alab_core::native_root_command::Command {
            program: request.program.clone(),
            arg0: Some(
                request
                    .arg0
                    .map_or_else(
                        || request.program.to_string_lossy().into_owned(),
                        String::from,
                    )
                    .into(),
            ),
            arguments: request.args.iter().map(Into::into).collect(),
            environment: environment
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
            directory: request.cwd.as_path().to_owned(),
        };
        codex_alab_core::native_root_child::spawn(command)
            .await
            .map(|child| child.map(|child| Child::Native(Box::new(child))))
    }
    #[cfg(not(unix))]
    {
        let _ = request;
        Ok(None)
    }
}
