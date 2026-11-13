// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Namespaced runtime data and early original-terminal recovery. Core/goal
//! decoding belongs to the app-server after fresh native constructor admission.
use crate::terminal_modes;
use crate::upgrade_exec_custody;
use crate::upgrade_startup;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use std::io;
use std::sync::Arc;
use std::sync::OnceLock;

pub const MAGIC: &[u8] = b"codex-runtime-state-v1\n";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame<'a> {
    version: u32,
    pub primary: String,
    #[serde(borrow)]
    pub goals: &'a RawValue,
    pub terminal: Terminal<'a>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal<'a> {
    #[serde(borrow)]
    pub input: &'a RawValue,
    pub modes: terminal_modes::Snapshot,
    pub startup: upgrade_startup::Snapshot,
}
/// Encode data supplied by a committed source owner; this grants no SDK role.
pub fn encode(
    primary: &str,
    goals: &[u8],
    input: &[u8],
    modes: &terminal_modes::Snapshot,
    startup: &upgrade_startup::Snapshot,
) -> io::Result<Vec<u8>> {
    startup.validate()?;
    let frame = Frame {
        version: 1,
        primary: primary.into(),
        goals: serde_json::from_slice(goals)?,
        terminal: Terminal {
            input: serde_json::from_slice(input)?,
            modes: modes.clone(),
            startup: startup.clone(),
        },
    };
    let mut bytes = MAGIC.to_vec();
    serde_json::to_writer(&mut bytes, &frame)?;
    if bytes.len() > upgrade_exec_custody::MAX_STATE_BYTES {
        return Err(invalid("oversized runtime state"));
    }
    Ok(bytes)
}
/// Parse the terminal envelope while leaving Core/config data opaque. A reader
/// separately validates its own schema and independently obtains native authority.
pub fn data(bytes: &[u8]) -> io::Result<Frame<'_>> {
    if bytes.len() > upgrade_exec_custody::MAX_STATE_BYTES {
        return Err(invalid("oversized runtime state"));
    }
    let body = bytes
        .strip_prefix(MAGIC)
        .ok_or_else(|| invalid("unknown runtime state frame"))?;
    let frame: Frame<'_> = serde_json::from_slice(body)?;
    if frame.version != 1 {
        return Err(invalid("incompatible runtime state version"));
    }
    frame.terminal.startup.validate()?;
    Ok(frame)
}
pub fn goal_bytes<'a>(bytes: &'a [u8], expected_primary: &str) -> io::Result<&'a [u8]> {
    let frame = data(bytes)?;
    if frame.primary != expected_primary {
        return Err(invalid(
            "runtime state differs from explicit resume routing",
        ));
    }
    Ok(frame.goals.get().as_bytes())
}
/// Retain the complete original frame through startup, including failures. Its
/// opaque goal body is not a native SDK source or permission to resume work.
pub struct Pending {
    bytes: Arc<[u8]>,
    primary: String,
}
impl Pending {
    pub fn primary(&self) -> &str {
        &self.primary
    }
    pub fn validate_primary(&self, expected_primary: &str) -> io::Result<()> {
        if expected_primary != self.primary {
            return Err(invalid(
                "runtime state differs from explicit resume routing",
            ));
        }
        Ok(())
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
static PENDING: OnceLock<Pending> = OnceLock::new();

#[must_use = "retain the terminal recovery owner through CLI completion"]
pub struct Recovery {
    installed: bool,
    cleanup: Option<fn() -> io::Result<()>>,
}
impl Recovery {
    /// The CLI supplies the real TUI's protocol cleanup without creating a
    /// dependency from generic framing back to the application implementation.
    pub fn with_cleanup(mut self, cleanup: fn() -> io::Result<()>) -> Self {
        self.cleanup = Some(cleanup);
        self
    }
}
impl Drop for Recovery {
    fn drop(&mut self) {
        if self.installed
            && terminal_modes::original_retained()
            && !terminal_modes::preserve_on_exit()
            && let Err(error) = self.cleanup.unwrap_or(terminal_modes::disable_raw_mode)()
        {
            tracing::error!(%error,"could not restore original terminal after replacement startup");
        }
    }
}
/// Called immediately after actual kernel custody adoption and ALab activation,
/// before a runtime or credential/config reader. Other opaque namespaces remain
/// owned by their consumers. This does not authenticate SDK constructor grants.
pub fn stage_before_runtime() -> io::Result<Recovery> {
    let Some(bytes) = upgrade_exec_custody::take_state_if(|bytes| bytes.starts_with(MAGIC)) else {
        return Ok(Recovery {
            installed: false,
            cleanup: None,
        });
    };
    if PENDING.get().is_some() {
        return Err(invalid("runtime startup state was already installed"));
    }
    let parsed = data(&bytes)?;
    upgrade_startup::restore(
        parsed.terminal.startup,
        &parsed.terminal.modes,
        parsed.terminal.input.get().as_bytes(),
    )?;
    let recovery = Recovery {
        installed: true,
        cleanup: None,
    };
    let primary = parsed.primary;
    PENDING
        .set(Pending {
            bytes: bytes.into(),
            primary,
        })
        .map_err(|_| invalid("runtime startup state was already installed"))?;
    Ok(recovery)
}
pub fn pending() -> Option<&'static Pending> {
    PENDING.get()
}
fn invalid(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}
