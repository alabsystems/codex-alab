// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A process-death handoff stays in custody before its successor's first frame or record.
//! These cases leave the OS running; they do not test OS-crash or power-loss persistence.

use super::FakeCli;
use super::THREAD;
use super::frame;
use super::launch;
use super::shared_with;
use crate::record;
use crate::session::ProgramState;
use crate::session::Writer;
use clap::CommandFactory;
use codex_alab_core::session::HandPid;
use codex_alab_core::session::HumanSeen;
use codex_alab_core::session::LaunchWitness;
use codex_alab_core::session::NativeOrigin;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_alab_core::session::Shared;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

struct Handoff {
    source: record::Record,
    owed: OwedAct,
    first: &'static Shared,
    writer: Writer,
}

fn handoff(dir: &Path) -> Handoff {
    let rollout = dir.join("rollout.jsonl");
    std::fs::write(&rollout, "{}\n").expect("materialized native history");
    let old = shared_with(launch(1_000_000_001, true));
    old.observe(&frame(Some(&rollout), ProgramState::Idle, dir), 1_000);
    let owed = OwedAct {
        thread: THREAD.into(),
        native_origin: Some(NativeOrigin {
            turn_id: "completed-native-turn".into(),
            phase: NativePhase::Complete,
        }),
        rule: Some("continue-done-check@v1".into()),
        text: Some("Defend it.".into()),
        mode: Some("cruise".into()),
        not_before_ms: codex_alab_core::session::now_ms() + 240_000,
        ..Default::default()
    };
    old.set_owed_act(Some(owed.clone()));
    old.set_pace(
        THREAD,
        codex_alab_core::session::Pace {
            short_streak: 2,
            ..Default::default()
        },
    );
    let old_writer = Writer::new(old, dir.into(), Duration::ZERO, Some(FakeCli::command));
    old_writer.write_now().expect("original scheduled act");
    let source = record::read(&old_writer.path()).expect("original durable handoff");
    let first = successor(&source, 1_000_000_002);
    let writer = Writer::new(
        first,
        dir.into(),
        Duration::from_secs(3600),
        Some(FakeCli::command),
    );
    writer.prepare();
    Handoff {
        source,
        owed,
        first,
        writer,
    }
}

fn successor(source: &record::Record, pid: u32) -> &'static Shared {
    let mut next = launch(pid, true);
    next.args = source
        .resume_argv
        .as_ref()
        .expect("actual resume line")
        .iter()
        .map(OsString::from)
        .collect();
    next.started_at_ms = source.updated_at + 1;
    shared_with(next)
}

fn prove(shared: &Shared) {
    let started = shared.launch().expect("launch").started_at_ms;
    shared.set_launch_witness(Some(LaunchWitness {
        hand: Some("lease:aterm-harness@812".into()),
        human: HumanSeen::Never,
        read_at_ms: started + 1,
        server_pid: Some(812),
        hand_pid: HandPid::Pid(812),
    }));
}

#[test]
fn durable_handoff_retains_an_adopted_owed_deadline_before_the_first_frame() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff {
        source,
        mut owed,
        first,
        writer,
    } = handoff(home.path());
    prove(first);
    // The monotonic/epoch round trip is deliberately conservative by a millisecond.
    owed.not_before_ms += 1;
    first.set_owed_act(Some(owed.clone()));
    writer.write_now().expect("adopted handoff write");
    let current = record::read(&writer.path()).expect("successor record");
    assert_eq!(current.owed.map(OwedAct::from), Some(owed));
    assert_eq!(current.session_id, source.session_id);
    assert_eq!(current.resume_argv, source.resume_argv);
}

