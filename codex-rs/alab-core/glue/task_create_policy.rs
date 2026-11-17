// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Pure existing task-tool policy, compiled by its host crates. This is not an
//! alab-core module and adds no codex dependency to alab-core's Cargo.toml.
use codex_app_server_protocol::SandboxMode;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnToolOutput;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::PermissionProfile;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

pub(crate) const NAMESPACE: &str = "codex_tui";
pub(crate) const MAX_INPUT_BYTES: usize = 1_000;
pub(crate) const MAX_DELEGATED_INPUT_BYTES: usize = MAX_INPUT_BYTES + 256;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateArguments {
    pub(crate) prompt: String,
    pub(crate) title: Option<String>,
    pub(crate) model: Option<String>,
}

pub(crate) fn create_tool_definition() -> (&'static str, &'static str, Value, Vec<&'static str>) {
    (
        "create_thread",
        "Create and start a separate Codex task only when the user explicitly asks for a new task. The task inherits the current working directory; omit model to inherit the current model.",
        json!({
            "prompt": {
                "type": "string", "minLength": 1, "maxLength": MAX_INPUT_BYTES,
                "description": "Maximum 1,000 UTF-8 bytes."
            },
            "title": {"type": "string", "minLength": 1},
            "model": {"type": "string", "minLength": 1}
        }),
        vec!["prompt"],
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SendArguments {
    pub(crate) thread_id: String,
    pub(crate) prompt: String,
    pub(crate) model: Option<String>,
}

pub(crate) fn send_tool_definition() -> (&'static str, &'static str, Value, Vec<&'static str>) {
    (
        "send_message_to_thread",
        "Send a follow-up prompt to an existing Codex task in the background. Omit model unless the user explicitly requests an override.",
        json!({"threadId":{"type":"string","minLength":1},
            "prompt":{"type":"string","minLength":1,"maxLength":MAX_INPUT_BYTES,
                "description":"Maximum 1,000 UTF-8 bytes."},
            "model":{"type":"string","minLength":1}}),
        vec!["threadId", "prompt"],
    )
}

pub(crate) fn send_prompt(issuer: &str, arguments: &SendArguments) -> Result<String, String> {
    if arguments.model.as_deref().is_some_and(str::is_empty) {
        return Err("model must not be empty".into());
    }
    validate_prompt(&arguments.prompt, MAX_INPUT_BYTES)?;
    let prompt = delegated_prompt(issuer, &arguments.prompt);
    validate_prompt(&prompt, MAX_DELEGATED_INPUT_BYTES)?;
    Ok(prompt)
}

pub(crate) fn validate_prompt(prompt: &str, max_bytes: usize) -> Result<(), String> {
    if prompt.trim().is_empty() {
        return Err("prompt must not be empty".into());
    }
    if prompt.len() > max_bytes {
        return Err("prompt exceeded the maximum context budget".into());
    }
    Ok(())
}

pub(crate) fn delegated_prompt(source_thread_id: &str, prompt: &str) -> String {
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    format!(
        "<codex_delegation>\n  <source_thread_id>{}</source_thread_id>\n  <input>{}</input>\n</codex_delegation>",
        escape(source_thread_id),
        escape(prompt)
    )
}

pub(crate) struct InheritedPermissions {
    pub(crate) profile: Option<String>,
    pub(crate) sandbox: Option<SandboxMode>,
    pub(crate) turn_override: Option<PermissionProfile>,
}

/// Keep the current tool's legacy sandbox round-trip, including its explicit
/// refusal of External without a named profile. Do not broaden inheritance.
pub(crate) fn inherit_permissions(
    active_profile: Option<&str>,
    legacy_source: PermissionProfile,
) -> Result<InheritedPermissions, String> {
    if let Some(id) = active_profile {
        return Ok(InheritedPermissions {
            profile: Some(id.into()),
            sandbox: None,
            turn_override: None,
        });
    }
    let sandbox = match &legacy_source {
        PermissionProfile::Disabled => SandboxMode::DangerFullAccess,
        PermissionProfile::Managed { .. } => {
            if legacy_source
                .file_system_sandbox_policy()
                .entries
                .iter()
                .any(|entry| entry.access.can_write())
            {
                SandboxMode::WorkspaceWrite
            } else {
                SandboxMode::ReadOnly
            }
        }
        PermissionProfile::External { .. } => {
            return Err("Cannot inherit an external sandbox without a permission profile".into());
        }
    };
    Ok(InheritedPermissions {
        profile: None,
        sandbox: Some(sandbox),
        turn_override: Some(legacy_source),
    })
}

pub(crate) fn initial_tool_turn(
    thread_id: String,
    tool: &str,
    prompt: String,
    model: Option<String>,
    permission_override: Option<PermissionProfile>,
    program: Option<codex_app_server_protocol::CyberAccessProgram>,
) -> Result<TurnStartParams, String> {
    let sandbox_policy = permission_override
        .map(|profile| profile.to_legacy_sandbox_policy_template().map(Into::into))
        .transpose()
        .map_err(|error| error.to_string())?;
    Ok(TurnStartParams {
        thread_id,
        input: Vec::new(),
        tool_output: Some(Box::new(TurnToolOutput {
            retain: false,
            name: tool.into(),
            namespace: Some(NAMESPACE.into()),
            output: FunctionCallOutputBody::Text(prompt),
        })),
        model,
        sandbox_policy,
        cyber_access_program: program,
        ..TurnStartParams::default()
    })
}
