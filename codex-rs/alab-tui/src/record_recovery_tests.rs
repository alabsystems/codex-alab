// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recovery metadata is optional and independently forward compatible. The real writer
//! supplies it only for the primary thread and passes it through a same-identity re-exec.

use super::FakeCli;
use super::THREAD;
use super::frame;
use super::launch;
use super::shared_with;
use crate::record;
use crate::record::Record;
use crate::session::ProgramState;
use crate::session::Writer;
use clap::CommandFactory;
use codex_alab_core::session::InFlightRetry;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use std::time::Duration;

fn intent(wall: RecoveryWall) -> Recovery {
    Recovery {
        thread: THREAD.into(),
        turn_id: "failed-turn".into(),
        point_at_ms: 1_000,
        not_before_ms: 1_800_000,
        by_autopilot: true,
        by_person: false,
        wall,
    }
}

fn in_flight_intent() -> InFlightRetry {
    InFlightRetry {
        thread: THREAD.into(),
        turn_id: "waiting-native-turn".into(),
        not_before_ms: 1_800_000,
    }
}

#[test]
fn a_same_identity_reexec_keeps_owed_work_before_the_first_native_frame() {
    let home = tempfile::tempdir().expect("scratch records");
    let rollout = home.path().join("rollout.jsonl");
    std::fs::write(&rollout, "{}\n").expect("materialized rollout");
    let before = shared_with(launch(4242, true));
    before.observe(
        &frame(Some(&rollout), ProgramState::Idle, home.path()),
        1_000,
    );
    let owed = codex_alab_core::session::OwedAct {
        thread: THREAD.into(),
        native_origin: Some(codex_alab_core::session::NativeOrigin {
            turn_id: "the-native-point-this-act-observed".into(),
            phase: codex_alab_core::session::NativePhase::Complete,
        }),
        rule: Some("continue-done-check@v1".into()),
        text: Some("Defend it.".into()),
        mode: Some("cruise".into()),
        not_before_ms: 1_800_000,
        ..Default::default()
    };
    before.set_owed_act(Some(owed.clone()));
    let writer = Writer::new(
        before,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    );
    writer
        .finalize(&super::Ending::Upgrading)
        .expect("upgrading record");
    let original = record::read(&writer.path()).expect("original record");

    let after = shared_with(launch(4242, true));
    let after_writer = Writer::new(
        after,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    );
    after_writer.prepare();
    assert!(after.view().primary_thread.is_none(), "no native frame");
    after_writer.write_now().expect("early successor record");
    let early = record::read(&after_writer.path()).expect("successor record");
    assert_eq!(
        early.owed.map(codex_alab_core::session::OwedAct::from),
        Some(owed)
    );
    assert_eq!(early.session_id.as_deref(), Some(THREAD));
    assert_eq!(early.resume_argv, original.resume_argv);
    after.set_owed_act(None);
    after_writer.write_now().expect("manual clearing");
    assert!(
        record::read(&after_writer.path())
            .expect("cleared record")
            .owed
            .is_none(),
        "the builder does not reseed inherited work"
    );
}