#[test]
fn durable_handoff_survives_another_crash_while_only_an_initializer_was_written() {
    for (framed, draft_empty) in [(false, true), (true, true), (true, false)] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            source,
            owed,
            first,
            writer,
        } = handoff(home.path());
        if framed {
            let rollout = home.path().join("rollout.jsonl");
            let mut current = frame(Some(&rollout), ProgramState::Idle, home.path());
            current.draft_empty = draft_empty;
            first.set_background_terminals(2, std::time::Instant::now());
            first.observe(&current, source.updated_at + 1);
        }
        writer.write_now().expect("unproven startup initializer");
        let initializer = record::read(&writer.path()).expect("initializer");
        let wire: serde_json::Value = serde_json::from_slice(
            &std::fs::read(writer.path()).expect("the actual host-visible record"),
        )
        .expect("record JSON");
        for key in ["sessionId", "resumeArgv", "cwd", "state", "exit"] {
            assert_eq!(
                wire.get(key),
                Some(&serde_json::Value::Null),
                "aterm's initializer contract requires explicit null {key}"
            );
        }
        assert!(
            wire.get("backgroundTerminals").is_none(),
            "a count is a frame fact the initializer does not carry"
        );
        assert!(initializer.resume_argv.is_none());
        assert!(
            initializer.session_id.is_none(),
            "an unprobed restored Frame cannot publish a contradictory partial identity"
        );
        assert!(initializer.displayed_thread_id.is_none());
        assert!(
            initializer.cwd.is_none()
                && initializer.state.is_none()
                && initializer.blocked_kind.is_none()
        );
        assert_eq!(initializer.status, record::Status::Busy);
        assert_eq!(
            initializer.draft_empty, draft_empty,
            "a real draft must not become eligible for host initializer inheritance"
        );
        assert_eq!(
            first.view().primary_thread.as_deref(),
            framed.then_some(THREAD),
            "serialization leaves native facts intact"
        );
        assert!(
            initializer.owed.is_none(),
            "no proof may publish the old act"
        );
        let mut chain = source;
        chain.updated_at = chain.updated_at.max(initializer.updated_at);
        let second = successor(&chain, 1_000_000_003);
        let second_writer = Writer::new(
            second,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        second_writer.prepare();
        assert_eq!(
            second.owed_act(),
            Some(owed),
            "a startup initializer cannot consume the only durable intent"
        );
    }
}

#[test]
fn durable_handoff_initializer_preserves_queued_pending_and_unobserved_input() {
    for input in ["queued", "pending", "key"] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            source,
            first,
            writer,
            ..
        } = handoff(home.path());
        let rollout = home.path().join("rollout.jsonl");
        let mut current = frame(Some(&rollout), ProgramState::Idle, home.path());
        current.queued_input = input == "queued";
        current.pending_user_turn = input == "pending";
        first.observe(&current, source.updated_at + 1);
        if input == "key" {
            first.human_input(source.updated_at + 2);
        }
        writer.write_now().expect("unproven record with input");
        let initializer = record::read(&writer.path()).expect("record");
        assert_eq!(initializer.status, record::Status::Busy);
        assert!(initializer.session_id.is_none() && initializer.resume_argv.is_none());
        assert!(
            !initializer.draft_empty,
            "pending input cannot qualify for aterm's empty initializer fallback ({input})"
        );
        assert_eq!(first.queued_input(), input == "queued");
        assert_eq!(first.pending_user_turn(), input == "pending");
        assert!(record::path_for(home.path(), source.pid).exists());
    }
}

#[test]
fn durable_handoff_materializes_its_authenticated_root_without_a_rollout_frame() {
    for same_root in [false, true] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            source,
            first,
            writer,
            ..
        } = handoff(home.path());
        prove(first);
        let thread = if same_root {
            THREAD
        } else {
            "019a0000-0000-7000-8000-000000000002"
        };
        let mut current = frame(None, ProgramState::Idle, home.path());
        current.primary_thread = Some(thread);
        current.displayed_thread = Some(thread);
        first.observe(&current, source.updated_at + 1);
        writer
            .write_now()
            .expect("first Frame without its rollout path yet");
        let current = record::read(&writer.path()).expect("record");
        assert_eq!(current.session_id.as_deref(), Some(thread));
        assert_eq!(
            current.resume_argv,
            same_root.then_some(source.resume_argv).flatten(),
            "only the authenticated source root was already materialized"
        );
    }
}

