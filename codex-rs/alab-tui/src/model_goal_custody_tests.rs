// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Goal provenance survives a native reply without importing obsolete turn intent.

use super::*;
use codex_alab_core::session::Pace;
use codex_alab_core::session::WallGoal;
use pretty_assertions::assert_eq;

fn goal() -> WallGoal {
    WallGoal {
        goal_id: "original-goal".into(),
        objective: "finish the original task".into(),
        token_budget: Some(1_000),
        chronology_fence_generation: None,
        turn: "model-blocked-turn".into(),
        person_stops: 0,
    }
}

fn native_reply(shared: &Shared) {
    assert!(shared.turn_started(THREAD, "unrelated-reply"));
    shared.set_pace(
        THREAD,
        Pace {
            short_streak: 3,
            ..Pace::default()
        },
    );
}

fn source(dir: &Path, pid: u32, at: u64, has_goal: bool, session: &str) {
    let line = ["/twin/codex", "resume", THREAD];
    dead_record(dir, pid, THREAD, &line, at);
    let path = dir.join(format!("{pid}.json"));
    let mut value = read(&path);
    value["atermSession"] = session.into();
    value["pace"] = serde_json::to_value(record::Pace::from(Pace {
        short_streak: 99,
        model_blocked_goal: has_goal.then(goal),
        ..Pace::default()
    }))
    .expect("source pace");
    std::fs::write(&path, serde_json::to_vec(&value).expect("source record"))
        .expect("write source");
}

#[test]
fn same_identity_model_custody_survives_a_reply_before_record_preparation() {
    let home = tempfile::tempdir().expect("home");
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    let old = shared_with(launch(pid, true));
    old.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    old.set_pace(
        THREAD,
        Pace {
            short_streak: 99,
            model_blocked_goal: Some(goal()),
            ..Pace::default()
        },
    );
    Writer::new(old, dir.clone(), Duration::ZERO, None)
        .finalize(&Ending::Upgrading)
        .expect("previous image");

    let shared = shared_with(launch(pid, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 2);
    native_reply(shared);
    let writer = Writer::new(shared, dir, Duration::ZERO, None);
    writer.prepare();
    assert!(
        shared.inherited().is_none(),
        "old turn intent stays superseded"
    );
    assert_eq!(shared.carried_model_goal(THREAD), Some(goal()));
    assert_eq!(
        shared.pace(THREAD),
        Some(Pace {
            short_streak: 3,
            model_blocked_goal: Some(goal()),
            ..Pace::default()
        })
    );
    writer.write_now().expect("write successor");
    assert_eq!(
        record::read(&writer.path())
            .and_then(|record| record.pace)
            .map(Pace::from)
            .and_then(|pace| pace.model_blocked_goal),
        Some(goal()),
        "custody is durable before the runtime validates history"
    );
}

#[test]
fn a_same_identity_empty_model_source_supersedes_a_dead_positive_source() {
    let home = tempfile::tempdir().expect("home");
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    let mut own = launch(pid, true);
    own.args = ["/twin/codex", "resume", THREAD]
        .map(OsString::from)
        .to_vec();
    let at = own.started_at_ms - 1_000;
    let old = shared_with(own.clone());
    old.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    Writer::new(old, dir.clone(), Duration::ZERO, None)
        .finalize(&Ending::Upgrading)
        .expect("empty previous image");
    source(&dir, 999_981, at, true, "fake");
    let shared = shared_with(own);
    native_reply(shared);
    Writer::new(shared, dir, Duration::ZERO, None).prepare();
    assert!(
        shared.predecessor(THREAD).is_some(),
        "dead source was discovered"
    );
    assert!(
        shared.inherited().is_none(),
        "native work keeps old turn state superseded"
    );
    assert_eq!(shared.carried_model_goal(THREAD), None);
    assert_eq!(
        shared.pace(THREAD).and_then(|pace| pace.model_blocked_goal),
        None
    );
}

#[test]
fn only_the_unique_newest_record_can_supply_late_model_custody() {
    for case in ["newest-goal", "newest-empty", "tied"] {
        let home = tempfile::tempdir().expect("home");
        let dir = record::sessions_dir(home.path());
        record::ensure_dir(&dir).expect("sessions directory");
        let mut own = launch(4282, true);
        own.args = ["/twin/codex", "resume", THREAD]
            .map(OsString::from)
            .to_vec();
        let at = own.started_at_ms - 1_000;
        source(&dir, 999_982, at, case != "newest-goal", "fake");
        source(
            &dir,
            999_983,
            at + u64::from(case != "tied"),
            case == "newest-goal",
            "fake",
        );
        let shared = shared_with(own);
        native_reply(shared);
        let writer = Writer::new(shared, dir, Duration::ZERO, None);
        writer.prepare();
        assert_eq!(
            shared.carried_model_goal(THREAD),
            (case == "newest-goal").then(goal),
            "{case}"
        );
        assert_eq!(shared.pace(THREAD).map(|pace| pace.short_streak), Some(3));
        assert!(shared.owed_act().is_none());
    }
}

#[test]
fn a_late_model_source_waits_for_launch_proof_and_cannot_reappear_after_retirement() {
    for retired in [false, true] {
        let home = tempfile::tempdir().expect("home");
        let dir = record::sessions_dir(home.path());
        record::ensure_dir(&dir).expect("sessions directory");
        let mut own = launch(4283, true);
        own.args = ["/twin/codex", "resume", THREAD]
            .map(OsString::from)
            .to_vec();
        let started = own.started_at_ms;
        source(&dir, 999_984, started - 1_000, true, "another-tab");
        let shared = shared_with(own);
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
        native_reply(shared);
        let writer = Writer::new(shared, dir, Duration::ZERO, None);
        writer.prepare();
        assert_eq!(shared.carried_model_goal(THREAD), None);
        writer.write_now().expect("unproven successor");
        assert!(
            writer
                .path()
                .parent()
                .expect("sessions")
                .join("999984.json")
                .exists(),
            "an unproven source stays durable until launch qualification settles"
        );
        if retired {
            shared.retire_carried_model_goal(THREAD);
        }
        shared.set_launch_witness(Some(codex_alab_core::session::LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: codex_alab_core::session::HumanSeen::Never,
            read_at_ms: started + 1,
            server_pid: Some(812),
            hand_pid: codex_alab_core::session::HandPid::Absent,
        }));
        writer.write_now().expect("proven successor");
        assert_eq!(shared.carried_model_goal(THREAD), (!retired).then(goal));
        assert_eq!(shared.pace(THREAD).map(|pace| pace.short_streak), Some(3));
    }
}
