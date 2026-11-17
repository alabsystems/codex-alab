// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lifecycle queries use the original authenticated member transport. They
//! never reconnect onto a different work channel or accept a supplied endpoint.
use super::*;
use codex_alab_core::native_root::IoOffer;
use codex_alab_core::native_root::ToolAction;
use codex_alab_core::native_root::ToolState;
use serde::de::DeserializeOwned;

frame!(RootFrame, String, root: serde_json::Value);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Key {
    version: u32,
    tool_operation: String,
    birth: String,
}
#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum State {
    Running {
        version: u32,
        tool_operation: String,
        birth: String,
    },
    Finished {
        version: u32,
        tool_operation: String,
        birth: String,
        wait_status: i32,
        receipt_scope: String,
    },
}

async fn call<T: DeserializeOwned>(
    member: &NativeMember,
    action: &str,
    operation: &str,
    tool: Option<&str>,
    payload: Option<&[u8]>,
) -> io::Result<T> {
    if member.target.is_some()
        || !valid_hex(operation, 32)
        || tool.is_some_and(|tool| !valid_hex(tool, 32))
        || member.poisoned.load(std::sync::atomic::Ordering::Acquire)
        || member.retired.load(std::sync::atomic::Ordering::Acquire)
    {
        return Err(unavailable());
    }
    let mut fields = format!(" member={}", member.id);
    if let Some(tool) = tool {
        fields.push_str(&format!(" tool_operation={tool}"));
    }
    if let Some(payload) = payload {
        use sha2::Digest as _;
        if payload.is_empty() || payload.len() > 512 * 1024 {
            return Err(unavailable());
        }
        let digest = format!("{:x}", sha2::Sha256::digest(payload));
        fields.push_str(&format!(
            " payload_length={} payload_digest={digest}",
            payload.len()
        ));
    }
    let body = member
        .transport
        .call(action, operation, &fields, Some(&member.id))
        .await?;
    let frame: RootFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
    frame.validate(
        &frame.request,
        operation,
        &member.transport.sid,
        &member.transport.birth,
        member.transport.epoch()?.as_deref(),
    )?;
    if frame.member != member.id
        || !member.transport.conn.is_open()
        || member.poisoned.load(std::sync::atomic::Ordering::Acquire)
        || member.retired.load(std::sync::atomic::Ordering::Acquire)
    {
        return Err(unavailable());
    }
    serde_json::from_value(frame.root).map_err(|_| unavailable())
}

pub(super) async fn available(member: &NativeMember, operation: &str) -> io::Result<()> {
    let version: Version = call(member, "member-root", operation, None, None).await?;
    if version.version != 1 {
        return Err(unavailable());
    }
    Ok(())
}

fn identity(version: u32, actual: &str, expected: &str, birth: &str) -> io::Result<()> {
    if version != 1 || actual != expected || !valid_hex(birth, 32) {
        return Err(unavailable());
    }
    Ok(())
}

pub(super) async fn key(member: &NativeMember, operation: &str, tool: &str) -> io::Result<String> {
    let key: Key = call(member, "member-root-tool-key", operation, Some(tool), None).await?;
    identity(key.version, &key.tool_operation, tool, &key.birth)?;
    Ok(key.birth)
}

fn observed(state: State, tool: &str) -> io::Result<ToolState> {
    match state {
        State::Running {
            version,
            tool_operation,
            birth,
        } => {
            identity(version, &tool_operation, tool, &birth)?;
            Ok(ToolState::Running { birth })
        }
        State::Finished {
            version,
            tool_operation,
            birth,
            wait_status,
            receipt_scope,
        } => {
            identity(version, &tool_operation, tool, &birth)?;
            if receipt_scope != "captured-cohort" {
                return Err(unavailable());
            }
            Ok(ToolState::Finished { birth, wait_status })
        }
    }
}

pub(super) async fn state(
    member: &NativeMember,
    operation: &str,
    tool: &str,
    action: ToolAction,
) -> io::Result<ToolState> {
    let action = match action {
        ToolAction::Observe => "member-root-tool-state",
        ToolAction::Stop => "member-root-tool-stop",
    };
    observed(
        call(member, action, operation, Some(tool), None).await?,
        tool,
    )
}

#[cfg(test)]
#[path = "source_root_tests.rs"]
mod tests;

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Offer {
    Rejected {
        version: u32,
        tool_operation: String,
    },
    Pending {
        version: u32,
        tool_operation: String,
        nonce: String,
    },
    Retained {
        version: u32,
        tool_operation: String,
        birth: String,
    },
}