#[test]
fn durable_handoff_keeps_the_source_through_a_failed_successor_write() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff {
        source,
        owed,
        first,
        writer,
    } = handoff(home.path());
    prove(first);
    std::fs::create_dir(writer.path()).expect("make rename fail at the real write boundary");
    assert!(writer.write_now().is_err());
    assert_eq!(
        record::read(&record::path_for(home.path(), source.pid)),
        Some(source.clone())
    );
    std::fs::remove_dir(writer.path()).expect("remove failed successor target");
    let second = successor(&source, 1_000_000_003);
    let second_writer = Writer::new(
        second,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    );
    second_writer.prepare();
    assert_eq!(second.owed_act(), Some(owed));
}

#[test]
fn durable_handoff_does_not_resurrect_an_explicit_clear_or_new_native_turn() {
    for native in [
        None,
        Some(THREAD),
        Some("019a0000-0000-7000-8000-000000000002"),
    ] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            source,
            first,
            writer,
            ..
        } = handoff(home.path());
        if let Some(thread) = native {
            assert!(first.turn_started(thread, "newer-native-turn"));
        } else {
            prove(first);
            first.set_owed_act(None);
        }
        writer.write_now().expect("durable supersession");
        let current = record::read(&writer.path()).expect("clearing record");
        assert!(current.owed.is_none());
        assert_eq!(
            current.session_id.as_deref(),
            native.or(Some(THREAD)),
            "the actual accepted root precedes the bootstrap identity"
        );
        assert!(current.resume_argv.is_some());
        if native.is_some() {
            assert_eq!(current.status, record::Status::Busy);
        }
        assert!(
            !record::path_for(home.path(), source.pid).exists(),
            "the durable clearing retires the old intent (native={native:?})"
        );
        let later = current;
        let second = successor(&later, 1_000_000_003);
        let second_writer = Writer::new(
            second,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        second_writer.prepare();
        assert!(
            second.owed_act().is_none(),
            "a later crash cannot revive cleared work (native={native:?})"
        );
    }
}

#[test]
fn durable_handoff_does_not_remove_a_source_replaced_at_the_same_pid_path() {
    for replace_identity in [false, true] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            mut source,
            first,
            writer,
            ..
        } = handoff(home.path());
        prove(first);
        if replace_identity {
            source.identity = None;
        } else {
            source.updated_at += 1;
            source.owed = None;
        }
        writer
            .write_now_with(|| {
                record::write_atomic(home.path(), &source).expect(
                    "another owner replaced the captured source after successor publication",
                );
            })
            .expect("successor commit");
        assert_eq!(
            record::read(&record::path_for(home.path(), source.pid)),
            Some(source)
        );
    }
}

#[test]
fn durable_handoff_quarantine_restores_mismatches_without_overwriting_a_newer_record() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff { source, .. } = handoff(home.path());
    let mut replacement = source.clone();
    replacement.updated_at += 1;
    replacement.owed = None;
    record::write_atomic(home.path(), &replacement).expect("replacement identity's record");
    let mut newer = replacement.clone();
    newer.updated_at += 1;
    assert!(
        !record::retire_if_unchanged(
            home.path(),
            &source,
            |_, _| false,
            || {
                record::write_atomic(home.path(), &newer)
                    .expect("new canonical record while quarantine is validated");
            }
        )
        .expect("preserve both newer records")
    );
    assert_eq!(
        record::read(&record::path_for(home.path(), source.pid)),
        Some(newer)
    );
    assert!(
        std::fs::read_dir(home.path())
            .expect("records")
            .flatten()
            .any(|entry| record::read(&entry.path()).as_ref() == Some(&replacement)),
        "the mismatched captured record is preserved"
    );
}

#[test]
fn durable_handoff_quarantine_restores_a_now_live_identity() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff { source, .. } = handoff(home.path());
    assert!(
        !record::retire_if_unchanged(home.path(), &source, |_, _| true, || {})
            .expect("live identity is restored")
    );
    assert_eq!(
        record::read(&record::path_for(home.path(), source.pid)),
        Some(source)
    );
}

