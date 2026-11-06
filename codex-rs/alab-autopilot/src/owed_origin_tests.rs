// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A failed record write or an older image can leave an at-rest act behind a newer native turn.

use crate::decide::Act;
use crate::decide::Mode;
use crate::decide::Owed;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::decide::Texts;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::decide;
use codex_alab_core::session::InFlightRetry;
use codex_alab_core::session::NativeOrigin;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_history::RolloutItem;
use codex_protocol::protocol::EventMsg;
use serde_json::json;
use std::time::Duration;
use std::time::Instant;

fn policy(mode: Mode) -> Policy {
    Policy {
        continue_on: true,
        continue_text: Some("continue the current task".into()),
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
        autopilot: mode,
        texts: Texts::default(),
    }
}

fn event(value: serde_json::Value) -> RolloutItem {
    RolloutItem::EventMsg(serde_json::from_value::<EventMsg>(value).expect("native event"))
}

fn started(turn: &str) -> RolloutItem {
    event(
        json!({"type":"task_started", "turn_id":turn, "turn_attribution": {"turn_id": turn, "turn_trigger": "user"}, "model_context_window":258400,
        "collaboration_mode_kind":"default"}),
    )
}

fn wake_started(turn: &str) -> RolloutItem {
    event(
        json!({"type":"task_started", "turn_id":turn, "model_context_window":258400,
        "collaboration_mode_kind":"default", "turn_attribution":{"turn_id":turn, "turn_trigger":"goal"}}),
    )
}

fn complete(turn: &str) -> RolloutItem {
    event(json!({"type":"task_complete", "turn_id":turn,
        "last_agent_message":"the old task finished"}))
}

fn owed(now: Instant) -> Owed {
    Owed {
        native_origin: None,
        rule: crate::words::RULE_CONTINUE,
        text: "obsolete continuation of the old task".into(),
        mode: Mode::Cruise,
        not_before: now,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    }
}

fn bound(now: Instant, turn: &str, phase: NativePhase, host_turn: bool) -> Owed {
    Owed {
        native_origin: Some(NativeOrigin {
            turn_id: turn.into(),
            phase,
        }),
        rule: if host_turn {
            crate::words::RULE_HOST_TURN
        } else {
            crate::words::RULE_CONTINUE
        },
        ..owed(now)
    }
}

fn abort(turn: Option<&str>, reason: &str) -> RolloutItem {
    event(json!({"type":"turn_aborted", "turn_id":turn, "reason":reason}))
}

fn raw(owed: &Owed, landed: bool) -> OwedAct {
    OwedAct {
        thread: "thread".into(),
        native_origin: owed.native_origin.clone(),
        rule: (!landed).then(|| owed.rule.into()),
        text: (!landed).then(|| owed.text.clone()),
        mode: (!landed).then(|| owed.mode.word().into()),
        landed,
        ..Default::default()
    }
}

#[test]
fn only_validated_live_carriable_intent_receives_an_awake_guard() {
    let now = Instant::now();
    let items = [started("current"), complete("current")];
    let history = crate::resume_history::latest(&items);
    let valid = bound(now, "current", NativePhase::Complete, false);
    for landed in [false, true] {
        let handoff = crate::owed_origin::record(&raw(&valid, landed), &history, None)
            .expect("the exact raw native origin validates the act or landing");
        assert!(handoff.carries_automatic_intent(), "landed={landed}");
    }
    assert!(!crate::owed_origin::BoundHandoff::empty().carries_automatic_intent());
    let invalid = bound(now, "obsolete", NativePhase::Complete, false);
    assert!(crate::owed_origin::record(&raw(&invalid, false), &history, None).is_none());
    let unbound = owed(now);
    assert!(crate::owed_origin::record(&raw(&unbound, true), &history, None).is_none());
    let wall = Owed {
        rule: crate::words::RULE_API_RETRY,
        ..valid
    };
    let wall = crate::owed_origin::record(&raw(&wall, false), &history, None).unwrap();
    assert!(wall.wall_try());
    assert!(!wall.carries_automatic_intent());
}

