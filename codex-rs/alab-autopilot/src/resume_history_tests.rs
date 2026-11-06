// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native stop/interrupt hooks can publish an older terminal after a newer start.

use super::NativePoint;
use super::ResumeSource;
use super::latest;
use crate::decide::Act;
use crate::decide::Mode;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::decide::Texts;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::decide;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use codex_history::RolloutItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use serde_json::json;
use std::time::Duration;
use std::time::Instant;

const CURRENT: &str = "the-current-native-turn";
const OLD: &str = "the-older-native-turn";
const EPOCH: u64 = 1_800_000_000_000;

fn policy() -> Policy {
    Policy {
        continue_on: true,
        continue_text: Some("continue the task".into()),
        continue_per_hour: None,
        approve_all: true,
        answer_questions: true,
        answer_text: None,
        questions: Questions::Recommended,
        retry_api_errors: true,
        probe_api: true,
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        upgrade_ask: false,
        facts: false,
        human_grace: Duration::ZERO,
        autopilot: Mode::Cruise,
        texts: Texts::default(),
    }
}

fn event(value: serde_json::Value) -> RolloutItem {
    RolloutItem::EventMsg(serde_json::from_value::<EventMsg>(value).expect("native event"))
}

fn started(turn: &str) -> RolloutItem {
    event(
        json!({"type":"task_started", "turn_id":turn, "model_context_window":258400,
        "collaboration_mode_kind":"default",
        "turn_attribution": {"turn_id": turn, "turn_trigger": "user"}}),
    )
}

fn aborted(turn: Option<&str>) -> RolloutItem {
    event(json!({"type":"turn_aborted", "turn_id":turn, "reason":"interrupted"}))
}

fn completed(turn: &str, info: CodexErrorInfo) -> RolloutItem {
    event(json!({"type":"task_complete", "turn_id":turn,
        "error":{"message":"the provider ended this turn", "codex_error_info":info}}))
}

/// A turn codex-alab typed (its `turn_trigger`), cut before any terminal.
fn ours(turn: &str) -> RolloutItem {
    event(
        json!({"type":"task_started", "turn_id":turn, "model_context_window":258400,
        "collaboration_mode_kind":"default",
        "turn_attribution": {"turn_id": turn, "turn_trigger": "alab_autopilot"}}),
    )
}

/// A turn that ended cleanly (no error: no point a resume restores).
fn done(turn: &str) -> RolloutItem {
    event(json!({"type":"task_complete", "turn_id":turn, "last_agent_message":"Stage 1 is in."}))
}

fn regular_input() -> RolloutItem {
    event(
        json!({"type":"user_message", "message":"authorized independent work",
        "images":null, "local_images":[], "text_elements":[]}),
    )
}

fn compaction_boundary(turn: &str, id: &str, started: bool) -> RolloutItem {
    event(json!({
        "type": if started { "item_started" } else { "item_completed" },
        "thread_id":"00000000-0000-4000-8000-000000000001", "turn_id":turn,
        "item":{"type":"ContextCompaction", "id":id}, "started_at_ms":1, "completed_at_ms":2
    }))
}

