// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Environment requests retain data; resolved attachments must come from a fresh owner.
use codex_protocol::protocol::EnvironmentConfig;
use codex_protocol::protocol::EnvironmentConfigState;
use codex_protocol::protocol::TurnEnvironmentRequest;
use codex_protocol::protocol::TurnEnvironmentRequests;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Requests {
    cwd: AbsolutePathBuf,
    requests: Vec<Request>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    cwd: String,
    roots: Vec<String>,
    state: State,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum State {
    FromThread,
    Pending,
    Failed(String),
    Ready(Value),
}

impl Requests {
    pub(super) fn capture(source: &TurnEnvironmentRequests) -> Result<Self, String> {
        let mut requests = Vec::new();
        for value in &source.environment_requests {
            let TurnEnvironmentRequest {
                environment_id,
                cwd,
                workspace_roots,
                config,
            } = value;
            let state = match config {
                EnvironmentConfigState::FromThread => State::FromThread,
                EnvironmentConfigState::Pending => State::Pending,
                EnvironmentConfigState::Failed(error) => State::Failed(error.clone()),
                EnvironmentConfigState::Ready(config) => State::Ready(evidence(config)?),
            };
            requests.push(Request {
                id: environment_id.clone(),
                cwd: cwd.to_string(),
                roots: workspace_roots.iter().map(ToString::to_string).collect(),
                state,
            });
        }
        let result = Self {
            cwd: source.legacy_fallback_cwd.clone(),
            requests,
        };
        result.validate()?;
        Ok(result)
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for request in &self.requests {
            if request.id.is_empty() || !ids.insert(&request.id) {
                return Err("tree environment identity is empty or repeated".into());
            }
            PathUri::parse(&request.cwd).map_err(|e| e.to_string())?;
            for root in &request.roots {
                PathUri::parse(root).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    pub(super) fn restore(
        &self,
        fresh: &TurnEnvironmentRequests,
    ) -> Result<TurnEnvironmentRequests, String> {
        self.validate()?;
        if self.cwd != fresh.legacy_fallback_cwd
            || self.requests.len() != fresh.environment_requests.len()
        {
            return Err("tree environments need matching fresh owner bindings".into());
        }
        let mut seen = HashSet::new();
        for request in &fresh.environment_requests {
            if !seen.insert(&request.environment_id) {
                return Err("fresh owner repeats an environment".into());
            }
        }
        let mut result = Vec::new();
        for saved in &self.requests {
            let fresh = fresh
                .environment_requests
                .iter()
                .find(|request| request.environment_id == saved.id)
                .ok_or("tree environment has no fresh owner binding")?;
            if fresh.cwd.to_string() != saved.cwd
                || fresh
                    .workspace_roots
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    != saved.roots
            {
                return Err("tree environment workspace changed before restoration".into());
            }
            match (&saved.state, &fresh.config) {
                (State::FromThread, EnvironmentConfigState::FromThread)
                | (State::Pending, EnvironmentConfigState::Pending) => {}
                (State::Failed(expected), EnvironmentConfigState::Failed(current))
                    if expected == current => {}
                (State::Ready(expected), EnvironmentConfigState::Ready(current))
                    if expected == &evidence(current)? => {}
                _ => {
                    return Err(
                        "tree environment owner configuration changed; revalidation is required"
                            .into(),
                    );
                }
            }
            // Copy only the freshly supplied attachment, never reconstruct one from evidence.
            result.push(fresh.clone());
        }
        Ok(TurnEnvironmentRequests::new(self.cwd.clone(), result))
    }
}

fn evidence(config: &EnvironmentConfig) -> Result<Value, String> {
    let EnvironmentConfig {
        environment_hooks,
        allow_login_shell,
        workspace_roots,
        permission_profile,
        shell_environment_policy,
        windows_sandbox_level,
        windows_sandbox_type,
        use_legacy_landlock,
        exec_policy,
        mcp_policy,
        network_policy,
        selected_capability_roots,
    } = config;
    let codex_protocol::config_types::ShellEnvironmentPolicy {
        inherit,
        ignore_default_excludes,
        exclude,
        r#set,
        include_only,
        use_profile,
    } = shell_environment_policy;
    Ok(json!({
        "hooks": environment_hooks, "allow_login_shell": allow_login_shell,
        "roots": workspace_roots.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "profile": permission_profile.permission_profile(),
        "active_profile": permission_profile.active_permission_profile(),
        "profile_roots": permission_profile.profile_workspace_roots().iter().map(|r| r.as_uri().to_string()).collect::<Vec<_>>(),
        "shell": { "inherit": inherit, "ignore_default_excludes": ignore_default_excludes,
            "exclude": exclude.iter().map(codex_protocol::config_types::EnvironmentVariablePattern::pattern).collect::<Vec<_>>(), "set": r#set,
            "include_only": include_only.iter().map(codex_protocol::config_types::EnvironmentVariablePattern::pattern).collect::<Vec<_>>(), "use_profile": use_profile },
        "windows_level": windows_sandbox_level, "windows_type": windows_sandbox_type,
        "landlock": use_legacy_landlock, "exec": exec_policy.as_ref().map(codex_execpolicy::RequirementsExecPolicy::fingerprint),
        "mcp": mcp_policy.as_ref().map(mcp_evidence), "network": network_policy,
        "selected_roots": selected_capability_roots,
    }))
}
fn mcp_evidence(policy: &codex_protocol::mcp_policy::EnvironmentMcpPolicy) -> Value {
    use codex_protocol::mcp_policy::*;
    fn matcher(value: &McpServerValueMatcher) -> Value {
        match value {
            McpServerValueMatcher::Exact { value } => json!(["exact", value]),
            McpServerValueMatcher::Prefix { value } => json!(["prefix", value]),
            McpServerValueMatcher::Regex { expression } => json!(["regex", expression]),
        }
    }
    fn server(value: &McpServerRequirement) -> Value {
        match value {
            McpServerRequirement::Identity { identity } => match identity {
                McpServerIdentity::Command { command } => json!(["command", command]),
                McpServerIdentity::Url { url } => json!(["url", url]),
            },
            McpServerRequirement::Command(McpServerCommandMatcher { executable, args }) => json!([
                "command_match",
                executable,
                args.iter().map(matcher).collect::<Vec<_>>()
            ]),
            McpServerRequirement::Url(value) => json!(["url_match", matcher(value)]),
        }
    }
    fn servers(value: &Option<std::collections::BTreeMap<String, McpServerRequirement>>) -> Value {
        value
            .as_ref()
            .map(|values| {
                Value::Object(
                    values
                        .iter()
                        .map(|(key, value)| (key.clone(), server(value)))
                        .collect(),
                )
            })
            .unwrap_or(Value::Null)
    }
    let EnvironmentMcpPolicy {
        servers: values,
        plugins,
    } = policy;
    json!({ "servers": servers(values), "plugins": plugins.as_ref().map(|values| values.iter().map(|(key, PluginMcpRequirements { mcp_servers })| (key.clone(), servers(mcp_servers))).collect::<serde_json::Map<_, _>>()) })
}
