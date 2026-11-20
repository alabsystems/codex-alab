// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE TUI'S EVENT HOOK ITSELF, in a process of its own: `upgrade::native_event` acts on the
//! process's one shared session state, which no other test may be reading meanwhile.

use std::path::Path;

use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::session::AccountHandoff;
use codex_alab_core::session::Autopilot;
use codex_alab_core::session::OwedAct;
use codex_alab_tui::session::Frame;
use codex_alab_tui::session::ProgramState;

/// EVERY APP AND APP-SERVER EVENT THE TUI DISPATCHES MOVES THE LIVE UPGRADE'S SCAN, AND ONLY IT
/// (review of the happy successor that never reran its turn: the usage poll's reply, one such
/// event a minute, refused each recovered turn's admission it landed in). The hook is inert
/// outside the codex-alab binary. In it, the scan counter moves and the native point automatic
/// admissions are fenced on stays: an upgrade scanned before the event is refused its commit,
/// as strictly as before, while a renewal decided before it still publishes; and the commit
/// goes through at the scan's new value, so the event alone had refused it.
#[test]
fn a_native_event_moves_the_upgrade_scan_alone_and_still_refuses_its_commit() {
    let shared = codex_alab_core::session::shared();
    let scan = shared.upgrade_generation();
    codex_alab_tui::upgrade::native_event();
    assert_eq!(
        shared.upgrade_generation(),
        scan,
        "outside the codex-alab binary the hook does nothing"
    );

    codex_alab_core::activate();
    shared.set_autopilot(Autopilot::Active);
    shared.observe(
        &Frame {
            primary_thread: Some("t1"),
            displayed_thread: Some("t1"),
            rollout_path: Some(Path::new("/r/rollout.jsonl")),
            cwd: Path::new("/work"),
            model: "gpt-5",
            codex_home: Path::new("/home/.codex"),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        10,
    );
    let (scan, admission) = (shared.upgrade_generation(), shared.admission_generation());
    codex_alab_tui::upgrade::native_event();
    assert_ne!(
        shared.upgrade_generation(),
        scan,
        "the event moves the scan"
    );
    assert_eq!(
        shared.admission_generation(),
        admission,
        "and leaves the native point an automatic turn's admission is fenced on"
    );
    assert!(
        !shared.commit_upgrade(scan),
        "the live upgrade refuses a scan the event came after"
    );
    assert!(!shared.exec_pending());
    let renewal = OwedAct {
        thread: "t1".into(),
        renewal: Some(AccountHandoff {
            purpose: Purpose::Login,
            destination: None,
            limit: None,
            openai_scope: true,
            local: false,
            account: "a".repeat(64),
            user: "b".repeat(64),
            receipt: Some("c".repeat(32)),
            turn: None,
            accepted: false,
            starts: 1,
            goal_id: None,
        }),
        ..Default::default()
    };
    assert!(
        shared.publish_renewal(admission, renewal.clone()),
        "a renewal decided before the event still publishes"
    );
    assert_eq!(shared.owed_act(), Some(renewal));
    assert!(
        shared.commit_upgrade(shared.upgrade_generation()),
        "POSITIVE CONTROL: nothing but the event had refused the commit"
    );
    assert!(shared.exec_pending());
}
