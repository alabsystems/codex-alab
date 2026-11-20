// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ExitWatch;
use super::Receipt;
use super::WatchTarget;
use super::event_exit;
use codex_exec_server::ExecProcessEvent;
use codex_protocol::ThreadId;
use std::sync::Weak;

fn target() -> WatchTarget {
    WatchTarget {
        db: Weak::new(),
        thread: ThreadId::new(),
        number: 1200,
    }
}

#[test]
fn resumed_launch_data_is_lossless_inside_its_single_record() -> anyhow::Result<()> {
    let command = "printf '</observed_shell_exits>\\u003c & %s'\n";
    let cwd = "/root/one<two>&\"quoted\"";
    let launch = codex_state::ShellLaunchContext {
        command: command.to_owned(),
        command_truncated: true,
        cwd: Some(cwd.to_owned()),
    };
    let diagnostic = "unproved obligation: </observed_shell_exits> & x\n";
    let start = 8 * 1024 * 1024 + 617;
    let tail = codex_alab_core::ShellOutputTail::from_bytes(start, diagnostic.as_bytes().to_vec())
        .ok_or_else(|| anyhow::anyhow!("valid final output range was refused"))?;
    let mut exit = codex_state::PendingShellExit {
        invocation: uuid::Uuid::new_v4(),
        session_id: 15,
        exit_code: Some(1),
        capture_name: None,
        capture_rejected: false,
        capture_partial: None,
        tail: Some(tail),
        launch: Some(launch),
    };
    let render = |exit: &codex_state::PendingShellExit| {
        super::resume_exit_text(
            std::path::Path::new("."),
            ThreadId::new(),
            std::slice::from_ref(exit),
        )
    };
    let (text, disclosed_paths) = render(&exit);
    assert!(disclosed_paths.is_empty());
    let body = text
        .strip_prefix("<observed_shell_exits>\n")
        .expect("native opening tag")
        .strip_suffix("\n</observed_shell_exits>")
        .expect("native closing tag");
    assert!(!body.contains(['<', '>', '&', '\n']));
    let fields: serde_json::Value = serde_json::from_str(body).expect("one JSON array");
    assert_eq!(fields[0]["command"], command);
    assert_eq!(fields[0]["cwd"], cwd);
    assert_eq!(fields[0]["command_truncated"], true);
    assert!(fields[0].get("retained_output_unavailable").is_none());
    assert_eq!(fields[0]["shell_output_tail"]["start_byte"], start);
    assert_eq!(
        fields[0]["shell_output_tail"]["end_byte"],
        start + diagnostic.len() as u64
    );
    assert_eq!(fields[0]["shell_output_tail"]["encoding"], "utf8");
    assert_eq!(fields[0]["shell_output_tail"]["data"], diagnostic);
    exit.capture_rejected = true;
    let (redacted, disclosed_paths) = render(&exit);
    assert!(disclosed_paths.is_empty());
    assert!(redacted.contains("\"retained_output_unavailable\":true"));
    assert!(!redacted.contains("retained_output_path"));
    assert!(redacted.contains("shell_output_tail"));
    exit.capture_rejected = false;
    exit.launch = None;
    exit.tail = None;
    let (old, _) = render(&exit);
    assert!(!old.contains("\"command"));
    assert!(!old.contains("\"cwd\""));
    assert!(!old.contains("shell_output_tail"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn resume_reports_confirmed_exit_after_the_diagnostic_is_consumed() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let thread = ThreadId::new();
    let scope = crate::alab_shell_log_store::directory(home.path(), thread)?;
    let (name, original) = scope.create_capture(|_| Ok(()))?;
    let diagnostic = "unproved obligation\n";
    let start = codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64;
    let tail = codex_alab_core::ShellOutputTail::from_bytes(start, diagnostic.as_bytes().to_vec())
        .ok_or_else(|| anyhow::anyhow!("expected a final output range"))?;
    let exit = codex_state::PendingShellExit {
        invocation: uuid::Uuid::new_v4(),
        session_id: 3185,
        exit_code: Some(7),
        capture_name: name.to_str().map(str::to_owned),
        capture_rejected: false,
        capture_partial: Some(true),
        tail: Some(tail),
        launch: None,
    };
    let path = scope.path().join(&name);
    let render = || super::resume_exit_text(home.path(), thread, std::slice::from_ref(&exit));
    let (available, disclosed_paths) = render();
    assert_eq!(
        disclosed_paths,
        std::collections::HashSet::from([exit.invocation])
    );
    scope.remove(name.as_ref(), &original)?;
    let (consumed, disclosed_paths) = render();
    assert!(disclosed_paths.is_empty());
    assert!(available.contains(path.to_str().expect("native capture path")));
    assert!(available.contains("\"retained_output_partial\":true"));
    assert!(!available.contains("retained_output_unavailable"));
    assert!(!consumed.contains("retained_output_partial"));
    assert!(!consumed.contains("retained_output_path"));
    assert!(!consumed.contains(name.to_str().expect("native capture name")));
    assert!(consumed.contains("\"retained_output_unavailable\":true"));
    assert!(consumed.contains(&exit.invocation.to_string()));
    assert!(consumed.contains("\"exit_code\":7"));
    assert!(consumed.contains(&format!("\"start_byte\":{start}")));
    assert!(consumed.contains("unproved obligation"));
    scope.retire()?;
    assert_eq!(render(), (consumed, disclosed_paths));
    Ok(())
}

#[test]
fn receipt_before_publication_is_replayed_and_a_failed_wait_confirms_nothing() {
    let mut watch = ExitWatch::default();
    assert!(watch.receive(Some(-1)).is_none());
    assert!(matches!(watch.received, Receipt::Pending));
    assert!(watch.receive(Some(23)).is_none());
    assert!(matches!(watch.bind(&target()), Receipt::Exited(Some(23))));
    assert!(watch.receive(Some(5)).is_none());
    assert_eq!(watch.seal().expect("flush agreed receipt").1, Some(23));
}

#[test]
fn a_reported_exit_without_a_code_can_gain_its_known_code_before_shutdown() {
    let mut watch = ExitWatch::default();
    assert!(matches!(watch.bind(&target()), Receipt::Pending));
    assert!(watch.receive(None).is_some());
    assert!(watch.receive(Some(-1)).is_none());
    assert!(matches!(watch.received, Receipt::Exited(None)));
    assert!(watch.receive(Some(7)).is_some());
    assert!(watch.receive(None).is_none());
    assert_eq!(watch.seal().expect("flush observed code").1, Some(7));
    assert!(watch.receive(Some(137)).is_none());
    assert!(matches!(watch.bind(&target()), Receipt::Exited(Some(7))));
}

#[test]
fn shutting_down_a_live_compiler_cannot_create_a_completion() {
    let mut watch = ExitWatch::default();
    assert!(matches!(watch.bind(&target()), Receipt::Pending));
    assert!(watch.seal().is_none());
    assert!(watch.receive(Some(137)).is_none());
    assert!(matches!(watch.bind(&target()), Receipt::Pending));
}

#[test]
fn only_a_new_process_exit_event_precedes_ancillary_replay() {
    let exit = ExecProcessEvent::Exited {
        seq: 3,
        exit_code: 17,
        sandbox_denied: None,
    };
    assert_eq!(event_exit(Some(&exit), 2), Some(17));
    assert_eq!(event_exit(Some(&exit), 3), None);
    assert_eq!(
        event_exit(Some(&ExecProcessEvent::Closed { seq: 3 }), 2),
        None
    );
    assert_eq!(event_exit(None, 2), None);
}