#[test]
fn a_late_writer_cannot_seed_old_handoff_fields_after_native_acceptance() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    const OTHER_ROOT: &str = "019a0000-0000-7000-8000-000000000002";
    for reexec in [false, true] {
        for native_thread in [THREAD, OTHER_ROOT] {
            for boundary in [
                "not-started",
                "running",
                "completed",
                "stopped-before-start",
            ] {
                if boundary == "not-started" && native_thread != THREAD {
                    continue;
                }
                for pending in ["owed", "recovery", "in-flight"] {
                    let home = tempfile::tempdir().expect("scratch records");
                    let rollout = home.path().join("rollout.jsonl");
                    std::fs::write(&rollout, "{}\n").expect("materialized rollout");
                    let source_state = match pending {
                        "in-flight" => ProgramState::Working,
                        "recovery" => ProgramState::Error,
                        _ => ProgramState::Idle,
                    };
                    let old = shared_with(launch(1_000_000_001, true));
                    old.note_root_thread(THREAD);
                    old.observe(&frame(Some(&rollout), source_state, home.path()), 1_000);
                    let owed = (pending == "owed").then(|| codex_alab_core::session::OwedAct {
                        thread: THREAD.into(),
                        rule: Some("continue-done-check@v1".into()),
                        text: Some("An obsolete continuation.".into()),
                        mode: Some("cruise".into()),
                        not_before_ms: 1_800_000,
                        ..Default::default()
                    });
                    let recovery = (pending == "recovery").then(|| {
                        intent(RecoveryWall::Server {
                            what: "an obsolete capacity wait".into(),
                            cut_off: false,
                            retry_at_ms: Some(1_700_000),
                        })
                    });
                    let in_flight = (pending == "in-flight").then(in_flight_intent);
                    old.set_owed_act(owed.clone());
                    old.set_recovery(recovery.clone());
                    old.set_in_flight_retry(in_flight.clone());
                    let pace = codex_alab_core::session::Pace {
                        short_streak: 5,
                        ..codex_alab_core::session::Pace::default()
                    };
                    old.set_pace(THREAD, pace.clone());
                    // A busy retry can be carried only below the cap. The other
                    // sources exercise an exhausted chain and its genuine import.
                    let chain =
                        codex_alab_core::session::MAX_CARRY_ONS - u32::from(pending == "in-flight");
                    old.set_carried_on(chain);
                    let writer = Writer::new(
                        old,
                        home.path().into(),
                        Duration::ZERO,
                        Some(FakeCli::command),
                    );
                    writer.write_now().expect("original pending work");
                    let previous = record::read(&writer.path()).expect("old record");
                    let mut next_launch = launch(
                        if reexec {
                            previous.pid
                        } else {
                            previous.pid + 1
                        },
                        true,
                    );
                    next_launch.args = previous
                        .resume_argv
                        .expect("resume line")
                        .iter()
                        .map(std::ffi::OsString::from)
                        .collect();
                    next_launch.started_at_ms = previous.updated_at + 1;
                    let next = shared_with(next_launch);
                    next.note_root_thread(native_thread);
                    if boundary == "stopped-before-start" {
                        next.thread_ended(native_thread);
                    } else if boundary != "not-started" {
                        assert!(next.turn_started(native_thread, "accepted-after-the-old-image"));
                    }
                    if boundary == "completed" {
                        next.turn_ended(native_thread, "accepted-after-the-old-image");
                    }
                    // A person's or real native work's reset must survive a late
                    // sweep of another root, including after that new turn ended.
                    next.set_carried_on(0);
                    let current_state = match boundary {
                        "not-started" => source_state,
                        "running" => ProgramState::Working,
                        _ => ProgramState::Idle,
                    };
                    let mut current_frame = frame(Some(&rollout), current_state, home.path());
                    current_frame.primary_thread = Some(native_thread);
                    current_frame.displayed_thread = Some(native_thread);
                    next.observe(&current_frame, previous.updated_at + 1);
                    next.set_launch_witness(Some(LaunchWitness {
                        hand: Some("lease:aterm-harness@812".into()),
                        human: HumanSeen::Never,
                        read_at_ms: previous.updated_at + 2,
                        server_pid: Some(812),
                        hand_pid: HandPid::Absent,
                    }));
                    let next_writer = Writer::new(
                        next,
                        home.path().into(),
                        Duration::ZERO,
                        Some(FakeCli::command),
                    );
                    next_writer.prepare();
                    next_writer
                        .write_now()
                        .expect("record after late preparation");
                    let current = record::read(&next_writer.path()).expect("current record");
                    let genuine_startup = boundary == "not-started";
                    let context = format!(
                        "reexec={reexec}, native={native_thread}, boundary={boundary}, pending={pending}"
                    );
                    let expected_owed = genuine_startup.then_some(owed).flatten();
                    let expected_recovery = genuine_startup.then_some(recovery).flatten();
                    let expected_in_flight = genuine_startup.then_some(in_flight).flatten();
                    let expected_pace = genuine_startup.then_some(pace);
                    assert_eq!(next.owed_act(), expected_owed, "{context}");
                    assert_eq!(
                        current.owed.map(codex_alab_core::session::OwedAct::from),
                        expected_owed,
                        "{context}"
                    );
                    assert_eq!(next.recovery(), expected_recovery, "{context}");
                    assert_eq!(
                        current.recovery.map(Recovery::from),
                        expected_recovery,
                        "{context}"
                    );
                    assert_eq!(next.in_flight_retry(), expected_in_flight, "{context}");
                    assert_eq!(
                        current.in_flight_retry.map(InFlightRetry::from),
                        expected_in_flight,
                        "{context}"
                    );
                    assert_eq!(next.pace(THREAD), expected_pace, "{context}");
                    assert_eq!(
                        current.pace.map(codex_alab_core::session::Pace::from),
                        expected_pace,
                        "{context}"
                    );
                    assert_eq!(
                        next.inherited().is_some(),
                        genuine_startup && reexec,
                        "{context}: no obsolete future resume authority"
                    );
                    assert_eq!(
                        current.session_id.as_deref(),
                        Some(native_thread),
                        "{context}"
                    );
                    assert_eq!(
                        next.carried_on(),
                        if genuine_startup { chain } else { 0 },
                        "{context}"
                    );
                    assert_eq!(
                        current.carried_on,
                        if genuine_startup { chain } else { 0 },
                        "{context}: an old image cannot replace the new native boundary"
                    );
                }
            }
        }
    }
}