#[test]
fn a_newer_native_attempt_revokes_a_legacy_owed_act() {
    let now = Instant::now();
    let items = [started("old"), complete("old"), started("corrected")];
    let mut state = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut state,
        &items,
        false,
        Some((Some(owed(now)), false)),
        0,
        now,
    ));
    assert_eq!(
        decide(
            &state,
            &policy(Mode::Cruise),
            &Timing::default(),
            now,
            Duration::ZERO
        )
        .act,
        Act::Nothing,
        "the failed record write cannot replay the old act over the accepted correction"
    );
}

#[test]
fn a_newer_native_attempt_revokes_a_legacy_landing() {
    let now = Instant::now();
    let items = [started("old"), complete("old"), started("corrected")];
    let mut state = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut state,
        &items,
        false,
        Some((None, true)),
        0,
        now,
    ));
    assert!(!state.landed, "the accepted correction reopened the task");
}

#[test]
fn a_completed_origin_accepts_only_the_same_completed_attempt() {
    let now = Instant::now();
    let intended = bound(now, "old", NativePhase::Complete, false);
    for (items, valid) in [
        (vec![started("old"), complete("old")], true),
        (vec![started("old"), complete("old"), started("new")], false),
        (
            vec![
                started("old"),
                complete("old"),
                started("new"),
                complete("new"),
            ],
            false,
        ),
        (
            vec![started("old"), abort(Some("old"), "interrupted")],
            false,
        ),
        (
            vec![
                started("old"),
                complete("old"),
                event(json!({"type":"user_message", "message":"correct the task", "images":[]})),
            ],
            false,
        ),
    ] {
        for landed in [false, true] {
            let carried = crate::owed_origin::record(
                &raw(&intended, landed),
                &crate::resume_history::latest(&items),
                None,
            );
            assert_eq!(carried.is_some(), valid, "landed={landed}");
            let mut state = ThreadState::default();
            crate::extension::resume_bound(
                &mut state,
                &items,
                false,
                Some(carried.unwrap_or_else(crate::owed_origin::BoundHandoff::empty)),
                0,
                now,
            );
            assert_eq!(state.landed, valid && landed);
            assert_eq!(state.owed.is_some(), valid && !landed);
        }
    }
    // A missing origin cannot distinguish the intended completion from a newer one.
    for items in [
        vec![started("old"), complete("old")],
        vec![
            started("old"),
            complete("old"),
            started("corrected"),
            complete("corrected"),
        ],
    ] {
        for landed in [false, true] {
            assert!(
                crate::owed_origin::record(
                    &raw(&owed(now), landed),
                    &crate::resume_history::latest(&items),
                    None
                )
                .is_none()
            );
            let mut state = ThreadState::default();
            crate::extension::resume_into(
                &mut state,
                &items,
                true,
                Some(((!landed).then(|| owed(now)), landed)),
                0,
                now,
            );
            assert!(!state.landed);
            assert!(state.owed.is_none());
            assert!(!state.carry_on);
        }
    }
}

#[test]
fn an_open_attempt_preserves_only_its_bound_paced_host_turn() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(1200);
    let items = [started("running")];
    let intended = bound(deadline, "running", NativePhase::Running, true);
    let mut state = ThreadState::default();
    for _ in 0..2 {
        crate::extension::resume_into(
            &mut state,
            &items,
            true,
            Some((Some(intended.clone()), false)),
            0,
            now,
        );
        let decision = decide(
            &state,
            &policy(Mode::Cruise),
            &Timing::default(),
            now,
            Duration::ZERO,
        );
        assert!(matches!(
            decision.act,
            Act::Type {
                rule: crate::words::RULE_HOST_TURN,
                ..
            }
        ));
        assert_eq!(decision.not_before, Some(deadline));
        assert_eq!(
            state
                .owed
                .as_ref()
                .and_then(|owed| owed.native_origin.as_ref()),
            intended.native_origin.as_ref()
        );
    }
    for items in [
        vec![started("running"), started("new")],
        vec![started("running"), abort(Some("running"), "interrupted")],
        vec![started("running"), complete("running")],
    ] {
        let mut state = ThreadState::default();
        crate::extension::resume_into(
            &mut state,
            &items,
            true,
            Some((Some(intended.clone()), false)),
            0,
            now,
        );
        assert!(state.owed.is_none());
        assert!(
            !state.carry_on,
            "rejecting the owed action does not create crash carry-on authority"
        );
    }
}