#[test]
fn compaction_policy_refusals_never_authorize_replaying_history_after_resume() {
    let now = Instant::now();
    for input in [false, true] {
        for completion in [None, Some((OLD, "pending")), Some((CURRENT, "unrelated"))] {
            let mut items = vec![started(OLD), regular_input(), started(CURRENT)];
            if input {
                items.push(regular_input());
            }
            items.push(compaction_boundary(CURRENT, "pending", true));
            if let Some((turn, id)) = completion {
                items.push(compaction_boundary(turn, id, false));
            }
            items.push(completed(CURRENT, CodexErrorInfo::CyberPolicy));
            let mut state = ThreadState::default();
            crate::extension::resume_into(&mut state, &items, ResumeSource::Crash, None, 0, now);
            assert!(matches!(
                state.point.as_ref().and_then(|point| point.wall.as_ref()),
                Some(crate::decide::Wall::Fatal { .. })
            ));
            assert!(matches!(
                decide(&state, &policy(), &Timing::default(), now, Duration::ZERO).act,
                Act::Attention { .. }
            ));
        }
    }
    // A pre/standalone compaction may hold no input at all. An earlier turn's input
    // or a foreign paginated UserMessage cannot grant a regular sampling continuation.
    for extra in [
        None,
        Some(event(
            json!({"type":"item_completed", "thread_id":"00000000-0000-4000-8000-000000000001", "turn_id":OLD,
        "item":{"type":"UserMessage", "id":"foreign", "content":[]}, "started_at_ms":1, "completed_at_ms":2}),
        )),
    ] {
        let mut items = vec![started(OLD), regular_input(), started(CURRENT)];
        if let Some(extra) = extra {
            items.push(extra);
        }
        items.push(completed(CURRENT, CodexErrorInfo::CyberPolicy));
        let mut state = ThreadState::default();
        crate::extension::resume_into(&mut state, &items, ResumeSource::Crash, None, 0, now);
        assert!(matches!(
            state.point.as_ref().and_then(|point| point.wall.as_ref()),
            Some(crate::decide::Wall::Fatal { .. })
        ));
    }
    // A successful matching compaction does not misclassify a later sampling refusal.
    let items = vec![
        started(CURRENT),
        regular_input(),
        compaction_boundary(CURRENT, "done", true),
        compaction_boundary(CURRENT, "done", false),
        completed(CURRENT, CodexErrorInfo::CyberPolicy),
    ];
    let mut state = ThreadState::default();
    crate::extension::resume_into(&mut state, &items, ResumeSource::Crash, None, 0, now);
    assert!(matches!(
        decide(&state, &policy(), &Timing::default(), now, Duration::ZERO).act,
        Act::Type {
            rule: crate::words::RULE_POLICY_CONTINUE,
            ..
        }
    ));
}

#[test]
fn an_attributed_default_person_message_resets_the_policy_sequence_but_a_legacy_start_does_not() {
    for attributed in [false, true] {
        let mut items = vec![
            started(OLD),
            regular_input(),
            completed(OLD, CodexErrorInfo::CyberPolicy),
            event(
                json!({"type":"task_started", "turn_id":"safe", "model_context_window":258400,
                "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":"safe", "turn_trigger":crate::extension::TURN_TRIGGER}}),
            ),
            event(
                json!({"type":"user_message", "message":crate::words::POLICY_CONTINUE, "images":null, "local_images":[], "text_elements":[]}),
            ),
        ];
        let mut start = json!({"type":"task_started", "turn_id":CURRENT,"model_context_window":258400,"collaboration_mode_kind":"default"});
        if attributed {
            start["turn_attribution"] = json!({"turn_id":CURRENT});
        }
        items.push(event(start));
        assert!(
            super::policy_continued(&items),
            "a start without its regular input cannot reset the guard"
        );
        items.push(event(json!({"type":"item_completed", "thread_id":"00000000-0000-4000-8000-000000000001", "turn_id":OLD,
            "item":{"type":"UserMessage", "id":"foreign-person-input", "content":[]}, "started_at_ms":1, "completed_at_ms":2})));
        assert!(
            super::policy_continued(&items),
            "a foreign person's input cannot reset the current sequence"
        );
        items.push(regular_input());
        items.push(completed(CURRENT, CodexErrorInfo::CyberPolicy));
        assert_eq!(super::policy_continued(&items), !attributed);
        let mut state = ThreadState::default();
        crate::extension::resume_into(
            &mut state,
            &items,
            ResumeSource::Crash,
            None,
            0,
            Instant::now(),
        );
        assert_eq!(
            matches!(
                decide(
                    &state,
                    &policy(),
                    &Timing::default(),
                    Instant::now(),
                    Duration::ZERO
                )
                .act,
                Act::Type {
                    rule: crate::words::RULE_POLICY_CONTINUE,
                    ..
                }
            ),
            attributed
        );
    }
}

