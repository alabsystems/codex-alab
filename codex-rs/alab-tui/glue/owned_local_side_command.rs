// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual passive command surface, using the ORIGINAL command mapper/cell.
//! Full live prefixes replace output DATA on the SAME call; real subsequent
//! deltas apply once in original store order. This creates no Core operation.
use super::alab_side_delivery::Receipt;
use super::*;
use crate::app::side_companion::SideCompanion;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
impl SideCompanion {
    pub(in crate::app) fn alab_apply_command(
        &mut self,
        notification: &ServerNotification,
        receipt: &Receipt,
    ) -> Result<Option<bool>> {
        let relevant = matches!(notification,ServerNotification::ItemStarted(x) if matches!(x.item.payload,ThreadItemPayload::CommandExecution{..}))
            || matches!(
                notification,
                ServerNotification::CommandExecutionOutputDelta(_)
            );
        if !relevant {
            return Ok(None);
        }
        if !self.alab_command_delivery.newer(receipt)? {
            return Ok(Some(false));
        }
        match notification {
            ServerNotification::ItemStarted(event) => {
                let ThreadItemPayload::CommandExecution {
                    id,
                    command,
                    command_actions,
                    source,
                    status,
                    aggregated_output,
                    ..
                } = &event.item.payload
                else {
                    return Ok(None);
                };
                if *status != codex_app_server_protocol::CommandExecutionStatus::InProgress {
                    bail!("live command prefix is not in progress")
                }
                let (argv, parsed) = crate::chatwidget::ChatWidget::alab_side_command_projection(
                    command,
                    command_actions,
                );
                if let Some(active) = self.display.active.as_mut() {
                    let cell=active.as_any_mut().downcast_mut::<ExecCell>().ok_or_else(||eyre!("overlapping original active surfaces need their original grouping route"))?;
                    if !cell.iter_calls().any(|call| call.call_id == *id) {
                        if !cell.add_call(id.clone(), argv.clone(), parsed.clone(), *source, None) {
                            bail!("original active command group rejected another call")
                        }
                    }
                    // Validate against the actual original call, not merely an
                    // ID. A full authoritative prefix replaces only its output.
                    let call = cell
                        .group
                        .calls
                        .iter_mut()
                        .rev()
                        .find(|call| call.call_id == *id)
                        .ok_or_else(|| eyre!("actual command call missing"))?;
                    if call.command != argv || call.parsed != parsed || call.source != *source {
                        bail!("live command prefix changed original command policy")
                    }
                    call.output = None;
                    if let Some(output) = aggregated_output {
                        cell.append_output(id, output);
                    }
                } else {
                    let mut cell =
                        new_active_exec_command(id.clone(), argv, parsed, *source, None, false);
                    if let Some(output) = aggregated_output {
                        cell.append_output(id, output);
                    }
                    self.display.active = Some(Box::new(cell));
                    self.parked_tail = self.cells.len();
                }
            }
            ServerNotification::CommandExecutionOutputDelta(event) => {
                let cell = self
                    .display
                    .active
                    .as_mut()
                    .and_then(|active| active.as_any_mut().downcast_mut::<ExecCell>())
                    .ok_or_else(|| eyre!("actual command output has no original cell"))?;
                if !event.delta.is_empty() && !cell.append_output(&event.item_id, &event.delta) {
                    bail!("actual command output changed its original call")
                }
            }
            _ => return Ok(None),
        }
        if let ServerNotification::ItemStarted(event) = notification {
            self.display
                .alab_history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .record(
                    vec![super::alab_side_history_data::Source::Item {
                        turn: event.turn_id.clone(),
                        item: event.item.payload.id().to_owned(),
                    }],
                    vec![event.item.payload.clone()],
                    &[],
                )?;
        }
        self.alab_command_delivery.finish(receipt);
        Ok(Some(true))
    }
}