#[test]
fn an_already_observed_interrupt_keeps_its_host_turn_across_an_early_second_crash() {
    let now = Instant::now();
    let intended = bound(
        now + Duration::from_secs(1200),
        "running",
        NativePhase::Interrupted,
        true,
    );
    let items = [started("running"), abort(Some("running"), "interrupted")];
    for source in [
        crate::resume_history::ResumeSource::Other,
        crate::resume_history::ResumeSource::Crash,
    ] {
        let mut state = ThreadState::default();
        crate::extension::resume_into(
            &mut state,
            &items,
            source,
            Some((Some(intended.clone()), false)),
            0,
            now,
        );
        assert_eq!(state.owed.as_ref(), Some(&intended));
        assert!(matches!(
            decide(
                &state,
                &policy(Mode::Cruise),
                &Timing::default(),
                now,
                Duration::ZERO
            )
            .act,
            Act::Type {
                rule: crate::words::RULE_HOST_TURN,
                ..
            }
        ));
    }
    for items in [
        vec![
            started("running"),
            abort(Some("running"), "interrupted"),
            started("corrected"),
        ],
        vec![started("running"), abort(Some("other"), "interrupted")],
        vec![started("running"), abort(None, "interrupted")],
        vec![started("running"), abort(Some("running"), "replaced")],
        vec![
            started("running"),
            abort(None, "interrupted"),
            abort(Some("running"), "interrupted"),
        ],
        vec![
            started("running"),
            abort(Some("running"), "interrupted"),
            abort(None, "interrupted"),
            abort(Some("running"), "interrupted"),
        ],
        vec![
            started("running"),
            abort(Some("running"), "interrupted"),
            event(json!({"type":"user_message", "message":"corrected", "images":[]})),
        ],
    ] {
        let mut state = ThreadState::default();
        crate::extension::resume_into(
            &mut state,
            &items,
            true,
            Some((Some(intended.clone()), false)),
            0,
            now,
        );
        assert!(state.owed.is_none());
        assert!(!state.carry_on);
    }
}

#[test]
fn legacy_open_host_turn_binding_needs_an_exact_existing_in_flight_fact() {
    let now = Instant::now();
    let intended = Owed {
        rule: crate::words::RULE_HOST_TURN,
        ..owed(now)
    };
    let items = [started("running")];
    let history = crate::resume_history::latest(&items);
    for (thread, turn, valid) in [
        ("thread", "running", true),
        ("other", "running", false),
        ("thread", "old", false),
    ] {
        let retry = InFlightRetry {
            thread: thread.into(),
            turn_id: turn.into(),
            not_before_ms: 99,
        };
        let bound = crate::owed_origin::record(&raw(&intended, false), &history, Some(&retry));
        assert_eq!(bound.is_some(), valid);
        if let Some(bound) = bound {
            let mut state = ThreadState::default();
            crate::extension::resume_bound(&mut state, &items, false, Some(bound), 0, now);
            assert_eq!(
                state
                    .owed
                    .as_ref()
                    .and_then(|owed| owed.native_origin.as_ref())
                    .map(|origin| origin.phase),
                Some(NativePhase::Running)
            );
        }
    }
    assert!(crate::owed_origin::record(&raw(&intended, false), &history, None).is_none());
    assert!(
        crate::owed_origin::record(
            &raw(&owed(now), false),
            &history,
            Some(&InFlightRetry {
                thread: "thread".into(),
                turn_id: "running".into(),
                not_before_ms: 99
            })
        )
        .is_none()
    );
}