#[test]
fn a_matching_earlier_assistant_exhaustion_marker_survives_restart() {
    for source in [
        "assistant",
        "paginated",
        "foreign",
        "user",
        "quoted",
        "older",
    ] {
        let mut items = vec![
            started(OLD),
            regular_input(),
            event(json!({"type":"agent_message", "message":crate::words::POLICY_EXHAUSTED})),
            completed(OLD, CodexErrorInfo::CyberPolicy),
            event(
                json!({"type":"task_started", "turn_id":CURRENT,"model_context_window":258400,
                "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":CURRENT,"turn_trigger":crate::extension::TURN_TRIGGER}}),
            ),
            regular_input(),
        ];
        let text = if source == "quoted" {
            format!("The stop marker is {}.", crate::words::POLICY_EXHAUSTED)
        } else {
            crate::words::POLICY_EXHAUSTED.into()
        };
        match source {
            "assistant" | "quoted" => items.push(event(json!({"type":"agent_message","message":text}))),
            "paginated" | "foreign" => items.push(event(json!({"type":"item_completed", "thread_id":"00000000-0000-4000-8000-000000000001",
                "turn_id":if source == "foreign" { OLD } else { CURRENT }, "item":{"type":"AgentMessage","id":"commentary","content":[{"type":"Text","text":text}],"phase":"commentary"},"started_at_ms":1,"completed_at_ms":2}))),
            "user" => items.push(event(json!({"type":"user_message","message":text,"images":null,"local_images":[],"text_elements":[]}))),
            "older" => {},
            _ => unreachable!(),
        }
        items.push(event(json!({"type":"task_complete","turn_id":CURRENT,"last_agent_message":"The refused action remains excluded."})));
        let exhausted = matches!(source, "assistant" | "paginated");
        assert_eq!(
            super::terminal_policy_exhausted(&items),
            exhausted,
            "{source}"
        );
        let now = Instant::now();
        let mut state = ThreadState::default();
        crate::extension::resume_into(&mut state, &items, ResumeSource::Crash, None, 0, now);
        assert!(state.policy_continued);
        assert_eq!(
            matches!(
                decide(&state, &policy(), &Timing::default(), now, Duration::ZERO).act,
                Act::Attention { .. }
            ),
            exhausted,
            "{source}"
        );
        let owed = crate::decide::Owed {
            native_origin: Some(codex_alab_core::session::NativeOrigin {
                turn_id: CURRENT.into(),
                phase: codex_alab_core::session::NativePhase::Complete,
            }),
            rule: crate::words::RULE_CONTINUE,
            text: "continue the task".into(),
            not_before: now,
            mode: Mode::Cruise,
            repo_owed: None,
            landing: false,
            resume_goal: false,
        };
        let mut owed_state = ThreadState::default();
        crate::extension::resume_into(
            &mut owed_state,
            &items,
            ResumeSource::Other,
            Some((Some(owed), false)),
            0,
            now,
        );
        assert!(owed_state.point.as_ref().is_some_and(|p| p.restored));
        assert_eq!(
            matches!(
                decide(
                    &owed_state,
                    &policy(),
                    &Timing::default(),
                    now,
                    Duration::ZERO
                )
                .act,
                Act::Attention { .. }
            ),
            exhausted,
            "{source} (owed point)"
        );
        let mut failed_items = items.clone();
        *failed_items.last_mut().expect("terminal") =
            completed(CURRENT, CodexErrorInfo::ServerOverloaded);
        let mut recovered_state = ThreadState::default();
        let timing = Timing::default();
        crate::extension::resume_bound_with(
            &mut recovered_state,
            &failed_items,
            ResumeSource::Crash,
            Some(crate::owed_origin::BoundHandoff::empty()),
            0,
            now,
            |state| crate::recovery::restore(state, &failed_items, &saved(), now, EPOCH, &timing),
        );
        assert!(recovered_state.point.as_ref().is_some_and(|p| p.restored));
        assert_eq!(
            matches!(
                decide(&recovered_state, &policy(), &timing, now, Duration::ZERO).act,
                Act::Attention { .. }
            ),
            exhausted,
            "{source} (recovered point)"
        );
    }
}