pub(super) async fn prepare(
    member: &NativeMember,
    operation: &str,
    tool: &str,
    payload: &[u8],
) -> io::Result<IoOffer> {
    let offer: Offer = call(
        member,
        "member-root-tool-prepare",
        operation,
        Some(tool),
        Some(payload),
    )
    .await?;
    offered(offer, tool)
}

fn offered(offer: Offer, tool: &str) -> io::Result<IoOffer> {
    match offer {
        Offer::Rejected {
            version,
            tool_operation,
        } => {
            if version != 1 || tool_operation != tool {
                return Err(unavailable());
            }
            Ok(IoOffer::Rejected)
        }
        Offer::Pending {
            version,
            tool_operation,
            nonce,
        } => {
            identity(version, &tool_operation, tool, &nonce)?;
            Ok(IoOffer::Pending { nonce })
        }
        Offer::Retained {
            version,
            tool_operation,
            birth,
        } => {
            identity(version, &tool_operation, tool, &birth)?;
            Ok(IoOffer::Retained { birth })
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Execution {
    NotSpawned {
        version: u32,
        tool_operation: String,
        attempt: String,
        errno: Option<i32>,
    },
    Refused {
        version: u32,
        tool_operation: String,
        errno: Option<i32>,
    },
    AwaitingBirth {
        version: u32,
        tool_operation: String,
    },
    Pending {
        version: u32,
        tool_operation: String,
        birth: String,
    },
    NoExecError {
        version: u32,
        tool_operation: String,
        birth: String,
    },
    Failed {
        version: u32,
        tool_operation: String,
        birth: String,
        errno: Option<i32>,
    },
}

fn execution_observed(
    value: Execution,
    tool: &str,
) -> io::Result<codex_alab_core::native_root::ToolExecution> {
    use codex_alab_core::native_root::ToolExecution;
    Ok(match value {
        Execution::NotSpawned {
            version,
            tool_operation,
            attempt,
            errno,
        } => {
            identity(version, &tool_operation, tool, &attempt)?;
            ToolExecution::NotSpawned { attempt, errno }
        }
        Execution::Refused {
            version,
            tool_operation,
            errno,
        } => {
            if version != 1 || tool_operation != tool {
                return Err(unavailable());
            }
            ToolExecution::Refused { errno }
        }
        Execution::AwaitingBirth {
            version,
            tool_operation,
        } => {
            if version != 1 || tool_operation != tool {
                return Err(unavailable());
            }
            ToolExecution::AwaitingBirth
        }
        Execution::Pending {
            version,
            tool_operation,
            birth,
        } => {
            identity(version, &tool_operation, tool, &birth)?;
            ToolExecution::Pending { birth }
        }
        Execution::NoExecError {
            version,
            tool_operation,
            birth,
        } => {
            identity(version, &tool_operation, tool, &birth)?;
            ToolExecution::NoExecError { birth }
        }
        Execution::Failed {
            version,
            tool_operation,
            birth,
            errno,
        } => {
            identity(version, &tool_operation, tool, &birth)?;
            ToolExecution::Failed { birth, errno }
        }
    })
}

pub(super) async fn execution(
    member: &NativeMember,
    operation: &str,
    tool: &str,
) -> io::Result<codex_alab_core::native_root::ToolExecution> {
    execution_observed(
        call(member, "member-root-tool-exec", operation, Some(tool), None).await?,
        tool,
    )
}

frame!(ListenerFrame, String, listener_nonce: String);

pub(super) async fn prepare_listener(member: &NativeMember, operation: &str) -> io::Result<String> {
    if member.target.is_some()
        || !valid_hex(operation, 32)
        || member.poisoned.load(std::sync::atomic::Ordering::Acquire)
        || member.retired.load(std::sync::atomic::Ordering::Acquire)
    {
        return Err(unavailable());
    }
    let body = member
        .transport
        .call(
            "member-control-listener",
            operation,
            &format!(" member={}", member.id),
            Some(&member.id),
        )
        .await?;
    let frame: ListenerFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
    frame.validate(
        &frame.request,
        operation,
        &member.transport.sid,
        &member.transport.birth,
        member.transport.epoch()?.as_deref(),
    )?;
    if frame.member != member.id
        || !valid_hex(&frame.listener_nonce, 32)
        || !member.transport.conn.is_open()
        || member.poisoned.load(std::sync::atomic::Ordering::Acquire)
        || member.retired.load(std::sync::atomic::Ordering::Acquire)
    {
        return Err(unavailable());
    }
    Ok(frame.listener_nonce)
}
