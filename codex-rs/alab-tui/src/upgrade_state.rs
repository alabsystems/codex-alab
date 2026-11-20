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

/// Joined runtime format: Core/config, goals, input and terminal startup. Bump
/// whenever a nested schema or receiver contract becomes incompatible.
pub const VERSION: u32 = 5;
pub const MAGIC: &[u8] = b"codex-runtime-state-v5\n";
// The released v2 format and this v5 format retain client-input records. The resolved-media v3
// and v4 readers live with upstream's resolved user input representation.
const READABLE_FORMATS: &[(u32, &[u8])] = &[(2, b"codex-runtime-state-v2\n"), (VERSION, MAGIC)];

pub(crate) fn reader_versions() -> Vec<u32> {
    READABLE_FORMATS
        .iter()
        .map(|(version, _)| *version)
        .collect()
}
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
    #[serde(borrow)]
    pub editor: &'a RawValue,
}
/// Encode data supplied by a committed source owner; this grants no SDK role.
pub fn encode(
    primary: &str,
    goals: &[u8],
    input: &[u8],
    modes: &terminal_modes::Snapshot,
    startup: &upgrade_startup::Snapshot,
    editor: &[u8],
) -> io::Result<Vec<u8>> {
    startup.validate()?;
    let frame = Frame {
        version: VERSION,
        primary: primary.into(),
        goals: serde_json::from_slice(goals)?,
        terminal: Terminal {
            input: serde_json::from_slice(input)?,
            modes: modes.clone(),
            startup: startup.clone(),
            editor: serde_json::from_slice(editor)?,
        },
    };
    crate::upgrade_frame::encode(
        MAGIC,
        &frame,
        upgrade_exec_custody::MAX_STATE_BYTES,
        "oversized runtime state",
    )
}
/// Parse the terminal envelope while leaving Core/config data opaque. A reader
/// separately validates its own schema and independently obtains native authority.
pub fn data(bytes: &[u8]) -> io::Result<Frame<'_>> {
    if bytes.len() > upgrade_exec_custody::MAX_STATE_BYTES {
        return Err(invalid("oversized runtime state"));
    }
    let (version, body) = READABLE_FORMATS
        .iter()
        .find_map(|(version, magic)| bytes.strip_prefix(*magic).map(|body| (*version, body)))
        .ok_or_else(|| invalid("unknown runtime state frame"))?;
    let frame: Frame<'_> = serde_json::from_slice(body)?;
    if frame.version != version {
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
    deadline: tokio::time::Instant,
    bytes: Arc<[u8]>,
    primary: String,
    continuation: tokio::sync::OnceCell<Arc<dyn codex_alab_core::runtime_upgrade::Continuation>>,
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
    /// One startup deadline covers constructor loading, workspace selection
    /// and restoration on the final embedded server.
    pub fn deadline(&self) -> tokio::time::Instant {
        self.deadline
    }
    /// Bind this exact checkpoint to the fresh native constructor before the
    /// CLI loads ordinary configuration or connects to an app server.
    pub async fn admit(&self) -> io::Result<()> {
        self.continuation
            .get_or_try_init(|| async {
                let host = codex_alab_core::native_source_launch::runtime_upgrade()?;
                let build = upgrade_exec_custody::adopted_build()
                    .ok_or_else(|| invalid("runtime restoration has no admitted package"))?;
                tokio::time::timeout_at(
                    self.deadline,
                    host.load(self.primary(), self.bytes(), build),
                )
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native runtime load deadline elapsed",
                    )
                })?
            })
            .await?;
        Ok(())
    }
    pub fn continuation(
        &self,
    ) -> io::Result<Arc<dyn codex_alab_core::runtime_upgrade::Continuation>> {
        self.continuation
            .get()
            .cloned()
            .ok_or_else(|| invalid("runtime startup has no native continuation"))
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
    let installed = upgrade_exec_custody::install_state_if(
        |bytes| bytes.starts_with(b"codex-runtime-state-"),
        |bytes| {
            // The custody lock serializes this module's only Pending publisher.
            // A rejected install retains the original frame, never absence.
            if PENDING.get().is_some() {
                return Err(invalid("runtime startup state was already installed"));
            }
            let parsed = data(bytes)?;
            let pending = Pending {
                deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(120),
                bytes: Arc::from(bytes),
                primary: parsed.primary,
                continuation: tokio::sync::OnceCell::const_new(),
            };
            upgrade_startup::restore(
                parsed.terminal.startup,
                &parsed.terminal.modes,
                parsed.terminal.input.get().as_bytes(),
            )?;
            let recovery = Recovery {
                installed: true,
                cleanup: None,
            };
            PENDING
                .set(pending)
                .map_err(|_| invalid("runtime startup state was already installed"))?;
            Ok(recovery)
        },
    )?;
    Ok(installed.unwrap_or(Recovery {
        installed: false,
        cleanup: None,
    }))
}
pub fn pending() -> Option<&'static Pending> {
    PENDING.get()
}
fn invalid(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_existing_frames_and_writes_the_queued_result_format() {
        // Frozen 0.11 terminal envelope. Its decoder leaves goals, editor and input
        // opaque here.
        const RELEASED: &[u8] = br#"codex-runtime-state-v2
{"version":2,"primary":"00000000-0000-4000-8000-000000000001","goals":null,"terminal":{
  "input":{"version":1,"buffer":[],"ready":[],"escape":null,"discarded_sequence":null},
  "modes":{"version":1,"operating_system":"macos","terminal":{"device":1,"inode":2,"rdevice":3},
    "original":{"flags":[11010,3,19200,1483],"characters":[4,255,255,127,23,21,18,255,3,28,26,25,17,19,22,15,1,0,20,255],"input_speed":9600,"output_speed":9600,"line":0},
    "running":{"flags":[10240,2,19200,67],"characters":[4,255,255,127,23,21,18,255,3,28,26,25,17,19,22,15,1,0,20,255],"input_speed":9600,"output_speed":9600,"line":0}},
  "startup":{"version":1,"cursor":[0,0],"colors":null,"enhanced_keys":false,
    "terminal_app_over_ssh":false,"reports_key_releases":true,"da1_supported":false,"tmux_image_base":null,
    "viewport":[0,0,80,24],"saved_viewport":null,"hidden_cursor":true,"visible_history_rows":0,
    "screen":{"active":true,"mouse_active":true,"mouse_capture_disabled":false,"input_configured":true,
      "keyboard_active":true,"parent_reports_key_releases":true,"pointer":0},
    "focused":true,"owned_screen":true,"alt_screen_enabled":true,"mouse_capture":0},
  "editor":{"synthetic_terminal_failure_fixture":true}}}"#;
        let mut previous = data(RELEASED).unwrap();
        let encoded = encode(
            &previous.primary,
            previous.goals.get().as_bytes(),
            previous.terminal.input.get().as_bytes(),
            &previous.terminal.modes,
            &previous.terminal.startup,
            previous.terminal.editor.get().as_bytes(),
        )
        .unwrap();
        let v2 = b"codex-runtime-state-v2\n".as_slice();
        let v5 = b"codex-runtime-state-v5\n".as_slice();
        previous.version = VERSION;
        let mut expected = v5.to_vec();
        expected.extend(serde_json::to_vec(&previous).unwrap());
        assert_eq!(encoded, expected);
        let current_body = encoded.strip_prefix(v5).unwrap();
        assert_eq!(data(&encoded).unwrap().version, 5);
        assert_eq!(reader_versions(), [2, 5]);
        for mismatched in [
            [v2, current_body].concat(),
            [v5, RELEASED.strip_prefix(v2).unwrap()].concat(),
            [b"codex-runtime-state-v4\n", current_body].concat(),
        ] {
            assert!(data(&mismatched).is_err());
        }
    }
}