fn saved() -> Recovery {
    Recovery {
        thread: "thread".into(),
        turn_id: CURRENT.into(),
        point_at_ms: EPOCH,
        not_before_ms: EPOCH + 80_000,
        by_autopilot: true,
        by_person: false,
        wall: RecoveryWall::Server {
            what: "model at capacity".into(),
            cut_off: false,
            retry_at_ms: None,
        },
    }
}

#[test]
fn a_late_retired_abort_preserves_the_current_failed_turn_and_recovery() {
    let now = Instant::now();
    let timing = Timing::default();
    for abort_after_completion in [false, true] {
        let mut items = vec![started(OLD), started(CURRENT), regular_input()];
        let complete = completed(CURRENT, CodexErrorInfo::ServerOverloaded);
        if abort_after_completion {
            items.push(complete.clone());
        }
        items.push(aborted(Some(OLD)));
        if !abort_after_completion {
            items.push(complete);
        }
        assert!(
            matches!(latest(&items).point, NativePoint::Complete(complete) if complete.turn_id == CURRENT),
            "a late terminal belongs to its retired native attempt"
        );
        let mut state = ThreadState::default();
        let mut recovered = false;
        assert!(crate::extension::resume_bound_with(
            &mut state,
            &items,
            false,
            Some(crate::owed_origin::BoundHandoff::empty()),
            0,
            now,
            |state| {
                recovered = crate::recovery::restore(state, &items, &saved(), now, EPOCH, &timing);
                recovered
            }
        ));
        assert!(
            recovered,
            "the latest matching provider failure remains recoverable"
        );
        let decision = decide(&state, &policy(), &timing, now, Duration::ZERO);
        assert!(matches!(
            decision.act,
            Act::Type {
                rule: crate::words::RULE_API_RETRY,
                ..
            }
        ));
        assert_eq!(decision.not_before, Some(now + Duration::from_secs(80)));
        assert_eq!(state.point_turn_id.as_deref(), Some(CURRENT));
    }
}

#[test]
fn a_late_retired_abort_cannot_hide_the_current_policy_refusal() {
    let now = Instant::now();
    for abort_after_completion in [false, true] {
        let mut items = vec![started(OLD), started(CURRENT), regular_input()];
        let complete = completed(CURRENT, CodexErrorInfo::CyberPolicy);
        if abort_after_completion {
            items.push(complete.clone());
        }
        items.push(aborted(Some(OLD)));
        if !abort_after_completion {
            items.push(complete);
        }
        let mut state = ThreadState::default();
        crate::extension::resume_into(&mut state, &items, true, None, 0, now);
        assert!(matches!(
            decide(&state, &policy(), &Timing::default(), now, Duration::ZERO).act,
            Act::Type {
                rule: crate::words::RULE_POLICY_CONTINUE,
                ..
            }
        ));
        assert!(!state.carry_on);
        assert!(state.owed.is_none());
        assert_eq!(state.point_turn_id.as_deref(), Some(CURRENT));
    }
}

#[test]
fn native_policy_continuation_stays_spent_after_success_compaction_and_restart() {
    let now = Instant::now();
    let mut items = vec![
        started(OLD),
        completed(OLD, CodexErrorInfo::CyberPolicy),
        event(
            json!({"type":"task_started", "turn_id":"safe-policy-turn", "model_context_window":258400,
            "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":"safe-policy-turn", "turn_trigger":crate::extension::TURN_TRIGGER}}),
        ),
        event(
            json!({"type":"user_message", "message":crate::words::POLICY_CONTINUE,
            "images":null, "local_images":[], "text_elements":[]}),
        ),
        event(
            json!({"type":"task_complete", "turn_id":"safe-policy-turn", "last_agent_message":"Reviewed independently allowed tests."}),
        ),
        serde_json::from_value(
            json!({"type":"compacted", "payload":{"message":"a compacted summary"}}),
        )
        .expect("native compaction"),
        event(
            json!({"type":"task_started", "turn_id":CURRENT, "model_context_window":258400,
            "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":CURRENT, "turn_trigger":crate::extension::TURN_TRIGGER}}),
        ),
        completed(CURRENT, CodexErrorInfo::CyberPolicy),
    ];
    for source in [
        ResumeSource::Other,
        ResumeSource::Crash,
        ResumeSource::Signal,
    ] {
        let mut state = ThreadState::default();
        crate::extension::resume_into(&mut state, &items, source, None, 0, now);
        assert!(state.policy_continued);
        assert!(matches!(
            decide(&state, &policy(), &Timing::default(), now, Duration::ZERO).act,
            Act::Attention { .. }
        ));
    }
    // A later unknown/legacy attribution cannot accidentally reset the boundary.
    items.push(event(
        json!({"type":"task_started", "turn_id":"unknown", "model_context_window":258400,
        "collaboration_mode_kind":"default"}),
    ));
    assert!(super::policy_continued(&items));
    items.push(started("corrected-by-person"));
    assert!(!super::policy_continued(&items));
    // The person quoting our text is no evidence of an automatic continuation.
    items.push(event(
        json!({"type":"user_message", "message":crate::words::POLICY_CONTINUE,
        "images":null, "local_images":[], "text_elements":[]}),
    ));
    assert!(!super::policy_continued(&items));
}