#[test]
fn durable_handoff_retires_a_source_left_only_in_quarantine() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff {
        source,
        first,
        writer,
        ..
    } = handoff(home.path());
    let quarantine = home
        .path()
        .join(format!("{}.json.interrupted.tmp", source.pid));
    std::fs::rename(record::path_for(home.path(), source.pid), &quarantine)
        .expect("interrupted prior cleanup");
    // A second process can discover the temporary through the normal census.
    let next = successor(&source, 1_000_000_003);
    let next_writer = Writer::new(
        next,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    );
    next_writer.prepare();
    assert!(next.owed_act().is_some());
    prove(next);
    next.set_owed_act(None);
    next_writer
        .write_now()
        .expect("durable explicit clearing retires its temporary source");
    assert!(!quarantine.exists());
    assert!(
        first.owed_act().is_some(),
        "other runtime state was not mutated"
    );
    let _ = writer;
    let current = record::read(&next_writer.path()).expect("clearing record");
    let last = successor(&current, 1_000_000_004);
    Writer::new(
        last,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    )
    .prepare();
    assert!(last.owed_act().is_none());
}

#[test]
fn durable_handoff_multiple_native_roots_serialize_only_an_exhausted_empty_revocation() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff {
        source,
        first,
        writer,
        ..
    } = handoff(home.path());
    for (thread, turn) in [
        ("019a0000-0000-7000-8000-000000000002", "new-b"),
        ("019a0000-0000-7000-8000-000000000003", "new-c"),
    ] {
        assert!(first.turn_started(thread, turn));
    }
    writer
        .write_now()
        .expect("durable ambiguous native supersession");
    let current = record::read(&writer.path()).expect("empty bootstrap revocation");
    assert_eq!(current.session_id.as_deref(), Some(THREAD));
    assert_eq!(current.resume_argv, source.resume_argv);
    assert_eq!(current.status, record::Status::Busy);
    assert!(
        current.owed.is_none()
            && current.recovery.is_none()
            && current.in_flight_retry.is_none()
            && current.pace.is_none()
    );
    assert!(
        current.carried_on >= codex_alab_core::session::MAX_CARRY_ONS,
        "a busy revocation cannot authorize generic carry-on"
    );
    assert_eq!(
        first.carried_on(),
        0,
        "do not exhaust the live threads' chain"
    );
    assert!(!record::path_for(home.path(), source.pid).exists());
    let second = successor(&current, 1_000_000_003);
    Writer::new(
        second,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    )
    .prepare();
    assert!(second.owed_act().is_none());
    assert!(second.carried_on() >= codex_alab_core::session::MAX_CARRY_ONS);
    // Once the actual primary appears, the writer publishes its true identity
    // and the live chain rather than keeping the conservative bootstrap bound.
    let actual_rollout = home.path().join("rollout.jsonl");
    let mut actual = frame(Some(&actual_rollout), ProgramState::Working, home.path());
    actual.primary_thread = Some("019a0000-0000-7000-8000-000000000002");
    actual.displayed_thread = actual.primary_thread;
    first.observe(&actual, current.updated_at + 1);
    writer
        .write_now()
        .expect("actual primary supersedes bootstrap revocation");
    let actual = record::read(&writer.path()).expect("actual primary record");
    assert_eq!(
        actual.session_id.as_deref(),
        Some("019a0000-0000-7000-8000-000000000002")
    );
    assert_eq!(actual.carried_on, first.carried_on());
}

#[test]
fn durable_handoff_identical_quarantine_links_are_one_snapshot_not_a_tie() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff { source, owed, .. } = handoff(home.path());
    let temporary = home
        .path()
        .join(format!("{}.json.restored.tmp", source.pid));
    std::fs::hard_link(record::path_for(home.path(), source.pid), &temporary)
        .expect("restore interrupted before removing quarantine");
    let next = successor(&source, 1_000_000_003);
    let next_writer = Writer::new(
        next,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    );
    next_writer.prepare();
    assert_eq!(next.owed_act(), Some(owed));
    assert!(
        next.predecessor(THREAD).is_some(),
        "duplicate paths do not make provenance ambiguous"
    );
    prove(next);
    next_writer.write_now().expect("new durable snapshot");
    assert!(!temporary.exists());
    assert!(!record::path_for(home.path(), source.pid).exists());
}