#[test]
fn writer_round_trips_in_flight_deadlines_without_turning_them_into_recovery() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Working, home.path()), 1_000);
    let retry = in_flight_intent();
    shared.set_in_flight_retry(Some(retry.clone()));
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    writer.write_now().expect("persist native wait");
    let stored = record::read(&writer.path()).expect("record");
    assert_eq!(
        stored.in_flight_retry.map(InFlightRetry::from),
        Some(retry.clone())
    );
    assert!(
        stored.recovery.is_none(),
        "an active request is not a failed turn"
    );

    let next = shared_with(launch(4242, true));
    let next_writer = Writer::new(next, home.path().into(), Duration::ZERO, None);
    next_writer.prepare();
    assert_eq!(
        next.inherited().and_then(|old| old.in_flight_retry.clone()),
        Some(retry)
    );

    shared.set_in_flight_retry(Some(InFlightRetry {
        thread: "another-thread".into(),
        ..in_flight_intent()
    }));
    writer.write_now().expect("unrelated wait");
    assert!(
        record::read(&writer.path())
            .expect("record")
            .in_flight_retry
            .is_none()
    );
}

#[test]
fn malformed_in_flight_metadata_discards_only_the_optional_field() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Working, home.path()), 1_000);
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    writer.write_now().expect("record");
    let original = record::read(&writer.path()).expect("record");
    let json = serde_json::to_value(&original).expect("JSON");
    assert!(json.get("inFlightRetry").is_none(), "older v1 shape");
    for bad in [
        serde_json::Value::Null,
        serde_json::json!({"thread":THREAD,"turnId":"turn","notBefore":"later"}),
        serde_json::json!({"thread":" ","turnId":"turn","notBefore":1}),
        serde_json::json!({"thread":THREAD,"turnId":"","notBefore":1}),
        serde_json::json!({"thread":THREAD,"turnId":"turn","notBefore":0}),
        serde_json::json!({"thread":THREAD,"turnId":"x".repeat(513),"notBefore":1}),
        serde_json::json!({"kind":"future-request-wait"}),
    ] {
        let mut candidate = json.clone();
        candidate["inFlightRetry"] = bad;
        assert_eq!(
            serde_json::from_value::<Record>(candidate).expect("record remains valid"),
            original
        );
    }
}

/// A `backgroundTerminals` this reader does not know (anything but a JSON integer within `u32`)
/// discards only that field, so the start's sweep still reads a predecessor's record (a crash
/// witness) that carries one; a count it knows reads as written.
#[test]
fn a_malformed_background_terminal_count_discards_only_the_count() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1_000);
    shared.set_background_terminals(3, std::time::Instant::now());
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    writer.write_now().expect("record");
    let original = record::read(&writer.path()).expect("record");
    assert_eq!(original.background_terminals, Some(3));
    let json = serde_json::to_value(&original).expect("JSON");
    let parse = |candidate: &serde_json::Value| {
        let bytes = serde_json::to_vec(candidate).expect("bytes");
        serde_json::from_slice::<Record>(&bytes)
    };
    let expected = Record {
        background_terminals: None,
        ..original
    };
    for bad in [
        serde_json::Value::Null,
        serde_json::json!("2"),
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!(4_294_967_296_u64),
        serde_json::json!([2]),
    ] {
        let mut candidate = json.clone();
        candidate["backgroundTerminals"] = bad.clone();
        let read = parse(&candidate).unwrap_or_else(|err| panic!("{bad}: {err}"));
        assert_eq!(read, expected, "{bad} discards only the count");
    }
    let mut largest = json.clone();
    largest["backgroundTerminals"] = serde_json::json!(4_294_967_295_u64);
    assert_eq!(
        parse(&largest)
            .expect("the largest count")
            .background_terminals,
        Some(u32::MAX)
    );
    let mut older = json;
    older
        .as_object_mut()
        .expect("object")
        .remove("backgroundTerminals");
    assert_eq!(parse(&older).expect("an older v1 record"), expected);
}

