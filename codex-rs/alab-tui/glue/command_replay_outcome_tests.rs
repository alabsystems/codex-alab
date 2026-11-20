// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
#[test]
fn recorded_command_status_preserves_optional_exit_and_style() {
    use crate::history_cell::HistoryCell as _;
    use CommandExecutionStatus::{Completed, Failed};
    use ratatui::style::Color;
    for (status, code, expected, color) in [
        (Completed, None, "Completed", Color::Green),
        (Failed, None, "Failed", Color::Red),
        (Failed, Some(0), "Failed (exit 0)", Color::Red),
        (Failed, Some(-1), "✗ (-1)", Color::Red),
        (Failed, Some(7), "✗ (7)", Color::Red),
        (Completed, Some(0), "✓", Color::Green),
    ] {
        let mut item = command_item(status);
        if let ThreadItemPayload::CommandExecution {
            exit_code,
            duration_ms,
            ..
        } = &mut item
        {
            *exit_code = code;
            *duration_ms = None;
        }
        let command = CommandHistory::from_item(item).unwrap();
        assert_eq!((command.output.exit_code, command.duration), (code, None));
        let cell = command.into_cell();
        assert!(!cell.is_active());
        let rows = cell.transcript_lines(80);
        let footer = rows.last().unwrap();
        assert_eq!(footer.to_string(), expected);
        assert_eq!(footer.spans[0].style.fg.or(footer.style.fg), Some(color));
    }
}