#[test]
fn orderly_handoff_cleanup_failure_cannot_resurrect_old_work_after_a_sibling_sweep() {
    for framed in [
        None,
        Some(THREAD),
        Some("019a0000-0000-7000-8000-000000000002"),
    ] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            source,
            first,
            writer,
            ..
        } = handoff(home.path());
        if let Some(thread) = framed {
            let rollout = home.path().join("rollout.jsonl");
            let mut current = frame(Some(&rollout), ProgramState::Idle, home.path());
            current.primary_thread = Some(thread);
            current.displayed_thread = Some(thread);
            first.observe(&current, source.updated_at + 1);
        }
        assert!(
            writer
                .finalize_with(&super::Ending::Remove, || {
                    Err(std::io::Error::other(
                        "cleanup interrupted before it could retire the source",
                    ))
                })
                .is_err()
        );
        let tombstone =
            record::read(&writer.path()).expect("cleanup failure keeps explicit clearing");
        assert_eq!(tombstone.exit.as_deref(), Some(record::HANDOFF_CLEARED));
        assert_eq!(tombstone.status, record::Status::Exited);
        assert_eq!(tombstone.session_id.as_deref(), Some(THREAD));
        assert_eq!(tombstone.resume_argv, source.resume_argv);
        assert!(
            tombstone.owed.is_none()
                && tombstone.recovery.is_none()
                && tombstone.in_flight_retry.is_none()
                && tombstone.pace.is_none()
        );
        let started = codex_alab_core::session::now_ms() + 1;
        let other = [
            OsString::from("codex"),
            OsString::from("resume"),
            OsString::from("another-tab"),
        ];
        record::sweep_with(
            home.path(),
            1_000_000_009,
            started,
            |_, _| false,
            |record| record::judge(record, &other, started, 86_400_000),
            |_, _| {},
        );
        assert!(
            writer.path().exists(),
            "a sibling cannot erase unretired clearing"
        );
        assert_eq!(
            record::judge(
                &tombstone,
                &other,
                tombstone.updated_at + 86_400_000,
                86_400_000
            ),
            record::Sweep::Leave
        );
        assert_eq!(
            record::judge(
                &tombstone,
                &other,
                tombstone.updated_at + 86_400_001,
                86_400_000
            ),
            record::Sweep::Remove
        );
        let mut source = source;
        source.updated_at = started;
        let next = successor(&source, 1_000_000_003);
        let next_writer = Writer::new(
            next,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        next_writer.prepare();
        assert!(
            next.owed_act().is_none(),
            "an orderly exit remains cleared even if cleanup failed (framed={framed:?})"
        );
        assert!(
            next.carried_on() >= codex_alab_core::session::MAX_CARRY_ONS,
            "an early restart after the clearing cannot authorize generic carry-on"
        );
    }
}

#[test]
fn orderly_handoff_success_removes_both_sides_only_after_the_clearing_is_durable() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff { source, writer, .. } = handoff(home.path());
    writer
        .finalize_with(&super::Ending::Remove, || {
            let tombstone = record::read(&writer.path()).expect("clearing precedes retirement");
            assert_eq!(tombstone.exit.as_deref(), Some(record::HANDOFF_CLEARED));
            assert!(tombstone.owed.is_none());
            assert!(record::path_for(home.path(), source.pid).exists());
            Ok(())
        })
        .expect("successful orderly cleanup");
    assert!(!writer.path().exists());
    assert!(!record::path_for(home.path(), source.pid).exists());
}

