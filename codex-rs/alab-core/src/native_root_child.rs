// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source IO and observations for a child whose custody remains in native Root.
//! No PID is exposed for Source-side signaling or reaping.
use crate::native_root::ToolAction;
use crate::native_root::ToolExecution;
use crate::native_root::ToolState;
use crate::native_source::Member;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::process::ExitStatusExt as _;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

pub struct Child {
    member: Arc<dyn Member>,
    tool: String,
    birth: String,
    exit: Option<ExitStatus>,
    pub stdout: Option<tokio::process::ChildStdout>,
    pub stderr: Option<tokio::process::ChildStderr>,
}

fn operation() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// Return None only in an ordinary process with no private native owner.
/// A withdrawn private owner is an error and never permits local fallback.
pub async fn spawn(command: crate::native_root_command::Command) -> io::Result<Option<Child>> {
    let Some(member) = crate::native_backend_owner::tool_member()? else {
        return Ok(None);
    };
    let payload = command.encode()?;
    let tool = operation();
    let (stdout, stdout_writer) = std::io::pipe()?;
    let (stderr, stderr_writer) = std::io::pipe()?;
    let stdin = std::fs::File::open("/dev/null")?;
    let descriptors: [OwnedFd; 3] = [stdin.into(), stdout_writer.into(), stderr_writer.into()];
    crate::native_backend_owner::send_tool_io(&operation(), &tool, &payload, descriptors).await?;
    // The caller owns its streams before awaiting exec observation. A lost or
    // cancelled reply requests stop for this same operation; it never rebirths.
    let mut pending = Pending {
        member: member.clone(),
        tool: tool.clone(),
        armed: true,
    };
    let birth = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            match member.root_tool_execution(&operation(), &tool).await? {
                ToolExecution::AwaitingBirth | ToolExecution::Pending { .. } => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                ToolExecution::NoExecError { birth } => return Ok(birth),
                ToolExecution::Failed { errno, .. }
                | ToolExecution::NotSpawned { errno, .. }
                | ToolExecution::Refused { errno } => {
                    return Err(errno.map_or_else(
                        || io::Error::other("native target exec observation failed"),
                        io::Error::from_raw_os_error,
                    ));
                }
            }
        }
    })
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    let stdout = tokio::process::ChildStdout::from_std(std::process::ChildStdout::from(
        OwnedFd::from(stdout),
    ))?;
    let stderr = tokio::process::ChildStderr::from_std(std::process::ChildStderr::from(
        OwnedFd::from(stderr),
    ))?;
    pending.armed = false;
    Ok(Some(Child {
        member,
        tool,
        birth,
        exit: None,
        stdout: Some(stdout),
        stderr: Some(stderr),
    }))
}

impl Child {
    /// Owning native wait and cohort absence, never a locally synthesized exit.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.observe(ToolAction::Observe).await
    }

    /// Stop the original native-retained cohort and wait for its actual receipt.
    pub async fn stop(&mut self) -> io::Result<ExitStatus> {
        self.observe(ToolAction::Stop).await
    }

    async fn observe(&mut self, action: ToolAction) -> io::Result<ExitStatus> {
        if let Some(status) = self.exit {
            return Ok(status);
        }
        loop {
            let state = self
                .member
                .root_tool_state(&operation(), &self.tool, action)
                .await?;
            let birth = match &state {
                ToolState::Running { birth } | ToolState::Finished { birth, .. } => birth,
            };
            if birth != &self.birth {
                return Err(crate::managed_auth::unavailable());
            }
            match state {
                ToolState::Running { .. } => tokio::time::sleep(Duration::from_millis(20)).await,
                ToolState::Finished { wait_status, .. } => {
                    let status = ExitStatus::from_raw(wait_status);
                    self.exit = Some(status);
                    return Ok(status);
                }
            }
        }
    }
}

struct Pending {
    member: Arc<dyn Member>,
    tool: String,
    armed: bool,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if self.armed {
            request_stop(self.member.clone(), self.tool.clone(), None);
        }
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        if self.exit.is_none() {
            request_stop(
                self.member.clone(),
                self.tool.clone(),
                Some(self.birth.clone()),
            );
        }
    }
}

fn request_stop(member: Arc<dyn Member>, tool: String, birth: Option<String>) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let birth = match birth {
                Some(birth) => birth,
                None => loop {
                    match member.root_tool_execution(&operation(), &tool).await {
                        Ok(ToolExecution::AwaitingBirth | ToolExecution::Pending { .. }) => {
                            tokio::time::sleep(Duration::from_millis(20)).await
                        }
                        Ok(
                            ToolExecution::NoExecError { birth }
                            | ToolExecution::Failed { birth, .. },
                        ) => break birth,
                        Ok(ToolExecution::NotSpawned { .. } | ToolExecution::Refused { .. })
                        | Err(_) => return,
                    }
                },
            };
            loop {
                match member
                    .root_tool_state(&operation(), &tool, ToolAction::Stop)
                    .await
                {
                    Ok(ToolState::Running { birth: actual }) if actual == birth => {
                        tokio::time::sleep(Duration::from_millis(20)).await
                    }
                    Ok(_) | Err(_) => return,
                }
            }
        });
    }
    // Runtime loss also ends the backend's physical owner connection. Native
    // Root then retains cleanup custody; this path invents no settlement ACK.
}
