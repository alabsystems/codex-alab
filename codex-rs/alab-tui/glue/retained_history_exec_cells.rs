// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native command group, full argv/parsed policy, clocks and incremental output.
use super::model::ExecCall;
use super::model::ExecCell;
use super::model::alab_retained_output;
use crate::history_cell::HistoryCell;
use crate::history_cell::alab_retained_cells::ActivityDetailsData;
use crate::history_cell::alab_retained_cells::CaptureContext;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use codex_app_server_protocol::CommandExecutionSource;
use codex_protocol::parse_command::ParsedCommand;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    calls: Vec<Call>,
    details: ActivityDetailsData,
    animations_enabled: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Call {
    call_id: String,
    command: Vec<String>,
    parsed: Vec<Parsed>,
    output: Option<alab_retained_output::Data>,
    source: CommandExecutionSource,
    start_time: Option<Snapshot>,
    duration: Option<Duration>,
    interaction_input: Option<String>,
}
impl Data {
    pub(crate) fn capture(
        actual: &dyn HistoryCell,
        context: &mut CaptureContext<'_>,
    ) -> io::Result<Option<Self>> {
        let Some(actual) = actual.as_any().downcast_ref::<ExecCell>() else {
            return Ok(None);
        };
        if actual.group.calls.len() > native::MAX_NODES {
            return Err(native::invalid("native exec call capacity"));
        }
        let calls = actual
            .group
            .calls
            .iter()
            .map(|call| {
                Ok(Call {
                    call_id: call.call_id.clone(),
                    command: call.command.clone(),
                    parsed: call.parsed.iter().map(Parsed::capture).collect(),
                    output: call
                        .output
                        .as_ref()
                        .map(alab_retained_output::Data::capture),
                    source: call.source,
                    start_time: call
                        .start_time
                        .map(Snapshot::capture)
                        .transpose()
                        .map_err(native::invalid)?,
                    duration: call.duration,
                    interaction_input: call.interaction_input.clone(),
                })
            })
            .collect::<io::Result<_>>()?;
        let details =
            ActivityDetailsData::capture(&actual.group.details, context, actual.group.calls.len())?;
        Ok(Some(Self {
            calls,
            details,
            animations_enabled: actual.animations_enabled(),
        }))
    }
    pub(crate) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        if self.calls.len() > native::MAX_NODES {
            return Err(native::invalid("native exec call capacity"));
        }
        let calls = self
            .calls
            .into_iter()
            .map(|call| {
                Ok(ExecCall {
                    call_id: call.call_id,
                    command: call.command,
                    parsed: call
                        .parsed
                        .into_iter()
                        .map(Parsed::restore)
                        .collect::<io::Result<_>>()?,
                    output: call
                        .output
                        .map(|output| {
                            if resources.legacy_command_outputs() {
                                output.restore_legacy(call.duration)
                            } else {
                                output.restore()
                            }
                        })
                        .transpose()?,
                    source: call.source,
                    start_time: call
                        .start_time
                        .map(|clock| clock.restore())
                        .transpose()
                        .map_err(native::invalid)?,
                    duration: call.duration,
                    interaction_input: call.interaction_input,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let details = self
            .details
            .restore(shared, resources, calls.len(), nesting)?;
        Ok(Box::new(super::model::alab_retained_output::restore_exec(
            calls,
            details,
            self.animations_enabled,
        )))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Parsed {
    Read {
        cmd: String,
        name: String,
        path: Vec<u8>,
    },
    ListFiles {
        cmd: String,
        path: Option<String>,
    },
    Search {
        cmd: String,
        query: Option<String>,
        path: Option<String>,
    },
    Unknown {
        cmd: String,
    },
}
impl Parsed {
    fn capture(actual: &ParsedCommand) -> Self {
        match actual {
            ParsedCommand::Read { cmd, name, path } => Self::Read {
                cmd: cmd.clone(),
                name: name.clone(),
                path: path.as_os_str().as_bytes().to_vec(),
            },
            ParsedCommand::ListFiles { cmd, path } => Self::ListFiles {
                cmd: cmd.clone(),
                path: path.clone(),
            },
            ParsedCommand::Search { cmd, query, path } => Self::Search {
                cmd: cmd.clone(),
                query: query.clone(),
                path: path.clone(),
            },
            ParsedCommand::Unknown { cmd } => Self::Unknown { cmd: cmd.clone() },
        }
    }
    fn restore(self) -> io::Result<ParsedCommand> {
        Ok(match self {
            Self::Read { cmd, name, path } => {
                if path.contains(&0) {
                    return Err(native::invalid("native parsed-command path contains NUL"));
                }
                ParsedCommand::Read {
                    cmd,
                    name,
                    path: std::path::PathBuf::from(std::ffi::OsString::from_vec(path)),
                }
            }
            Self::ListFiles { cmd, path } => ParsedCommand::ListFiles { cmd, path },
            Self::Search { cmd, query, path } => ParsedCommand::Search { cmd, query, path },
            Self::Unknown { cmd } => ParsedCommand::Unknown { cmd },
        })
    }
}
