// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[test]
fn raw_trace_preserves_missing_and_observed_exit_codes() -> anyhow::Result<()> {
    use crate::protocol_event::{ToolRuntimeTraceEvent, tool_runtime_trace_event};
    use codex_protocol::protocol::{
        EventMsg, ExecCommandEndEvent, ExecCommandSource, ExecCommandStatus,
    };
    use std::time::Duration;

    let zero = Some(Duration::ZERO);
    let elapsed = Some(Duration::from_millis(17));
    for (status, code, integer, duration) in [
        (ExecCommandStatus::Failed, None, -1, None),
        (ExecCommandStatus::Completed, None, 0, zero),
        (ExecCommandStatus::Failed, Some(-1), -1, None),
        (ExecCommandStatus::Completed, Some(0), 0, zero),
        (ExecCommandStatus::Completed, Some(0), 0, elapsed),
    ] {
        let event = EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id: "call".into(),
            plugin_id: None,
            script_path: None,
            process_id: None,
            turn_id: "turn".into(),
            completed_at_ms: 20,
            command: vec!["cat".into()],
            cwd: "file:///workspace".parse()?,
            parsed_cmd: Vec::new(),
            source: ExecCommandSource::Agent,
            interaction_input: None,
            aggregated_output: "observed output".into(),
            exit_code: integer,
            exit_code_unavailable: code.is_none(),
            duration: duration.unwrap_or_default(),
            duration_unavailable: duration.is_none(),
            status,
        });
        let Some(ToolRuntimeTraceEvent::Ended { payload, .. }) = tool_runtime_trace_event(&event)
        else {
            panic!("one final trace event");
        };
        let json = serde_json::to_value(payload)?;
        assert_eq!(
            json.get("exit_code").and_then(serde_json::Value::as_i64),
            Some(i64::from(integer))
        );
        assert_eq!(
            json.get("exit_code_unavailable")
                .and_then(serde_json::Value::as_bool),
            code.is_none().then_some(true)
        );
        assert_eq!(
            json.get("duration"),
            Some(&serde_json::to_value(duration.unwrap_or_default())?)
        );
        assert_eq!(
            json.get("duration_unavailable").cloned(),
            duration.is_none().then_some(serde_json::Value::Bool(true))
        );
        let parsed = super::parse_terminal_response_payload(
            json,
            &TerminalOperationKind::ExecCommand,
            "test",
        )?;
        assert_eq!(parsed.result.exit_code, code);
        assert_eq!(parsed.result.stdout, "observed output");
    }
    Ok(())
}