#[test]
fn durable_handoff_native_acceptance_still_supersedes_old_intent_during_signal_teardown() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff {
        source,
        first,
        writer,
        ..
    } = handoff(home.path());
    let current_root = "019a0000-0000-7000-8000-000000000002";
    assert!(first.turn_started(current_root, "accepted-before-hup"));
    assert!(first.set_signal(1));
    first.thread_ended(THREAD);
    first.thread_ended(current_root);
    writer
        .finalize(&super::Ending::Signalled(1))
        .expect("signal snapshot of accepted work");
    let current = record::read(&writer.path()).expect("durable native supersession");
    assert!(current.owed.is_none());
    assert!(current.carried_on >= codex_alab_core::session::MAX_CARRY_ONS);
    assert!(!record::path_for(home.path(), source.pid).exists());
    let next = successor(&current, 1_000_000_003);
    Writer::new(
        next,
        home.path().into(),
        Duration::ZERO,
        Some(FakeCli::command),
    )
    .prepare();
    assert!(next.owed_act().is_none());
    assert!(next.carried_on() >= codex_alab_core::session::MAX_CARRY_ONS);
}

#[test]
fn durable_handoff_keeps_an_unprobed_source_during_relaunchable_signal_teardown() {
    for framed in [false, true] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            source,
            owed,
            first,
            writer,
        } = handoff(home.path());
        if framed {
            first.observe(
                &frame(
                    Some(&home.path().join("rollout.jsonl")),
                    ProgramState::Idle,
                    home.path(),
                ),
                source.updated_at + 1,
            );
        }
        assert!(first.set_signal(15));
        first.thread_ended(THREAD);
        writer
            .finalize(&super::Ending::Signalled(15))
            .expect("relaunchable initializer");
        assert!(record::path_for(home.path(), source.pid).exists());
        let second = successor(&source, 1_000_000_003);
        let second_writer = Writer::new(
            second,
            home.path().into(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        second_writer.prepare();
        assert_eq!(second.owed_act(), Some(owed));
    }
}

#[test]
fn durable_handoff_does_not_retire_an_initializer_for_native_work_after_its_snapshot() {
    let home = tempfile::tempdir().expect("scratch records");
    let Handoff {
        source,
        first,
        writer,
        ..
    } = handoff(home.path());
    writer
        .write_now_with(|| {
            assert!(first.turn_started(THREAD, "accepted-after-the-write"));
        })
        .expect("initializer write completed before native acceptance");
    let initializer = record::read(&writer.path()).expect("committed initializer");
    assert!(initializer.resume_argv.is_none());
    assert!(
        record::path_for(home.path(), source.pid).exists(),
        "new live work cannot grant retirement to earlier bytes"
    );
    writer
        .write_now()
        .expect("write the actual accepted native identity");
    let current = record::read(&writer.path()).expect("new native snapshot");
    assert_eq!(current.session_id.as_deref(), Some(THREAD));
    assert!(current.resume_argv.is_some());
    assert_eq!(current.status, record::Status::Busy);
    assert!(current.owed.is_none());
    assert!(!record::path_for(home.path(), source.pid).exists());
}

#[test]
fn durable_handoff_does_not_bind_changed_pre_frame_intent_to_a_bootstrap_identity() {
    for change in [
        "earlier", "native", "rule", "text", "mode", "repo", "goal", "landing", "landed", "thread",
    ] {
        let home = tempfile::tempdir().expect("scratch records");
        let Handoff {
            mut owed,
            first,
            writer,
            ..
        } = handoff(home.path());
        prove(first);
        match change {
            "earlier" => owed.not_before_ms -= 1,
            "native" => {
                owed.native_origin
                    .as_mut()
                    .expect("bound native point")
                    .turn_id = "other-native-turn".into()
            }
            "rule" => owed.rule = Some("other-rule@v1".into()),
            "text" => owed.text = Some("different words".into()),
            "mode" => owed.mode = Some("land".into()),
            "repo" => owed.repo_owed = Some(vec!["different work".into()]),
            "goal" => owed.resume_goal = true,
            "landing" => owed.landing = true,
            "landed" => owed.landed = true,
            "thread" => owed.thread = "019a0000-0000-7000-8000-000000000002".into(),
            _ => unreachable!(),
        }
        first.set_owed_act(Some(owed));
        writer
            .write_now()
            .expect("bootstrap identity without changed authority");
        assert!(
            record::read(&writer.path()).expect("record").owed.is_none(),
            "{change}"
        );
    }
}
