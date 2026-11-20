// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn completed_guards_end_with_the_retained_or_queued_result() {
    let thread = ThreadId::new();
    let kept: Arc<dyn HistoryCell> = Arc::new(history_cell::new_info_event("kept".into(), None));
    let mut commands = Commands::default();
    for index in 0..100 {
        let row: Arc<dyn HistoryCell> = if index == 0 {
            Arc::clone(&kept)
        } else {
            Arc::new(history_cell::new_info_event("evicted".into(), None))
        };
        commands
            .observations
            .entry(index.to_string())
            .or_default()
            .insert(
                Owner {
                    thread,
                    turn: "finding".into(),
                },
                ResultState::Displayed(Arc::downgrade(&row)),
            );
    }
    commands.sweep(&[], false);
    assert_eq!(commands.observations.len(), 1);
    assert!(commands.observations.contains_key("0"));
    drop(kept);
    commands.sweep(&[], false);
    assert!(commands.observations.is_empty());
}

#[test]
fn evicted_pending_guards_last_only_while_their_terminal_might_need_an_exit() {
    let thread = ThreadId::new();
    let mut commands = Commands::default();
    let original = Weak::<ExecCell>::new();
    for (turn, process, reused) in [
        ("absent", "11", false),
        ("live", "25", false),
        ("later owner of same handle", "25", true),
    ] {
        commands
            .observations
            .entry("item".into())
            .or_default()
            .insert(
                Owner {
                    thread,
                    turn: turn.into(),
                },
                ResultState::Pending(Observation {
                    cell: original.clone(),
                    source: ExecCommandSource::UnifiedExecStartup,
                    process_id: Some(process.into()),
                    reused_call_id: false,
                    reused_handle: reused,
                }),
            );
    }
    commands.sweep(&[], true);
    assert_eq!(commands.observations["item"].len(), 2);
    let terminal = UnifiedExecProcessSummary {
        key: "25".into(),
        call_id: "item".into(),
        command_display: "owned".into(),
        recent_chunks: Vec::new(),
    };
    commands.sweep(&[terminal], false);
    let remaining = &commands.observations["item"];
    assert_eq!(remaining.len(), 1);
    assert!(remaining.contains_key(&Owner {
        thread,
        turn: "live".into()
    }));
    commands.sweep(&[], false);
    assert!(commands.observations.is_empty());
}