#[test]
fn a_judged_landing_keeps_the_exact_completed_wake_that_met_a_wall() {
    let now = Instant::now();
    let origin = bound(now, "wake", NativePhase::Complete, false);
    let items = [
        wake_started("wake"),
        event(json!({
            "type":"task_complete", "turn_id":"wake", "last_agent_message":null,
            "error":{"message":"the provider ended the wake", "codex_error_info":codex_protocol::protocol::CodexErrorInfo::UsageLimitExceeded}
        })),
    ];
    let history = crate::resume_history::latest(&items);
    let landing = crate::owed_origin::record(&raw(&origin, true), &history, None)
        .expect("a native completion fact does not discard a judged landing at a wall");
    let mut state = ThreadState::default();
    crate::extension::resume_bound(&mut state, &items, false, Some(landing), 0, now);
    let policy = policy(Mode::Land);
    state.observe_mode(Mode::Land);
    assert!(state.landed);
    assert_eq!(
        decide(
            &state,
            &policy,
            &Timing::default(),
            now + Duration::from_secs(86400),
            Duration::ZERO
        )
        .act,
        Act::Nothing
    );
    assert!(
        crate::owed_origin::record(&raw(&origin, false), &history, None).is_none(),
        "the same failed completion authorizes no ordinary continuation"
    );
    let host = bound(now, "wake", NativePhase::Complete, true);
    assert!(
        crate::owed_origin::record(&raw(&host, false), &history, None).is_none(),
        "the same failed completion authorizes no crash continuation"
    );
    let newer = [
        items[0].clone(),
        items[1].clone(),
        started("correction"),
        complete("correction"),
    ];
    assert!(
        crate::owed_origin::record(
            &raw(&origin, true),
            &crate::resume_history::latest(&newer),
            None
        )
        .is_none(),
        "a completed correction still revokes the old landing"
    );
}

#[test]
fn a_judged_landing_keeps_only_its_exact_aborted_own_wake() {
    let now = Instant::now();
    for (reason, phase) in [
        ("interrupted", NativePhase::Interrupted),
        ("replaced", NativePhase::Aborted),
    ] {
        let intended = bound(now, "wake", phase, false);
        let items = [wake_started("wake"), abort(Some("wake"), reason)];
        let landing = crate::owed_origin::record(
            &raw(&intended, true),
            &crate::resume_history::latest(&items),
            None,
        )
        .expect("the already-judged own wake can remain landed");
        let mut state = ThreadState::default();
        crate::extension::resume_bound(&mut state, &items, true, Some(landing), 0, now);
        state.observe_mode(Mode::Land);
        assert!(state.landed);
        assert_eq!(
            decide(
                &state,
                &policy(Mode::Land),
                &Timing::default(),
                now,
                Duration::ZERO
            )
            .act,
            Act::Probe
        );
        state.probed(None);
        // The kept landing is recorded again for aterm, which reopened it at the wake's turn
        // (review of gap 12, finding 1), and nothing after it.
        assert_eq!(
            decide(
                &state,
                &policy(Mode::Land),
                &Timing::default(),
                now,
                Duration::ZERO
            )
            .act,
            Act::Landed
        );
        state.acted(&Act::Landed, now, &Timing::default());
        assert_eq!(
            decide(
                &state,
                &policy(Mode::Land),
                &Timing::default(),
                now,
                Duration::ZERO
            )
            .act,
            Act::Nothing
        );
        assert!(
            crate::owed_origin::record(
                &raw(&intended, false),
                &crate::resume_history::latest(&items),
                None
            )
            .is_none()
        );
        let newer = [
            items[0].clone(),
            items[1].clone(),
            started("correction"),
            complete("correction"),
        ];
        assert!(
            crate::owed_origin::record(
                &raw(&intended, true),
                &crate::resume_history::latest(&newer),
                None
            )
            .is_none()
        );
        let person_items = [started("wake"), abort(Some("wake"), reason)];
        let landing = crate::owed_origin::record(
            &raw(&intended, true),
            &crate::resume_history::latest(&person_items),
            None,
        )
        .expect("matching raw point");
        let mut state = ThreadState::default();
        crate::extension::resume_bound(&mut state, &person_items, true, Some(landing), 0, now);
        assert!(
            !state.landed,
            "a person's own aborted turn reopens the task"
        );
    }
}