#[test]
fn malformed_native_origin_discards_owed_without_falling_back_to_legacy() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1_000);
    shared.set_owed_act(Some(codex_alab_core::session::OwedAct {
        thread: THREAD.into(),
        rule: Some("continue@v1".into()),
        text: Some("keep these words".into()),
        mode: Some("cruise".into()),
        ..Default::default()
    }));
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    writer.write_now().expect("record");
    let original = record::read(&writer.path()).expect("record");
    let json = serde_json::to_value(&original).expect("JSON");
    assert!(
        json["owed"].get("nativeOrigin").is_none(),
        "legacy shape remains readable"
    );
    for bad in [
        serde_json::Value::Null,
        serde_json::json!("complete"),
        serde_json::json!({"turnId":"turn", "phase":"future"}),
        serde_json::json!({"turnId":"turn"}),
        serde_json::json!({"turnId":"", "phase":"complete"}),
        serde_json::json!({"turnId":" ", "phase":"complete"}),
        serde_json::json!({"turnId":"x".repeat(513), "phase":"complete"}),
        serde_json::json!({"turnId":7, "phase":"running"}),
    ] {
        let mut candidate = json.clone();
        candidate["owed"]["nativeOrigin"] = bad;
        let read = serde_json::from_value::<Record>(candidate).expect("record remains readable");
        assert!(
            read.owed.is_none(),
            "malformed origin must not become unbound legacy intent"
        );
        assert_eq!(
            read,
            Record {
                owed: None,
                ..original.clone()
            }
        );
    }
    for phase in ["running", "complete", "interrupted", "aborted"] {
        let mut candidate = json.clone();
        candidate["owed"]["nativeOrigin"] = serde_json::json!({"turnId":"turn", "phase":phase});
        let read = serde_json::from_value::<Record>(candidate).expect("known native origin");
        let core = read.owed.expect("owed preserved");
        let roundtrip = record::Owed::from(codex_alab_core::session::OwedAct::from(core.clone()));
        assert_eq!(roundtrip, core);
    }
}

#[test]
fn in_flight_metadata_waits_for_the_same_launch_proof_before_a_second_crash() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    for framed in [false, true] {
        for case in [
            "automatic",
            "person",
            "sigint",
            "cleared",
            "off",
            "chain",
            "idle",
        ] {
            let preserved = matches!(case, "automatic" | "off");
            let home = tempfile::tempdir().expect("scratch records");
            let rollout = home.path().join("rollout.jsonl");
            std::fs::write(&rollout, "{}\n").expect("materialized rollout");
            let old = shared_with(launch(1_000_000_001, true));
            old.observe(
                &frame(Some(&rollout), ProgramState::Working, home.path()),
                1_000,
            );
            let retry = in_flight_intent();
            old.set_in_flight_retry(Some(retry.clone()));
            let writer = Writer::new(
                old,
                home.path().into(),
                Duration::ZERO,
                Some(FakeCli::command),
            );
            writer.write_now().expect("original native wait");
            let mut previous = record::read(&writer.path()).expect("original record");
            if case == "sigint" {
                previous.exit = Some("signal:2".into());
            }
            if case == "chain" {
                previous.carried_on = codex_alab_core::session::MAX_CARRY_ONS;
            }
            if case == "idle" {
                previous.status = record::Status::Idle;
            }
            record::write_atomic(home.path(), &previous).expect("source variant");
            let args = previous.resume_argv.expect("actual resume line");
            let started = previous.updated_at + 1;
            let mut next_launch = launch(1_000_000_002, true);
            next_launch.args = args.iter().map(std::ffi::OsString::from).collect();
            next_launch.started_at_ms = started;
            let next = shared_with(next_launch);
            let next_writer = Writer::new(
                next,
                home.path().into(),
                Duration::ZERO,
                Some(FakeCli::command),
            );
            next_writer.prepare();
            if framed {
                next.observe(
                    &frame(Some(&rollout), ProgramState::Working, home.path()),
                    started,
                );
            }
            next_writer.write_now().expect("before launch proof");
            assert!(
                record::read(&next_writer.path())
                    .expect("unproven record")
                    .in_flight_retry
                    .is_none(),
                "{case}, framed={framed}"
            );
            if case == "cleared" {
                next.set_in_flight_retry(None);
            }
            if case == "off" {
                next.set_autopilot(codex_alab_core::session::Autopilot::Off);
            }
            next.set_launch_witness(Some(LaunchWitness {
                hand: Some("lease:aterm-harness@812".into()),
                human: if case == "person" {
                    HumanSeen::At(started - 1)
                } else {
                    HumanSeen::Never
                },
                read_at_ms: started + 1,
                server_pid: Some(812),
                hand_pid: HandPid::Absent,
            }));
            next_writer.write_now().expect("after launch proof");
            let current = record::read(&next_writer.path()).expect("proven record");
            assert_eq!(
                current.in_flight_retry.map(InFlightRetry::from),
                preserved.then_some(retry),
                "{case}, framed={framed}"
            );
            if preserved {
                assert_eq!(
                    current.status,
                    record::Status::Busy,
                    "early crash retains busy provenance"
                );
                assert_eq!(current.session_id.as_deref(), Some(THREAD));
                assert_eq!(current.resume_argv, Some(args));
            }
        }
    }
}

