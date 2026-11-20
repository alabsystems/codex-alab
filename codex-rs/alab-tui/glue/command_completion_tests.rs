// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
#[test]
fn untimed_terminal_command_keeps_its_reported_status() {
    use codex_app_server_protocol::CommandExecutionStatus;
    let command = vec!["cat".to_string(), "file".to_string()];
    let mut cell = super::super::new_active_exec_command(
        "read".into(),
        command,
        Vec::new(),
        CommandExecutionSource::Agent,
        None,
        false,
    );
    cell.group.calls[0].output = Some(CommandOutput::default());
    assert!(cell.is_active());
    cell.group
        .push_detail(Arc::from(new_reasoning_summary_block(
            vec!["Following the command".to_owned()],
            Path::new("."),
        ) as Box<dyn HistoryCell>));
    let preview = cell
        .transcript_lines(80)
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(preview.contains("Following the command"), "{preview}");
    assert!(
        !preview.contains('✓') && !preview.contains('✗'),
        "{preview}"
    );
    for legacy_code in [None, Some(-1)] {
        let output = CommandOutput::from_execution(
            CommandExecutionStatus::Declined,
            legacy_code,
            CommandExecutionSource::Agent,
            None,
        );
        assert_eq!(output.exit_code, None);
        assert_eq!(output.status_label().as_deref(), Some("Declined"));
        cell.group.calls[0].output = Some(output);
        assert!(!cell.is_active());
    }
    let rich = cell
        .display_lines(80)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rich.contains("Declined"), "{rich}");
    let rows = cell.transcript_lines(80);
    assert!(rows.iter().any(|line| line.to_string() == "Declined"));
    assert!(
        rows.iter()
            .any(|line| line.to_string().contains("Following the command"))
    );
}

#[cfg(unix)]
#[test]
fn retained_command_output_preserves_legacy_numbers_and_optional_terminal_status() {
    use super::super::model::alab_retained_output::Data;
    let old = serde_json::from_value::<Data>(serde_json::json!({
        "exitCode": 7, "aggregatedOutput": "old", "liveOutput": null
    }))
    .unwrap()
    .restore()
    .unwrap();
    assert_eq!(old.nonzero_exit(), Some(7));
    let declined = CommandOutput::from_execution(
        codex_app_server_protocol::CommandExecutionStatus::Declined,
        None,
        CommandExecutionSource::Agent,
        Some("review".into()),
    );
    let serialized = serde_json::to_value(Data::capture(&declined)).unwrap();
    let restored = serde_json::from_value::<Data>(serialized)
        .unwrap()
        .restore()
        .unwrap();
    assert_eq!(restored.exit_code, None);
    assert_eq!(restored.status_label().as_deref(), Some("Declined"));
    assert_eq!(restored.lines().next().as_deref(), Some("review"));
}