#[test]
fn a_policy_continuation_crash_before_its_message_is_flushed_cannot_spend_it_again() {
    let now = Instant::now();
    let items = vec![
        started(OLD),
        completed(OLD, CodexErrorInfo::CyberPolicy),
        event(
            json!({"type":"task_started", "turn_id":CURRENT, "model_context_window":258400,
            "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":CURRENT, "turn_trigger":crate::extension::TURN_TRIGGER}}),
        ),
    ];
    let mut state = ThreadState::default();
    crate::extension::resume_into(&mut state, &items, ResumeSource::Crash, None, 0, now);
    assert!(
        state.policy_continued,
        "the native start already spent the bounded continuation"
    );
    let decision = decide(&state, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(
        matches!(decision.act, Act::Type { rule: crate::words::RULE_HOST_TURN, ref text }
        if text.ends_with(crate::words::POLICY_BOUNDARY))
    );
    let mut items = items;
    items.push(completed(CURRENT, CodexErrorInfo::CyberPolicy));
    let mut state = ThreadState::default();
    crate::extension::resume_into(&mut state, &items, ResumeSource::Crash, None, 0, now);
    assert!(matches!(
        decide(&state, &policy(), &Timing::default(), now, Duration::ZERO).act,
        Act::Attention { .. }
    ));
}

#[test]
fn compaction_cannot_discard_the_selected_program_and_a_new_message_can_select_standard() {
    use codex_protocol::turn_input::CyberAccessProgram;
    let message = || {
        event(
            json!({"type":"user_message", "message":"independent work", "images":null,
        "local_images":[], "text_elements":[]}),
        )
    };
    let context = |turn: &str, program: Option<CyberAccessProgram>| {
        serde_json::from_value(json!({
        "type":"turn_context", "payload":{"turn_id":turn, "cwd":"/tmp", "approval_policy":"never",
            "sandbox_policy":{"type":"danger-full-access"}, "model":"gpt-5.6-sol", "cyber_access_program":program}
    })).expect("native context")
    };
    let mut items = vec![
        started(OLD),
        context(OLD, Some(CyberAccessProgram::DaybreakBlue)),
        message(),
        event(
            json!({"type":"task_started", "turn_id":"compaction", "model_context_window":258400,
            "collaboration_mode_kind":"default"}),
        ),
        context("compaction", None),
    ];
    assert_eq!(
        super::cyber_program(&items),
        Some(CyberAccessProgram::DaybreakBlue)
    );
    items.extend([started(CURRENT), context(CURRENT, None), message()]);
    assert_eq!(
        super::cyber_program(&items),
        None,
        "an explicit regular message's selection is authoritative"
    );
}