#[test]
fn the_unique_newest_record_selects_all_retry_metadata_even_when_it_is_empty() {
    for tied in [false, true] {
        let home = tempfile::tempdir().expect("scratch records");
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").expect("materialized rollout");
        let old = shared_with(launch(1_000_000_001, true));
        old.observe(
            &frame(Some(&rollout), ProgramState::Working, home.path()),
            1_000,
        );
        old.set_in_flight_retry(Some(in_flight_intent()));
        old.set_carried_on(codex_alab_core::session::MAX_CARRY_ONS);
        let writer = Writer::new(
            old,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        writer.write_now().expect("old wait");
        let previous = record::read(&writer.path()).expect("old record");
        let mut newest = previous;
        newest.pid += 1;
        newest.updated_at += u64::from(!tied);
        newest.in_flight_retry = None;
        newest.carried_on = 0;
        record::write_atomic(home.path(), &newest).expect("new clearing");
        let started = newest.updated_at + 1;
        let mut next_launch = launch(1_000_000_003, true);
        next_launch.args = newest
            .resume_argv
            .expect("actual resume line")
            .iter()
            .map(std::ffi::OsString::from)
            .collect();
        next_launch.started_at_ms = started;
        let next = shared_with(next_launch);
        let next_writer = Writer::new(
            next,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        next_writer.prepare();
        assert!(
            next.in_flight_retry().is_none(),
            "tied={tied}: no stale native wait"
        );
        assert_eq!(
            next.carried_on(),
            if tied {
                codex_alab_core::session::MAX_CARRY_ONS
            } else {
                0
            },
            "only a uniquely newer record can prove that real work reset the chain"
        );
    }
}

#[test]
fn writer_round_trips_recovery_and_reexec_keeps_the_original_deadline() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Error, home.path()), 1_000);
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    let server = intent(RecoveryWall::Server {
        what: "model at capacity".into(),
        cut_off: true,
        retry_at_ms: Some(1_700_000),
    });
    shared.set_recovery(Some(server.clone()));
    writer.write_now().expect("persist failure wait");
    let record = record::read(&writer.path()).expect("read writer output");
    assert_eq!(record.recovery.map(Recovery::from), Some(server.clone()));

    let next = shared_with(launch(4242, true));
    let next_writer = Writer::new(next, home.path().into(), Duration::ZERO, None);
    next_writer.prepare();
    assert_eq!(
        next.inherited().and_then(|old| old.recovery.clone()),
        Some(server)
    );

    let usage = intent(RecoveryWall::Usage {
        quota: Some(codex_alab_core::quota::Scope {
            account: "test-account".into(),
            bucket: "codex".into(),
            user: Some("test-user".into()),
            ordinary_retry: true,
            ordinary_denied: true,
        }),
        reset_at_ms: Some(1_600_000),
        reset_label: Some("original reset".into()),
    });
    shared.set_recovery(Some(usage.clone()));
    writer.write_now().expect("persist usage wait");
    assert_eq!(
        record::read(&writer.path())
            .and_then(|record| record.recovery)
            .map(Recovery::from),
        Some(usage)
    );
    let mut legacy = serde_json::to_value(record::read(&writer.path()).unwrap()).unwrap();
    legacy["recovery"]["wall"]
        .as_object_mut()
        .unwrap()
        .remove("quota");
    let legacy: Record = serde_json::from_value(legacy).unwrap();
    let recovered: Recovery = legacy.recovery.unwrap().into();
    assert!(matches!(
        recovered.wall,
        RecoveryWall::Usage {
            quota: None,
            reset_at_ms: Some(1_600_000),
            ..
        }
    ));
    shared.set_recovery(Some(Recovery {
        thread: "another-thread".into(),
        ..intent(RecoveryWall::Usage {
            quota: None,
            reset_at_ms: None,
            reset_label: None,
        })
    }));
    writer.write_now().expect("unrelated thread");
    assert!(
        record::read(&writer.path())
            .expect("record")
            .recovery
            .is_none()
    );
}