/// A WALL'S TRY BINDS AT THE FAILED TURN IT WAS DECIDED AT (B2): the record carries the try with
/// the origin its decision observed, and where no recovery restores the point (a measured
/// return a check held back carries the blind retry with none; a record from an image before
/// recovery), the relaunch types it at its time, the words as decided, even where the history's
/// bare completion would read a provisional fatal (core's `Other` for a reply cut off). Before,
/// every act at a failed completion was refused, so the carried try did nothing and the wall
/// waited for a person's turn. NEGATIVE CONTROLS: a newer turn since, a try decided at another
/// turn, a legacy try that names no origin, and a continuation at the same failure bind not.
#[test]
fn a_walls_try_binds_at_the_failed_turn_it_was_decided_at() {
    use codex_protocol::protocol::CodexErrorInfo;
    let now = Instant::now();
    let failed = |turn: &str, info: CodexErrorInfo| {
        event(json!({
            "type":"task_complete", "turn_id":turn, "last_agent_message":null,
            "error":{"message":"the API failed", "codex_error_info":info}
        }))
    };
    let try_at = |turn: Option<&str>| Owed {
        native_origin: turn.map(|turn| NativeOrigin {
            turn_id: turn.into(),
            phase: NativePhase::Complete,
        }),
        rule: crate::words::RULE_API_RETRY,
        text: "Codex reported an API error; trying again.".into(),
        ..owed(now)
    };
    for info in [
        CodexErrorInfo::Other,
        CodexErrorInfo::HttpConnectionFailed {
            http_status_code: None,
        },
        CodexErrorInfo::ServerOverloaded,
    ] {
        let items = [started("walled"), failed("walled", info.clone())];
        let history = crate::resume_history::latest(&items);
        let mut act = raw(&try_at(Some("walled")), false);
        act.not_before_ms = codex_alab_core::session::now_ms() + 60_000;
        let carried = crate::owed_origin::record(&act, &history, None)
            .unwrap_or_else(|| panic!("{info:?}: the try binds at its own failure"));
        let mut state = ThreadState::default();
        crate::extension::resume_bound(&mut state, &items, false, Some(carried), 0, now);
        let p = policy(Mode::Cruise);
        let t = Timing::default();
        let d = decide(&state, &p, &t, now, Duration::ZERO);
        assert!(
            d.not_before
                .is_some_and(|at| at > now + Duration::from_secs(58)),
            "{info:?}: at its time: {d:?}"
        );
        assert_eq!(
            decide(
                &state,
                &p,
                &t,
                now + Duration::from_secs(62),
                Duration::ZERO
            )
            .act,
            Act::Type {
                rule: crate::words::RULE_API_RETRY,
                text: "Codex reported an API error; trying again.".into(),
            },
            "{info:?}: typed as decided, at its time"
        );
        // NEGATIVE CONTROLS.
        for (what, carried, items) in [
            (
                "a newer turn since",
                try_at(Some("walled")),
                vec![
                    started("walled"),
                    failed("walled", info.clone()),
                    started("newer"),
                    complete("newer"),
                ],
            ),
            ("another turn's try", try_at(Some("older")), items.to_vec()),
            ("a legacy try", try_at(None), items.to_vec()),
            (
                "a continuation",
                bound(now, "walled", NativePhase::Complete, false),
                items.to_vec(),
            ),
        ] {
            assert!(
                crate::owed_origin::record(
                    &raw(&carried, false),
                    &crate::resume_history::latest(&items),
                    None
                )
                .is_none(),
                "{info:?}: {what}"
            );
        }
    }
}