#[test]
fn missing_unknown_or_current_abort_identity_still_revokes_recovery_and_signal_carry_on() {
    let now = Instant::now();
    for turn in [None, Some("an-unknown-turn"), Some(CURRENT)] {
        let items = [
            started(OLD),
            started(CURRENT),
            aborted(turn),
            completed(CURRENT, CodexErrorInfo::ServerOverloaded),
        ];
        assert!(matches!(latest(&items).point, NativePoint::Aborted(_)));
        let mut state = ThreadState::default();
        let mut recovered = true;
        crate::extension::resume_bound_with(
            &mut state,
            &items,
            ResumeSource::Signal,
            Some(crate::owed_origin::BoundHandoff::empty()),
            0,
            now,
            |state| {
                recovered = crate::recovery::restore(
                    state,
                    &items,
                    &saved(),
                    now,
                    EPOCH,
                    &Timing::default(),
                );
                recovered
            },
        );
        assert!(!recovered);
        assert!(!state.carry_on);
        assert!(state.owed.is_none());
        assert_eq!(
            decide(&state, &policy(), &Timing::default(), now, Duration::ZERO).act,
            Act::Nothing
        );
    }
}

#[test]
fn a_reused_current_turn_id_is_not_retired() {
    let items = [
        started(CURRENT),
        started(OLD),
        started(CURRENT),
        aborted(Some(CURRENT)),
    ];
    assert!(
        matches!(latest(&items).point, NativePoint::Aborted(aborted) if aborted.turn_id.as_deref() == Some(CURRENT))
    );
}

#[test]
fn malformed_older_ids_cannot_prove_that_an_abort_is_retired() {
    let too_long = "x".repeat(513);
    for old in ["", " ", too_long.as_str()] {
        let items = [
            started(old),
            started(CURRENT),
            aborted(Some(old)),
            completed(CURRENT, CodexErrorInfo::ServerOverloaded),
        ];
        assert!(matches!(latest(&items).point, NativePoint::Aborted(_)));
        let mut state = ThreadState::default();
        state.observe_resume(true, None, false);
        assert!(!crate::recovery::restore(
            &mut state,
            &items,
            &saved(),
            Instant::now(),
            EPOCH,
            &Timing::default()
        ));
    }
}

/// A HISTORY THAT WORKED PUTS THE RESUMED SESSION PAST ITS OPENING (aterm 791977cae8's
/// `seed_worked` and `point_seed`; gap 32): a turn that ended or an act of codex-alab's, cut or
/// not, so under Land a person's quick message after the resume is carried, not left to them.
/// A turn that merely ran is no work, nor the opening turn a restart cut: one a crash left with
/// no terminal, or the running turn a signal's teardown interrupted (an interrupt a resume of
/// any other kind reads is a turn that ended). NEGATIVE CONTROL: the opening turn a crash cut
/// leaves the person's message theirs.
#[test]
fn a_history_that_worked_puts_the_resumed_session_past_its_opening() {
    let now = Instant::now();
    for (items, source, past) in [
        (vec![], ResumeSource::Other, false),
        (vec![started(CURRENT)], ResumeSource::Crash, false),
        (
            vec![started(CURRENT), aborted(Some(CURRENT))],
            ResumeSource::Signal,
            false,
        ),
        (
            vec![started(CURRENT), aborted(Some(CURRENT))],
            ResumeSource::Other,
            true,
        ),
        (
            vec![
                started(OLD),
                done(OLD),
                started(CURRENT),
                aborted(Some(CURRENT)),
            ],
            ResumeSource::Signal,
            true,
        ),
        (vec![ours(CURRENT)], ResumeSource::Crash, true),
        (vec![started(OLD), done(OLD)], ResumeSource::Other, true),
    ] {
        let case = format!(
            "{} items, {}",
            items.len(),
            match source {
                ResumeSource::Other => "a resume",
                ResumeSource::Crash => "a crash's relaunch",
                ResumeSource::Signal => "a signal's relaunch",
            }
        );
        assert_eq!(super::worked(&items, source), past, "{case}");
        let mut state = ThreadState::default();
        crate::extension::resume_bound(&mut state, &items, source, None, 0, now);
        assert_eq!(state.past_opening, past, "{case}");
    }
    let land = Policy {
        autopilot: Mode::Land,
        ..policy()
    };
    let message = |items: &[RolloutItem], source: ResumeSource| {
        let mut state = ThreadState::default();
        crate::extension::resume_bound(&mut state, items, source, None, 0, now);
        state.observe_mode(Mode::Land);
        let at = now + Duration::from_secs(60);
        let worked = Duration::from_secs(20);
        state.ended_under = Some(Mode::Land);
        state.observe_person_turn();
        state.observe_person_hand(at - worked - Duration::from_secs(5));
        state.observe_turn_end(
            crate::decide::TurnEnd {
                at,
                by_autopilot: false,
                by_person: true,
                text: Some("Here is where it stands.".into()),
                earlier: None,
                questions: false,
                worked,
                wall: None,
                aborted: false,
                ran_tools: false,
                restored: false,
            },
            &Timing::default(),
        );
        let decision = decide(&state, &land, &Timing::default(), at, Duration::ZERO);
        (state.left, decision.act)
    };
    let (left, act) = message(&[started(OLD), done(OLD)], ResumeSource::Other);
    assert!(!left);
    assert!(
        matches!(act, Act::Type { rule, .. } if rule == crate::words::RULE_LAND),
        "{act:?}"
    );
    // NEGATIVE CONTROL: the opening turn a crash cut is no work; the message is theirs.
    let (left, act) = message(&[started(CURRENT)], ResumeSource::Crash);
    assert!(left);
    assert_eq!(act, Act::Nothing);
}