#[test]
fn old_unknown_and_malformed_recovery_leave_the_rest_of_the_record_readable() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Error, home.path()), 1_000);
    shared.set_recovery(Some(intent(RecoveryWall::Server {
        what: "at capacity".into(),
        cut_off: false,
        retry_at_ms: Some(1_700_000),
    })));
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    writer.write_now().expect("record");
    let original = record::read(&writer.path()).expect("record");
    let mut old = serde_json::to_value(&original).expect("JSON");
    old.as_object_mut().expect("object").remove("recovery");
    let expected: Record = serde_json::from_value(old.clone()).expect("older v1 record");
    assert!(expected.recovery.is_none());
    for bad in [
        serde_json::Value::Null,
        serde_json::json!({"wall":{"kind":"a-future-provider-wait"}}),
        serde_json::json!({"thread":THREAD,"turnId":"failed-turn","pointAt":2,"notBefore":1,"byAutopilot":true,"byPerson":false,"wall":{"kind":"usage"}}),
        serde_json::json!({"thread":THREAD,"turnId":"failed-turn","pointAt":1,"notBefore":2,"byAutopilot":true,"byPerson":false,"wall":{"kind":"server","what":"at capacity","cutOff":false,"retryAt":"later"}}),
    ] {
        let mut candidate = old.clone();
        candidate["recovery"] = bad;
        assert_eq!(
            serde_json::from_value::<Record>(candidate).expect("only recovery is discarded"),
            expected
        );
    }
}

#[test]
fn long_provider_details_keep_classification_and_deadlines_in_a_readable_record() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Error, home.path()), 1_000);
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    let long = "界".repeat(100_000);
    for wall in [
        RecoveryWall::Server {
            what: long.clone(),
            cut_off: false,
            retry_at_ms: Some(1_700_000),
        },
        RecoveryWall::Usage {
            quota: None,
            reset_at_ms: Some(1_600_000),
            reset_label: Some(long),
        },
    ] {
        shared.set_recovery(Some(intent(wall)));
        writer.write_now().expect("bounded provider detail");
        let record = record::read(&writer.path()).expect("the whole session record stays readable");
        let recovered: Recovery = record
            .recovery
            .expect("recovery classification remains")
            .into();
        assert_eq!(recovered.turn_id, "failed-turn");
        assert_eq!(recovered.not_before_ms, 1_800_000);
        match recovered.wall {
            RecoveryWall::Server {
                what, retry_at_ms, ..
            } => {
                assert!(what.len() <= 4096);
                assert!(what.chars().all(|ch| ch == '界'));
                assert_eq!(retry_at_ms, Some(1_700_000));
            }
            RecoveryWall::Usage {
                reset_at_ms,
                reset_label,
                ..
            } => {
                let label = reset_label.expect("bounded label");
                assert!(label.len() <= 4096);
                assert!(label.chars().all(|ch| ch == '界'));
                assert_eq!(reset_at_ms, Some(1_600_000));
            }
            other => panic!("provider detail changed its classification: {other:?}"),
        }
    }
}