#[test]
fn a_pending_policy_continuation_keeps_its_paced_intent_at_the_failed_turn() {
    let now = Instant::now();
    let items = [
        started("refused"),
        event(json!({
            "type":"user_message", "message":"the original inert request",
            "images":null, "local_images":[], "text_elements":[]
        })),
        event(json!({
            "type":"task_complete", "turn_id":"refused", "last_agent_message":null,
            "error":{"message":"the policy refused the action", "codex_error_info":"cyber_policy"}
        })),
    ];
    let history = crate::resume_history::latest(&items);
    let p = policy(Mode::Cruise);
    let t = Timing::default();
    let mut before = ThreadState::default();
    crate::extension::resume_into(&mut before, &items, false, None, 0, now);
    let decision = decide(&before, &p, &t, now, Duration::ZERO);
    assert_eq!(
        decision.act,
        Act::Type {
            rule: crate::words::RULE_POLICY_CONTINUE,
            text: crate::words::POLICY_CONTINUE.into(),
        },
        "the native refusal authorizes only the bounded independent-work instruction"
    );
    // An existing readiness hold can defer this already-decided act before a live update.
    let intended = crate::extension::owes_of(
        &decision,
        Mode::Cruise,
        now + Duration::from_secs(1200),
        false,
        before.native_origin.clone(),
    )
    .expect("the policy continuation is a paced act");
    let mut snapshot = raw(&intended, false);
    snapshot.not_before_ms = codex_alab_core::session::now_ms() + 1_200_000;
    let mut ordinary = snapshot.clone();
    ordinary.rule = Some(crate::words::RULE_CONTINUE.into());
    ordinary.text = Some("continue the original task".into());
    assert!(crate::owed_origin::record(&ordinary, &history, None).is_none());
    let mut wrong = snapshot.clone();
    wrong.native_origin = Some(NativeOrigin {
        turn_id: "another-failed-turn".into(),
        phase: NativePhase::Complete,
    });
    assert!(crate::owed_origin::record(&wrong, &history, None).is_none());

    let carried = crate::owed_origin::record(&snapshot, &history, None)
        .expect("the policy continuation must bind to its own native refusal");
    let (expected, landed) = carried.clone().into_parts();
    assert!(!landed);
    let expected = expected.expect("the record retains its decided act");
    assert_eq!(expected.rule, intended.rule);
    assert_eq!(expected.text, intended.text);
    assert!(expected.not_before > now);
    let mut resumed = ThreadState::default();
    crate::extension::resume_bound(&mut resumed, &items, false, Some(carried), 0, now);
    let actual = decide(&resumed, &p, &t, now, Duration::ZERO);
    assert_eq!(
        actual.act,
        Act::Type {
            rule: expected.rule,
            text: expected.text,
        }
    );
    assert_eq!(actual.not_before, Some(expected.not_before));
}