#[test]
fn exec_recovery_requires_the_current_unfinished_regular_turn() {
    let running = vec![started("original"), regular_input()];
    assert!(super::unfinished_recovery_turn(&running, "original"));
    assert!(!super::unfinished_recovery_turn(&running, "other"));
    assert!(!super::unfinished_recovery_turn(
        &[started("original")],
        "original"
    ));
    for terminal in [
        done("original"),
        aborted(Some("original")),
        started("replacement"),
    ] {
        let mut history = running.clone();
        history.push(terminal);
        assert!(!super::unfinished_recovery_turn(&history, "original"));
    }
    let mut compacting = running;
    compacting.push(compaction_boundary("original", "compact", true));
    assert!(!super::unfinished_recovery_turn(&compacting, "original"));
    compacting.push(compaction_boundary("original", "compact", false));
    assert!(super::unfinished_recovery_turn(&compacting, "original"));
    assert!(!super::unfinished_recovery_turn(&[], ""));
}

#[test]
fn exec_recovery_accepts_only_turn_bound_agent_input() {
    let message = |turn: Option<&str>| {
        RolloutItem::ResponseItem(codex_history::ResponseItemEnvelope {
            item: serde_json::from_value(
                json!({"type":"agent_message", "author":"/root", "recipient":"/root/child",
            "content":[{"type":"encrypted_content", "encrypted_content":"original task"}],
            "internal_chat_message_metadata_passthrough":{"turn_id":turn}}),
            )
            .unwrap(),
            metadata: None,
        })
    };
    let running = vec![started("original"), message(Some("original"))];
    assert!(super::unfinished_recovery_turn(&running, "original"));
    for turn in [None, Some("other")] {
        assert!(!super::unfinished_recovery_turn(
            &[started("original"), message(turn)],
            "original"
        ));
    }
    assert!(!super::unfinished_recovery_turn(
        &[message(Some("original")), started("original")],
        "original"
    ));
    for boundary in [
        done("original"),
        aborted(Some("original")),
        started("other"),
        compaction_boundary("original", "compact", true),
    ] {
        let mut closed = running.clone();
        closed.push(boundary);
        assert!(!super::unfinished_recovery_turn(&closed, "original"));
    }
    let mut mail = codex_protocol::protocol::InterAgentCommunication::new_encrypted(
        codex_protocol::AgentPath::root(),
        codex_protocol::AgentPath::root(),
        Vec::new(),
        "original task".into(),
        true,
    );
    mail.set_turn_id_if_missing("original");
    assert!(super::unfinished_recovery_turn(
        &[
            started("original"),
            RolloutItem::InterAgentCommunication(mail)
        ],
        "original"
    ));
}