#[test]
fn a_proven_handoff_keeps_recovery_through_a_second_crash_before_any_root_frame() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    for (human, signal, preserved) in [
        (false, None, true),
        (true, None, false),
        (false, Some(2), false),
    ] {
        let home = tempfile::tempdir().expect("scratch records");
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").expect("materialized rollout");
        let old = shared_with(launch(1_000_000_001, true));
        old.observe(
            &frame(Some(&rollout), ProgramState::Error, home.path()),
            1_000,
        );
        let recovery = intent(RecoveryWall::Server {
            what: "model at capacity".into(),
            cut_off: false,
            retry_at_ms: Some(1_700_000),
        });
        old.set_recovery(Some(recovery.clone()));
        let writer = Writer::new(
            old,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        if let Some(signal) = signal {
            writer
                .finalize(&super::Ending::Signalled(signal))
                .expect("explicit stop");
        } else {
            writer.write_now().expect("original wait");
        }
        let previous = record::read(&writer.path()).expect("original record");
        let args = previous
            .resume_argv
            .clone()
            .expect("actual rebuilt resume line");
        let first_started = previous.updated_at + 1;
        let mut first_launch = launch(1_000_000_002, true);
        first_launch.args = args.iter().map(std::ffi::OsString::from).collect();
        first_launch.started_at_ms = first_started;
        let first = shared_with(first_launch);
        first.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: if human {
                HumanSeen::At(first_started - 1)
            } else {
                HumanSeen::Never
            },
            read_at_ms: first_started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        let first_writer = Writer::new(
            first,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        first_writer.prepare();
        let mut carried = recovery;
        if preserved {
            carried.not_before_ms += 500;
            first.set_recovery(Some(carried.clone()));
        }
        assert!(
            first.view().primary_thread.is_none(),
            "no resumed widget has initialized"
        );
        first_writer
            .write_now()
            .expect("pre-frame second crash record");
        let immediate = record::read(&first_writer.path()).expect("pre-frame record");
        assert_eq!(
            immediate.recovery.clone().map(Recovery::from),
            preserved.then_some(carried.clone())
        );
        if preserved {
            assert_eq!(immediate.session_id.as_deref(), Some(THREAD));
            assert_eq!(immediate.resume_argv, Some(args.clone()));
        }
        let second_started = immediate.updated_at.max(first_started) + 1;
        let mut second_launch = launch(1_000_000_003, true);
        second_launch.args = args.iter().map(std::ffi::OsString::from).collect();
        second_launch.started_at_ms = second_started;
        let second = shared_with(second_launch);
        second.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: HumanSeen::Never,
            read_at_ms: second_started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        let second_writer = Writer::new(
            second,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        second_writer.prepare();
        assert_eq!(second.recovery(), preserved.then_some(carried));
    }
}

#[test]
fn a_delayed_launch_probe_publishes_only_uncleared_proven_crash_recovery() {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    for framed in [false, true] {
        for case in [
            "automatic",
            "person",
            "sigint",
            "resume-cleared",
            "off-cleared",
        ] {
            let preserved = case == "automatic";
            let home = tempfile::tempdir().expect("scratch records");
            let rollout = home.path().join("rollout.jsonl");
            std::fs::write(&rollout, "{}\n").expect("materialized rollout");
            let old = shared_with(launch(1_000_000_001, true));
            old.observe(
                &frame(Some(&rollout), ProgramState::Error, home.path()),
                1_000,
            );
            let recovery = intent(RecoveryWall::Server {
                what: "model at capacity".into(),
                cut_off: false,
                retry_at_ms: Some(1_700_000),
            });
            old.set_recovery(Some(recovery.clone()));
            let writer = Writer::new(
                old,
                home.path().into(),
                Duration::ZERO,
                Some(FakeCli::command),
            );
            if case == "sigint" {
                writer
                    .finalize(&super::Ending::Signalled(2))
                    .expect("explicit stop");
            } else {
                writer.write_now().expect("original failure wait");
            }
            let previous = record::read(&writer.path()).expect("original record");
            let args = previous.resume_argv.expect("actual rebuilt resume line");
            let started = previous.updated_at + 1;
            let mut next_launch = launch(1_000_000_002, true);
            next_launch.args = args.iter().map(std::ffi::OsString::from).collect();
            next_launch.started_at_ms = started;
            let next = shared_with(next_launch);
            let next_writer = Writer::new(
                next,
                home.path().into(),
                Duration::ZERO,
                Some(FakeCli::command),
            );
            next_writer.prepare();
            if framed {
                next.observe(
                    &frame(Some(&rollout), ProgramState::Error, home.path()),
                    started,
                );
            }
            next_writer.write_now().expect("before the launch probe");
            assert!(
                record::read(&next_writer.path())
                    .expect("unverified record")
                    .recovery
                    .is_none(),
                "{case}, framed={framed}: no proof authorizes emission"
            );
            match case {
                "resume-cleared" => next.set_recovery(None),
                "off-cleared" => next.set_autopilot(Autopilot::Off),
                _ => {}
            }
            next.set_launch_witness(Some(LaunchWitness {
                hand: Some("lease:aterm-harness@812".into()),
                human: if case == "person" {
                    HumanSeen::At(started - 1)
                } else {
                    HumanSeen::Never
                },
                read_at_ms: started + 1,
                server_pid: Some(812),
                hand_pid: HandPid::Absent,
            }));
            next_writer.write_now().expect("after the launch probe");
            let current = record::read(&next_writer.path()).expect("probed record");
            assert_eq!(
                current.recovery.clone().map(Recovery::from),
                preserved.then_some(recovery.clone()),
                "{case}, framed={framed}"
            );
            let second_started = current.updated_at.max(started) + 1;
            let mut second_launch = launch(1_000_000_003, true);
            second_launch.args = args.iter().map(std::ffi::OsString::from).collect();
            second_launch.started_at_ms = second_started;
            let second = shared_with(second_launch);
            second.set_launch_witness(Some(LaunchWitness {
                hand: Some("lease:aterm-harness@812".into()),
                human: HumanSeen::Never,
                read_at_ms: second_started + 1,
                server_pid: Some(812),
                hand_pid: HandPid::Absent,
            }));
            let second_writer = Writer::new(
                second,
                home.path().into(),
                Duration::ZERO,
                Some(FakeCli::command),
            );
            second_writer.prepare();
            assert_eq!(
                second.recovery(),
                preserved.then_some(recovery),
                "{case}, framed={framed}: a second crash retains only authorized intent"
            );
        }
    }
}

#[test]
fn a_newer_matching_record_without_recovery_supersedes_an_older_failure() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    for off in [false, true] {
        let home = tempfile::tempdir().expect("scratch records");
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").expect("materialized rollout");
        let old = shared_with(launch(1_000_000_001, true));
        old.observe(
            &frame(Some(&rollout), ProgramState::Error, home.path()),
            1_000,
        );
        let recovery = intent(RecoveryWall::Server {
            what: "obsolete failed turn".into(),
            cut_off: false,
            retry_at_ms: Some(1_700_000),
        });
        old.set_recovery(Some(recovery.clone()));
        let writer = Writer::new(
            old,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        writer.write_now().expect("older unresolved wait");
        let mut newer = record::read(&writer.path()).expect("older record");
        let args = newer.resume_argv.clone().expect("actual resume line");
        newer.pid = 1_000_000_002;
        newer.updated_at += 1;
        newer.recovery = None;
        if off {
            newer.autopilot = record::Autopilot::Off;
        }
        record::write_atomic(home.path(), &newer).expect("newer cleared record");
        let started = newer.updated_at + 1;
        let mut next_launch = launch(1_000_000_003, true);
        next_launch.args = args.iter().map(std::ffi::OsString::from).collect();
        next_launch.started_at_ms = started;
        let next = shared_with(next_launch);
        let next_writer = Writer::new(
            next,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        next_writer.prepare();
        assert_eq!(
            next.predecessor(THREAD).expect("latest predecessor").pid,
            newer.pid
        );
        assert!(next.recovery().is_none(), "off={off}: newest clearing wins");
        next.observe(
            &frame(Some(&rollout), ProgramState::Error, home.path()),
            started,
        );
        next_writer.write_now().expect("unverified frame");
        assert!(
            record::read(&next_writer.path())
                .expect("unverified record")
                .recovery
                .is_none()
        );
        next.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: HumanSeen::Never,
            read_at_ms: started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        next_writer.write_now().expect("proven frame");
        assert!(
            record::read(&next_writer.path())
                .expect("proven record")
                .recovery
                .is_none(),
            "proof does not resurrect a superseded failure"
        );
        let local = Recovery {
            turn_id: "new-native-failed-turn".into(),
            ..recovery
        };
        next.set_recovery(Some(local.clone()));
        next_writer.write_now().expect("fresh local failure");
        assert_eq!(
            record::read(&next_writer.path())
                .and_then(|record| record.recovery)
                .map(Recovery::from),
            Some(local),
            "a fresh native failure remains persistable"
        );
    }
}

/// A NETWORK RECOVERY SAYS WHETHER ITS ROUTE WAS MEASURED (B2; CONTRACT §4): `measured` is
/// written only where the process measured the wall's route, read back as written, and a
/// record before the key reads as not measured. NEGATIVE CONTROL: a route not measured writes
/// no key.
#[test]
fn a_network_recovery_carries_whether_its_route_was_measured() {
    let home = tempfile::tempdir().expect("scratch records");
    let shared = shared_with(launch(4242, true));
    shared.observe(&frame(None, ProgramState::Error, home.path()), 1_000);
    let writer = Writer::new(shared, home.path().into(), Duration::ZERO, None);
    for measured in [true, false] {
        let network = intent(RecoveryWall::Network {
            what: "an API error (the API could not be reached)".into(),
            measured,
        });
        shared.set_recovery(Some(network.clone()));
        writer.write_now().expect("persist the network wait");
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(writer.path()).expect("the record file"))
                .expect("JSON");
        assert_eq!(
            raw["recovery"]["wall"].get("measured"),
            measured.then_some(&serde_json::Value::Bool(true)),
            "{raw}"
        );
        let record = record::read(&writer.path()).expect("read writer output");
        assert_eq!(record.recovery.map(Recovery::from), Some(network));
    }
    // A record from before the key.
    let old = serde_json::json!({"thread":THREAD,"turnId":"failed-turn","pointAt":1,
        "notBefore":2,"byAutopilot":true,"byPerson":false,
        "wall":{"kind":"network","what":"unreachable"}});
    let read: record::Recovery = serde_json::from_value(old).expect("an older network recovery");
    assert_eq!(
        read.wall,
        record::RecoveryWall::Network {
            what: "unreachable".into(),
            measured: false,
        }
    );
}

#[test]
fn subscription_recovery_keeps_identity_and_reads_legacy_records() {
    let original = intent(RecoveryWall::Entitlement {
        quota: Some(codex_alab_core::quota::Scope {
            account: "account".into(),
            user: Some("user".into()),
            bucket: "codex".into(),
            ordinary_retry: false,
            ordinary_denied: true,
        }),
    });
    let stored = record::Recovery::from(original.clone());
    let mut json = serde_json::to_value(&stored).unwrap();
    let read: record::Recovery = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(Recovery::from(read), original);
    json["wall"].as_object_mut().unwrap().remove("quota");
    let legacy: record::Recovery = serde_json::from_value(json).unwrap();
    assert!(matches!(
        Recovery::from(legacy).wall,
        RecoveryWall::Entitlement { quota: None }
    ));
}
