mod compact;
mod live_output;
mod model;
mod render;
mod transcript;

pub(crate) use model::CommandOutput;
#[cfg(test)]
pub(crate) use model::ExecCall;
pub(crate) use model::ExecCell;
pub(crate) use render::OutputLinesParams;
pub(crate) use render::TOOL_CALL_MAX_LINES;
pub(crate) use render::new_active_exec_command;
pub(crate) use render::output_lines;

#[cfg(unix)]
#[path = "../../../alab-tui/glue/retained_history_exec_cells.rs"]
pub(crate) mod alab_retained_exec; // ALAB(retained-ui): preserve exact native activity state and shared graph identity.