#[test]
fn a_carried_policy_instruction_requires_fixed_words_and_a_regular_policy_refusal() {
    let regular = || {
        event(json!({
            "type":"user_message", "message":"the original inert request",
            "images":null, "local_images":[], "text_elements":[]
        }))
    };
    let failed = |code: Option<&str>| {
        event(json!({
            "type":"task_complete", "turn_id":"refused", "last_agent_message":null,
            "error":{"message":"the native refusal", "codex_error_info":code}
        }))
    };
    let items = [started("refused"), regular(), failed(Some("cyber_policy"))];
    let snapshot = raw(
        &Owed {
            rule: crate::words::RULE_POLICY_CONTINUE,
            text: crate::words::POLICY_CONTINUE.into(),
            ..bound(Instant::now(), "refused", NativePhase::Complete, false)
        },
        false,
    );
    let history = crate::resume_history::latest(&items);
    for text in [
        crate::words::POLICY_CONTINUE.to_string(),
        format!(
            "{} {}",
            crate::words::POLICY_CONTINUE,
            crate::words::POLICY_BOUNDARY
        ),
    ] {
        let mut exact = snapshot.clone();
        exact.text = Some(text.clone());
        let accepted = crate::owed_origin::record(&exact, &history, None)
            .expect("the fixed instruction, with its known exclusion suffix, is valid");
        assert_eq!(
            accepted
                .into_parts()
                .0
                .expect("the carried instruction")
                .text,
            text
        );
    }
    for text in [
        "continue the original task".to_string(),
        format!("{} retry the original task", crate::words::POLICY_CONTINUE),
        format!(
            "{} {} extra",
            crate::words::POLICY_CONTINUE,
            crate::words::POLICY_BOUNDARY
        ),
    ] {
        let mut altered = snapshot.clone();
        altered.text = Some(text);
        assert!(crate::owed_origin::record(&altered, &history, None).is_none());
    }
    for code in [
        Some("bio_policy"),
        Some("server_overloaded"),
        Some("other"),
        None,
    ] {
        let other = [started("refused"), regular(), failed(code)];
        assert!(
            crate::owed_origin::record(&snapshot, &crate::resume_history::latest(&other), None)
                .is_none(),
            "the fixed instruction cannot authorize another error: {code:?}"
        );
    }
    let compacting = event(json!({
        "type":"item_started", "thread_id":"00000000-0000-4000-8000-000000000001",
        "turn_id":"refused", "item":{"type":"ContextCompaction", "id":"compacting"},
        "started_at_ms":1
    }));
    for unproven in [
        vec![started("refused"), failed(Some("cyber_policy"))],
        vec![
            started("refused"),
            regular(),
            compacting,
            failed(Some("cyber_policy")),
        ],
        vec![started("refused"), regular(), complete("refused")],
    ] {
        assert!(
            crate::owed_origin::record(&snapshot, &crate::resume_history::latest(&unproven), None)
                .is_none(),
            "a policy instruction requires proven regular input and its uncompacted policy refusal"
        );
    }
}

#[test]
fn changing_mode_after_policy_handoff_keeps_the_refused_action_excluded() {
    let now = Instant::now();
    let items = [
        started("refused"),
        event(json!({
            "type":"user_message", "message":"the original inert request",
            "images":null, "local_images":[], "text_elements":[]
        })),
        event(json!({
            "type":"task_complete", "turn_id":"refused", "last_agent_message":null,
            "error":{"message":"the policy refused the action", "codex_error_info":"cyber_policy"}
        })),
    ];
    let intended = Owed {
        rule: crate::words::RULE_POLICY_CONTINUE,
        text: crate::words::POLICY_CONTINUE.into(),
        ..bound(
            now + Duration::from_secs(1200),
            "refused",
            NativePhase::Complete,
            false,
        )
    };
    let mut snapshot = raw(&intended, false);
    snapshot.not_before_ms = codex_alab_core::session::now_ms() + 1_200_000;
    let carried =
        crate::owed_origin::record(&snapshot, &crate::resume_history::latest(&items), None)
            .expect("the fixed policy continuation binds to its native refusal");
    let mut resumed = ThreadState::default();
    crate::extension::resume_bound(&mut resumed, &items, false, Some(carried), 0, now);
    assert!(resumed.owed.is_some());
    assert!(!resumed.policy_continued, "an unsubmitted act is not spent");
    let timing = Timing::default();
    let before = decide(
        &resumed,
        &policy(Mode::Cruise),
        &timing,
        now,
        Duration::ZERO,
    );
    assert_eq!(
        before.act,
        Act::Type {
            rule: crate::words::RULE_POLICY_CONTINUE,
            text: crate::words::POLICY_CONTINUE.into(),
        }
    );
    for mode in [Mode::Land, Mode::Cruise] {
        resumed.observe_mode(mode);
        // Supply only a clean repository finding; no filesystem or real work is involved.
        resumed.probed(None);
        let decision = decide(&resumed, &policy(mode), &timing, now, Duration::ZERO);
        let excluded = match &decision.act {
            Act::Type { rule, text } => {
                *rule == crate::words::RULE_POLICY_CONTINUE && text == crate::words::POLICY_CONTINUE
            }
            Act::Nothing | Act::Attention { .. } => true,
            _ => false,
        };
        assert!(
            excluded,
            "changing to {mode:?} cannot turn the policy refusal into ordinary work: {decision:?}"
        );
        assert!(
            !resumed.policy_continued,
            "the decision has not been submitted"
        );
    }
}