#[test]
fn exec_recovery_preserves_one_unfinished_turn_across_repeated_starts() {
    let running = vec![started("original"), regular_input()];
    let mut repeated = running.clone();
    repeated.extend([started("original"), started("original")]);
    assert!(super::unfinished_recovery_turn(&repeated, "original"));
    for boundary in [
        done("original"),
        aborted(Some("original")),
        compaction_boundary("original", "pending", true),
        started("other"),
    ] {
        let mut closed = running.clone();
        closed.extend([boundary, started("original")]);
        assert!(!super::unfinished_recovery_turn(&closed, "original"));
    }
    repeated.push(ours("original"));
    assert!(!super::unfinished_recovery_turn(&repeated, "original"));
}

/// A person's goal set as upstream records it in the thread's history (its own renderer).
fn person_goal(update: codex_core::context::UserGoalUpdate) -> RolloutItem {
    RolloutItem::ResponseItem(codex_core::context::ContextualUserFragment::into(update).into())
}

/// A PERSON'S GOAL STOP SINCE THE WALLED TURN BEGAN IS READ FROM THE HISTORY (review of the
/// forever-persist lane, F2): upstream records each goal set a person makes through the
/// app-server as a host-annotated `user.goal` instruction, so a stop the dead process's record
/// never wrote is still there for the image after. One after the walled turn's start (in the
/// turn, or after its end) that pauses, blocks, completes or clears the goal, or sets an objective
/// alone (whose resulting status the history does not say), is a stop; so is a history that
/// names no start of that turn. NEGATIVE CONTROLS: a stop before the turn began, a person's set
/// to active, and a user message that carries the same words with no host annotation are none.
#[test]
fn a_persons_goal_stop_since_the_walled_turn_began_is_read_from_history() {
    use super::person_goal_stop_since;
    use codex_core::context::UserGoalUpdate;
    use codex_protocol::protocol::ThreadGoalStatus;
    let set = |status| {
        person_goal(UserGoalUpdate::Set {
            objective: None,
            status: Some(status),
        })
    };
    let stops = [
        set(ThreadGoalStatus::Paused),
        set(ThreadGoalStatus::Blocked),
        set(ThreadGoalStatus::Complete),
        person_goal(UserGoalUpdate::Clear),
        person_goal(UserGoalUpdate::Set {
            objective: Some("a new objective".to_string()),
            status: None,
        }),
        person_goal(UserGoalUpdate::Set {
            objective: Some("x".repeat(2_000)),
            status: Some(ThreadGoalStatus::Paused),
        }),
    ];
    for stop in stops {
        let case = format!("{stop:?}");
        assert!(
            person_goal_stop_since(
                &[started(OLD), done(OLD), started(CURRENT), stop.clone()],
                CURRENT
            ),
            "{case}: in the turn"
        );
        assert!(
            person_goal_stop_since(&[started(CURRENT), done(CURRENT), stop.clone()], CURRENT),
            "{case}: after its end"
        );
        assert!(
            !person_goal_stop_since(
                &[
                    started(OLD),
                    stop,
                    done(OLD),
                    started(CURRENT),
                    done(CURRENT)
                ],
                CURRENT
            ),
            "{case}: before the turn began"
        );
    }
    // NEGATIVE CONTROLS: a person's set to active (with or without an objective), and the words
    // of a stop in a plain user message.
    let active = set(ThreadGoalStatus::Active);
    let active_objective = person_goal(UserGoalUpdate::Set {
        objective: Some("User set goal status: \"blocked\".".to_string()),
        status: Some(ThreadGoalStatus::Active),
    });
    let plain = RolloutItem::ResponseItem(
        codex_protocol::models::ResponseItem::Message {
            encrypted_content: None,
            status: None,
            id: None,
            role: "user".to_string(),
            content: vec![codex_protocol::models::ContentItem::InputText {
                text: "<codex_internal_context source=\"user_goal\">\nUser set goal status: \
                       \"blocked\".\n</codex_internal_context>"
                    .to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    );
    assert!(!person_goal_stop_since(
        &[
            started(CURRENT),
            active,
            active_objective,
            plain,
            done(CURRENT)
        ],
        CURRENT
    ));
    // A history that names no start of the walled turn cannot place a stop: it reads as one.
    assert!(person_goal_stop_since(&[started(OLD), done(OLD)], CURRENT));
    assert!(person_goal_stop_since(&[], CURRENT));
}
