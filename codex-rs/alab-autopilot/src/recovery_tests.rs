// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recovery re-decides one native failure under current policy and its original clocks.

use crate::decide::Act;
use crate::decide::Mode;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::decide::Texts;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::decide::decide;
use crate::recovery;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::ResponseIncompleteError;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use serde_json::json;
use std::time::Duration;
use std::time::Instant;

const EPOCH: u64 = 1_800_000_000_000;
const MINUTE: u64 = 60_000;
const FAILED: &str = "the-native-failed-turn";

fn policy() -> Policy {
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
        autopilot: Mode::Cruise,
        texts: Texts::default(),
    }
}

fn state() -> ThreadState {
    let mut state = ThreadState::default();
    state.observe_resume(true, None, false);
    state.observe_mode(Mode::Cruise);
    state
}

fn at_wall(wall: Wall, now: Instant, timing: &Timing) -> ThreadState {
    let mut state = state();
    state.observe_turn_end(
        TurnEnd {
            at: now,
            by_autopilot: true,
            by_person: false,
            text: None,
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall: Some(wall),
            aborted: false,
            ran_tools: false,
            restored: false,
        },
        timing,
    );
    state
}

fn event(value: serde_json::Value) -> RolloutItem {
    RolloutItem::EventMsg(serde_json::from_value::<EventMsg>(value).expect("native rollout event"))
}

fn complete(turn: &str, error: Option<CodexErrorInfo>) -> RolloutItem {
    event(json!({
        "type": "task_complete",
        "turn_id": turn,
        "last_agent_message": null,
        "error": error.map(|info| json!({
            "message": "the provider ended this turn",
            "codex_error_info": info,
        })),
    }))
}

#[test]
fn a_crash_record_still_marked_busy_cannot_generically_repeat_a_policy_refusal() {
    let now = Instant::now();
    let items = [
        event(json!({
            "type": "task_started", "turn_id": FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window": 258400, "collaboration_mode_kind": "default",
        })),
        event(
            json!({"type":"user_message", "message":"Review the authorized task.",
            "images":null, "local_images":[], "text_elements":[]}),
        ),
        complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now,
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(
        matches!(decision.act, Act::Type { rule: crate::words::RULE_POLICY_CONTINUE, ref text } if text == crate::words::POLICY_CONTINUE),
        "the native policy refusal must survive a lagging busy record: {decision:?}"
    );
}

#[test]
fn a_resumed_abort_is_not_a_busy_crash_carry_on() {
    let now = Instant::now();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"})),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert_eq!(decision.act, Act::Nothing, "{decision:?}");
}

#[test]
fn a_resumed_native_abort_requires_the_exact_signal_teardown_exception() {
    let now = Instant::now();
    let start = event(
        json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
    );
    for (turn, reason, allowed) in [
        (Some(FAILED), "interrupted", true),
        (None, "interrupted", false),
        (Some("another-turn"), "interrupted", false),
        (Some(FAILED), "replaced", false),
        (Some(FAILED), "review_ended", false),
        (Some(FAILED), "budget_limited", false),
    ] {
        let items = [
            start.clone(),
            event(json!({"type":"turn_aborted", "turn_id":turn, "reason":reason})),
        ];
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &items,
            crate::resume_history::ResumeSource::Signal,
            None,
            0,
            now
        ));
        let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
        assert_eq!(
            matches!(
                decision.act,
                Act::Type {
                    rule: crate::words::RULE_HOST_TURN,
                    ..
                }
            ),
            allowed,
            "{turn:?} {reason}: {decision:?}"
        );
        if !allowed {
            assert_eq!(decision.act, Act::Nothing);
        }
    }
    let abort = event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"}));
    let correction = event(
        json!({"type":"user_message", "message":"a corrected request", "images":null,
        "local_images":[], "text_elements":[]}),
    );
    let items = [start, abort.clone(), correction, abort];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        crate::resume_history::ResumeSource::Signal,
        None,
        0,
        now
    ));
    assert_eq!(
        decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO).act,
        Act::Nothing
    );
}

#[test]
fn a_saved_retry_cannot_revive_after_new_native_input_or_abort() {
    let now = Instant::now();
    let start = event(
        json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
    );
    for barrier in [
        event(
            json!({"type":"user_message", "message":"a corrected request", "images":null,
            "local_images":[], "text_elements":[]}),
        ),
        event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"})),
        event(
            json!({"type":"task_started", "turn_id":"newer", "turn_attribution": {"turn_id": "newer", "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
    ] {
        let items = [
            start.clone(),
            complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
            barrier,
            complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
        ];
        let mut resumed = state();
        assert!(!recovery::restore(
            &mut resumed,
            &items,
            &server(),
            now,
            EPOCH,
            &Timing::default()
        ));
    }
}

#[test]
fn a_legacy_failure_without_classification_cannot_become_a_crash_continuation() {
    let now = Instant::now();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        event(json!({"type":"task_complete", "turn_id":FAILED,
            "error":{"message":"the previous provider failed without an error tag"}})),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(
        matches!(decision.act, Act::Attention { .. }),
        "{decision:?}"
    );
    assert!(!resumed.carry_on);
    assert!(resumed.owed.is_none());
}

#[test]
fn a_resumed_other_failure_needs_its_saved_recovery_before_any_retry() {
    let now = Instant::now();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        complete(FAILED, Some(CodexErrorInfo::Other)),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(
        matches!(decision.act, Act::Attention { .. }),
        "{decision:?}"
    );
    assert!(recovery::restore(
        &mut resumed,
        &items,
        &server(),
        now,
        EPOCH,
        &Timing::default()
    ));
    let recovered = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(matches!(recovered.act, Act::Type { .. }), "{recovered:?}");
    assert!(recovered.not_before.is_some_and(|at| at > now));
}

#[test]
fn a_valid_saved_recovery_keeps_its_carried_ladder_before_native_fallback() {
    let now = Instant::now();
    let timing = Timing::default();
    let cases = [
        (
            "server",
            CodexErrorInfo::Other,
            server().wall,
            Duration::from_secs(30 * 60),
        ),
        (
            "network",
            CodexErrorInfo::HttpConnectionFailed {
                http_status_code: None,
            },
            RecoveryWall::Network {
                what: "API unreachable".into(),
                measured: false,
            },
            Duration::from_secs(5 * 60),
        ),
        (
            "server",
            CodexErrorInfo::Other,
            RecoveryWall::Overlong {
                what: "output token limit".into(),
            },
            Duration::from_secs(15 * 60),
        ),
        (
            "entitlement",
            CodexErrorInfo::UsageLimitExceeded,
            RecoveryWall::Entitlement { quota: None },
            Duration::from_secs(360 * 60),
        ),
        (
            "usage",
            CodexErrorInfo::UsageLimitExceeded,
            RecoveryWall::Usage {
                quota: None,
                reset_at_ms: None,
                reset_label: None,
            },
            Duration::from_secs(30 * 60),
        ),
    ];
    for (class, info, wall, next_wait) in cases {
        let saved = Recovery {
            not_before_ms: EPOCH + 7 * 60 * MINUTE,
            wall,
            ..server()
        };
        let carried = crate::decide::WallTrack::carried(
            class,
            now - Duration::from_secs(10 * 60),
            2,
            Some(now - Duration::from_secs(60)),
            None,
            false,
        )
        .expect("carried wall class");
        let mut restored = state();
        restored.seed_pace(4, Some(carried.clone()));
        let items = [
            event(
                json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
            ),
            complete(FAILED, Some(info)),
        ];
        let mut recovered = false;
        assert!(crate::extension::resume_bound_with(
            &mut restored,
            &items,
            false,
            Some(crate::owed_origin::BoundHandoff::empty()),
            0,
            now,
            |state| {
                recovered = recovery::restore(state, &items, &saved, now, EPOCH, &timing);
                recovered
            }
        ));
        assert!(recovered);
        assert_eq!(
            restored.wall.as_ref(),
            Some(&carried),
            "{class}: a provisional fatal must not erase the carried rung"
        );
        assert_eq!(restored.short_streak, 4);
        let first = decide(&restored, &policy(), &timing, now, Duration::ZERO);
        let due = now + Duration::from_secs(7 * 60 * 60);
        assert_eq!(
            first.not_before,
            Some(due),
            "{class}: the exact saved lower bound is retained"
        );
        let wall = restored
            .point
            .as_ref()
            .and_then(|point| point.wall.clone())
            .expect("restored wall");
        restored.acted(&first.act, due, &timing);
        assert_eq!(restored.wall.as_ref().map(|track| track.attempts), Some(3));
        restored.observe_turn_end(
            TurnEnd {
                at: due + Duration::from_secs(1),
                by_autopilot: true,
                by_person: false,
                text: None,
                earlier: None,
                questions: false,
                worked: Duration::ZERO,
                wall: Some(wall),
                aborted: false,
                ran_tools: false,
                restored: false,
            },
            &timing,
        );
        assert_eq!(
            decide(
                &restored,
                &policy(),
                &timing,
                due + Duration::from_secs(1),
                Duration::ZERO
            )
            .not_before,
            // Counted from the failure's answer, a second after the try (aterm's ladder,
            // parity-3: every wall's ladder counts from its answer).
            Some(due + Duration::from_secs(1) + next_wait),
            "{class}: the next failure continues its carried retry rung"
        );
    }
}

#[test]
fn a_rejected_saved_recovery_keeps_native_fatal_attention_and_drops_the_old_ladder() {
    let now = Instant::now();
    let timing = Timing::default();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        complete(FAILED, Some(CodexErrorInfo::Other)),
    ];
    for saved in [
        Recovery {
            turn_id: "old-attempt".into(),
            ..server()
        },
        Recovery {
            wall: RecoveryWall::Network {
                what: "wrong old class".into(),
                measured: false,
            },
            ..server()
        },
    ] {
        let mut restored = state();
        restored.seed_pace(
            4,
            crate::decide::WallTrack::carried("server", now, 2, Some(now), None, false),
        );
        let mut recovered = true;
        assert!(crate::extension::resume_bound_with(
            &mut restored,
            &items,
            true,
            Some(crate::owed_origin::BoundHandoff::empty()),
            0,
            now,
            |state| {
                recovered = recovery::restore(state, &items, &saved, now, EPOCH, &timing);
                recovered
            }
        ));
        assert!(!recovered);
        assert!(matches!(
            decide(&restored, &policy(), &timing, now, Duration::ZERO).act,
            Act::Attention { .. }
        ));
        assert_eq!(
            restored
                .wall
                .as_ref()
                .and_then(crate::decide::WallTrack::carried_class),
            None
        );
        assert!(!restored.carry_on);
        assert!(restored.owed.is_none());
    }
}

#[test]
fn a_resumed_correction_or_abort_cannot_be_overwritten_by_an_old_completion() {
    let now = Instant::now();
    let start = event(
        json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
    );
    for barrier in [
        event(
            json!({"type":"user_message", "message":"a corrected request", "images":null,
            "local_images":[], "text_elements":[]}),
        ),
        event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"})),
    ] {
        let items = [
            start.clone(),
            complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
            barrier,
            complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
        ];
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &items,
            false,
            None,
            0,
            now
        ));
        let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
        assert_eq!(decision.act, Act::Nothing, "{items:?}: {decision:?}");
    }
}

#[test]
fn definitive_native_refusals_restore_attention_without_replaying_old_acts() {
    let now = Instant::now();
    let refused = [
        CodexErrorInfo::BioPolicy,
        CodexErrorInfo::MisalignmentPolicyViolation,
        CodexErrorInfo::Unauthorized,
        CodexErrorInfo::BadRequest,
        CodexErrorInfo::InvalidPrompt,
        CodexErrorInfo::SessionBudgetExceeded,
        CodexErrorInfo::TooManyDenials,
        CodexErrorInfo::SandboxError,
        CodexErrorInfo::HttpConnectionFailed {
            http_status_code: Some(401),
        },
        CodexErrorInfo::ResponseStreamDisconnected {
            http_status_code: Some(403),
        },
    ];
    for info in refused {
        let items = [
            event(json!({
                "type": "task_started", "turn_id": FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
                "model_context_window": 258400, "collaboration_mode_kind": "default",
            })),
            complete(FAILED, Some(info.clone())),
        ];
        for relaunched in [false, true] {
            for mode in [Mode::Cruise, Mode::Land] {
                let mut resumed = ThreadState::default();
                let mut policy = policy();
                policy.autopilot = mode;
                assert!(crate::extension::resume_into(
                    &mut resumed,
                    &items,
                    relaunched,
                    None,
                    0,
                    now,
                ));
                let decision = decide(&resumed, &policy, &Timing::default(), now, Duration::ZERO);
                assert!(
                    matches!(decision.act, Act::Attention { .. }),
                    "{info:?}: {decision:?}"
                );
                assert_eq!(decision.not_before, None);
                assert!(!resumed.carry_on);
                assert!(resumed.owed.is_none());
            }
        }
    }
}

#[test]
fn only_the_latest_native_attempt_can_leave_a_resumed_refusal_standing() {
    let now = Instant::now();
    let start = |turn| {
        event(json!({
            "type": "task_started", "turn_id": turn, "turn_attribution": {"turn_id": turn, "turn_trigger": "user"},
            "model_context_window": 258400, "collaboration_mode_kind": "default",
        }))
    };
    let base = vec![
        start(FAILED),
        complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
    ];
    let tails = [
        vec![start("corrected")],
        vec![start("corrected"), complete("corrected", None)],
        vec![event(
            json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"}),
        )],
        vec![event(
            json!({"type":"user_message", "message":"a corrected request", "images":null,
            "local_images":[], "text_elements":[]}),
        )],
        vec![
            start("corrected"),
            complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
        ],
    ];
    for tail in tails {
        let mut items = base.clone();
        items.extend(tail);
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &items,
            false,
            None,
            0,
            now,
        ));
        let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
        assert_eq!(decision.act, Act::Nothing, "{items:?}: {decision:?}");
    }
    // A retryable failure and a terminal success gain no attention authority here.
    for info in [None, Some(CodexErrorInfo::ServerOverloaded)] {
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &[start(FAILED), complete(FAILED, info)],
            false,
            None,
            0,
            now,
        ));
        assert_eq!(
            decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO).act,
            Act::Nothing
        );
    }
}

fn server() -> Recovery {
    Recovery {
        thread: "the-primary-thread".into(),
        turn_id: FAILED.into(),
        point_at_ms: EPOCH,
        not_before_ms: EPOCH + 30 * MINUTE,
        by_autopilot: true,
        by_person: false,
        wall: RecoveryWall::Server {
            what: "model at capacity".into(),
            cut_off: false,
            retry_at_ms: Some(EPOCH + 20 * MINUTE),
        },
    }
}

fn usage() -> Recovery {
    Recovery {
        not_before_ms: EPOCH + 20 * MINUTE + 70_000,
        wall: RecoveryWall::Usage {
            quota: None,
            reset_at_ms: Some(EPOCH + 20 * MINUTE),
            reset_label: Some("the provider's original reset".into()),
        },
        ..server()
    }
}

fn assert_serialized_deadline(before: Option<Instant>, after: Option<Instant>) {
    let before = before.expect("original future wait");
    let after = after.expect("restored future wait");
    assert!(
        after >= before,
        "serialization cannot shorten a lower bound"
    );
    assert!(
        after <= before + Duration::from_millis(2),
        "fixed samples add at most two millisecond rounding steps"
    );
}

#[test]
fn a_floored_epoch_sample_never_shortens_a_future_provider_deadline() {
    let now = Instant::now();
    let timing = Timing::default();
    for fraction in [Duration::ZERO, Duration::from_nanos(1)] {
        let original = at_wall(
            Wall::Server {
                what: "model at capacity".into(),
                cut_off: false,
                retry_at: Some(now + Duration::from_secs(30 * 60) + fraction),
            },
            now,
            &timing,
        );
        let before = decide(&original, &policy(), &timing, now, Duration::ZERO);
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &before,
            false,
            now,
            EPOCH,
        )
        .expect("future recovery intent");
        // Capture's hidden epoch fraction was 999 microseconds. One microsecond
        // later its floor advances a full millisecond; no wall-clock wait is used.
        let restored_now = now + Duration::from_micros(1);
        let mut restored = state();
        assert!(recovery::restore(
            &mut restored,
            &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
            &captured,
            restored_now,
            EPOCH + 1,
            &timing,
        ));
        let after = decide(&restored, &policy(), &timing, restored_now, Duration::ZERO);
        assert_serialized_deadline(before.not_before, after.not_before);
        assert!(matches!(
            restored.point.as_ref().and_then(|point| point.wall.as_ref()),
            Some(Wall::Server { retry_at: Some(at), .. })
                if *at >= now + Duration::from_secs(30 * 60) + fraction
        ));
    }
}

#[test]
fn only_the_latest_matching_native_failure_can_restore_a_wait() {
    let now = Instant::now();
    let timing = Timing::default();
    let recovery = server();
    let failures = [
        complete("another-turn", Some(CodexErrorInfo::ServerOverloaded)),
        complete(FAILED, None),
        complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded)),
        complete(FAILED, Some(CodexErrorInfo::Unauthorized)),
        complete(FAILED, Some(CodexErrorInfo::BadRequest)),
        complete(FAILED, Some(CodexErrorInfo::ContextWindowExceeded)),
        complete(
            FAILED,
            Some(CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(403),
            }),
        ),
        complete(
            FAILED,
            Some(CodexErrorInfo::HttpConnectionFailed {
                http_status_code: None,
            }),
        ),
        event(
            json!({"type": "task_started", "turn_id": "a-newer-turn", "turn_attribution": {"turn_id": "a-newer-turn", "turn_trigger": "user"}}),
        ),
        event(json!({"type": "user_message", "message": "a new instruction"})),
        event(json!({"type": "turn_aborted", "turn_id": FAILED, "reason": "interrupted"})),
    ];
    for newer in failures {
        let mut restored = state();
        let items = [
            complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
            newer,
        ];
        assert!(!recovery::restore(
            &mut restored,
            &items,
            &recovery,
            now,
            EPOCH,
            &timing,
        ));
        assert!(restored.point.is_none());
        assert!(restored.recovery_not_before.is_none());
    }

    let items = [
        complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
        event(json!({"type": "warning", "message": "a later informational warning"})),
    ];
    assert!(recovery::restore(
        &mut state(),
        &items,
        &recovery,
        now,
        EPOCH,
        &timing,
    ));
    let mut taskless = ThreadState::default();
    assert!(!recovery::restore(
        &mut taskless,
        &items,
        &recovery,
        now,
        EPOCH,
        &timing,
    ));
    assert!(!recovery::restore(
        &mut state(),
        &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
        &usage(),
        now,
        EPOCH,
        &timing,
    ));
}

#[test]
fn native_capacity_and_processing_errors_keep_their_recorded_server_classification() {
    let now = Instant::now();
    let timing = Timing::default();
    let policy = policy();
    for (details, expected_info, provider_deadline) in [
        (CodexErrorDetails::Timeout, CodexErrorInfo::Other, None),
        (
            CodexErrorDetails::RequestTimeout,
            CodexErrorInfo::Other,
            None,
        ),
        (
            CodexErrorDetails::Stream("the response stream ended early".into()),
            CodexErrorInfo::Other,
            None,
        ),
        (
            CodexErrorDetails::PolicyCapacityUnavailable,
            CodexErrorInfo::PolicyCapacityUnavailable,
            Some(now + Duration::from_secs(300)),
        ),
    ] {
        let error = CodexErr::new(details);
        let info = error.to_codex_protocol_error();
        assert_eq!(info, expected_info);
        let mut wall = crate::extension::classify(&info, error.details(), false)
            .expect("the live native failure is recoverable");
        assert!(matches!(wall, Wall::Server { .. }));
        if let Wall::Server { retry_at, .. } = &mut wall {
            // The native error lifecycle retains the provider's advice on this wall.
            *retry_at = provider_deadline;
        }
        let original = at_wall(wall, now, &timing);
        let before = decide(&original, &policy, &timing, now, Duration::ZERO);
        if let Some(deadline) = provider_deadline {
            assert_eq!(before.not_before, Some(deadline));
        }
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &before,
            false,
            now,
            EPOCH,
        )
        .expect("the original provider recovery intent");
        let mut restored = state();
        assert!(recovery::restore(
            &mut restored,
            &[complete(FAILED, Some(info))],
            &captured,
            now,
            EPOCH,
            &timing,
        ));
        let after = decide(&restored, &policy, &timing, now, Duration::ZERO);
        assert_eq!(after.act, before.act);
        assert_serialized_deadline(before.not_before, after.not_before);
    }
}

#[test]
fn network_overlong_and_entitlement_waits_round_trip_without_becoming_unguarded_prompts() {
    let now = Instant::now();
    let timing = Timing::default();
    let network = CodexErrorInfo::HttpConnectionFailed {
        http_status_code: None,
    };
    let network_wall = crate::extension::classify(
        &network,
        &CodexErrorDetails::Stream("no connection".into()),
        false,
    )
    .expect("native network classification");
    let incomplete = CodexErr::new(CodexErrorDetails::ResponseIncomplete(Box::new(
        ResponseIncompleteError {
            partial_output: Vec::new(),
            reason: "max_output_tokens".into(),
            response_id: None,
            token_usage: None,
        },
    )));
    let entitlement = CodexErr::new(CodexErrorDetails::QuotaExceeded);
    let cases = [
        (network_wall, network),
        (
            crate::extension::classify(
                &incomplete.to_codex_protocol_error(),
                incomplete.details(),
                false,
            )
            .expect("native output limit classification"),
            incomplete.to_codex_protocol_error(),
        ),
        (
            crate::extension::classify(
                &entitlement.to_codex_protocol_error(),
                entitlement.details(),
                false,
            )
            .expect("native entitlement classification"),
            entitlement.to_codex_protocol_error(),
        ),
    ];
    for (wall, info) in cases {
        let mut original = at_wall(wall.clone(), now, &timing);
        // An output-limit continuation can also be waiting on its retry ladder.
        if matches!(wall, Wall::Overlong { .. }) {
            original
                .wall
                .as_mut()
                .expect("output-limit ladder")
                .attempts = 1;
        }
        let before = decide(&original, &policy(), &timing, now, Duration::ZERO);
        assert!(matches!(before.act, Act::Type { .. }));
        assert!(before.not_before.is_some_and(|at| at > now));
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &before,
            false,
            now,
            EPOCH,
        )
        .expect("this retryable wait crosses a crash");
        assert!(matches!(
            (&wall, &captured.wall),
            (Wall::Network { .. }, RecoveryWall::Network { .. })
                | (Wall::Overlong { .. }, RecoveryWall::Overlong { .. })
                | (Wall::Entitlement, RecoveryWall::Entitlement { .. })
        ));
        let stored: codex_alab_tui::record::Recovery = captured.into();
        let json = serde_json::to_string(&stored).expect("typed session recovery JSON");
        let recovered: codex_alab_tui::record::Recovery =
            serde_json::from_str(&json).expect("recovery JSON remains readable");
        let mut restored = state();
        assert!(
            recovery::restore(
                &mut restored,
                &[complete(FAILED, Some(info.clone()))],
                &recovered.into(),
                now,
                EPOCH,
                &timing,
            ),
            "{wall:?} with native protocol error {info:?}"
        );
        let after = decide(&restored, &policy(), &timing, now, Duration::ZERO);
        assert_eq!(after.act, before.act);
        assert_serialized_deadline(before.not_before, after.not_before);
        let disabled = Policy {
            retry_api_errors: false,
            resume_limits: false,
            ..policy()
        };
        assert!(matches!(
            decide(&restored, &disabled, &timing, now, Duration::ZERO).act,
            Act::Attention { .. }
        ));
    }
}

#[test]
fn auth_fatal_and_policy_refusals_never_create_recovery_metadata() {
    let now = Instant::now();
    let timing = Timing::default();
    for info in [
        CodexErrorInfo::Unauthorized,
        CodexErrorInfo::BadRequest,
        CodexErrorInfo::CyberPolicy,
        CodexErrorInfo::ContextWindowExceeded,
    ] {
        let wall = crate::extension::classify(
            &info,
            &CodexErrorDetails::Stream("terminal refusal".into()),
            false,
        )
        .expect("the native refusal is classified");
        let original = at_wall(wall, now, &timing);
        let decision = decide(&original, &policy(), &timing, now, Duration::ZERO);
        if info == CodexErrorInfo::CyberPolicy {
            assert!(matches!(
                decision.act,
                Act::Type {
                    rule: crate::words::RULE_POLICY_CONTINUE,
                    ..
                }
            ));
        } else {
            assert!(matches!(decision.act, Act::Attention { .. }));
        }
        assert!(
            recovery::capture(
                "the-primary-thread",
                FAILED,
                &original,
                &decision,
                false,
                now,
                EPOCH,
            )
            .is_none()
        );
    }
}

#[test]
fn server_recovery_honors_both_original_deadlines_after_elapsed_downtime() {
    let now = Instant::now();
    let epoch = EPOCH + 10 * MINUTE;
    let timing = Timing::default();
    for (provider_minutes, original_minutes) in [(40, 30), (20, 45)] {
        let recovery = Recovery {
            not_before_ms: EPOCH + original_minutes * MINUTE,
            wall: RecoveryWall::Server {
                what: "model at capacity".into(),
                cut_off: false,
                retry_at_ms: Some(EPOCH + provider_minutes * MINUTE),
            },
            ..server()
        };
        let mut restored = state();
        assert!(recovery::restore(
            &mut restored,
            &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
            &recovery,
            now,
            epoch,
            &timing,
        ));
        let decision = decide(&restored, &policy(), &timing, now, Duration::ZERO);
        assert!(matches!(
            decision.act,
            Act::Type {
                rule: crate::words::RULE_API_RETRY,
                ..
            }
        ));
        assert_eq!(
            decision.not_before,
            Some(
                now + Duration::from_millis((provider_minutes.max(original_minutes) - 10) * MINUTE)
            ),
            "downtime spends the original wait instead of restarting it"
        );
        let longer = Timing {
            retry_backoff: vec![Duration::from_secs(60 * 60)],
            ..timing.clone()
        };
        let extended = decide(&restored, &policy(), &longer, now, Duration::ZERO);
        assert!(extended.not_before > decision.not_before);
    }
}

#[test]
fn usage_reset_grace_is_applied_to_the_provider_reset_and_not_to_the_carried_floor() {
    let now = Instant::now();
    let epoch = EPOCH + 5 * MINUTE;
    let timing = Timing::default();
    let recovery = usage();
    let mut restored = state();
    assert!(recovery::restore(
        &mut restored,
        &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
        &recovery,
        now,
        epoch,
        &timing,
    ));
    let reset = now + Duration::from_millis(15 * MINUTE);
    assert!(matches!(
        restored.point.as_ref().and_then(|point| point.wall.as_ref()),
        Some(Wall::Usage { reset_at: Some(at), .. }) if *at == reset
    ));
    let first = decide(&restored, &policy(), &timing, now, Duration::ZERO);
    assert!(matches!(first.act, Act::ResumeLimit { .. }));
    assert_eq!(first.not_before, Some(reset + Duration::from_secs(70)));
    assert_eq!(
        decide(&restored, &policy(), &timing, now, Duration::ZERO),
        first,
        "re-evaluation does not add another reset grace"
    );
    let longer = Timing {
        reset_grace: Duration::from_secs(180),
        ..timing
    };
    assert_eq!(
        decide(&restored, &policy(), &longer, now, Duration::ZERO).not_before,
        Some(reset + Duration::from_secs(180)),
        "a live longer grace still extends the original floor"
    );
}

#[test]
fn restored_intents_do_not_override_live_retry_or_usage_switches() {
    let now = Instant::now();
    let timing = Timing::default();
    let mut server_state = state();
    assert!(recovery::restore(
        &mut server_state,
        &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
        &server(),
        now,
        EPOCH,
        &timing,
    ));
    let disabled = Policy {
        retry_api_errors: false,
        ..policy()
    };
    let decision = decide(&server_state, &disabled, &timing, now, Duration::ZERO);
    assert!(matches!(decision.act, Act::Attention { key: "api", .. }));
    assert_eq!(decision.not_before, None);
    assert!(matches!(
        decide(&server_state, &policy(), &timing, now, Duration::ZERO).act,
        Act::Type {
            rule: crate::words::RULE_API_RETRY,
            ..
        }
    ));

    let mut usage_state = state();
    assert!(recovery::restore(
        &mut usage_state,
        &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
        &usage(),
        now,
        EPOCH,
        &timing,
    ));
    for disabled in [
        Policy {
            resume_limits: false,
            ..policy()
        },
        Policy {
            limit_wait: false,
            ..policy()
        },
    ] {
        let decision = decide(&usage_state, &disabled, &timing, now, Duration::ZERO);
        assert!(matches!(decision.act, Act::Attention { key: "limit", .. }));
        assert_eq!(decision.not_before, None);
    }
    usage_state.park_off();
    assert!(usage_state.recovery_not_before.is_none());
    assert_eq!(
        Mode::of_status(codex_alab_aterm::status::Autopilot::Off),
        None,
        "Off supplies no active mode for the supervisor to dispatch"
    );
}

#[test]
fn a_past_usage_reset_keeps_a_valid_immediate_recovery_boundary() {
    let now = Instant::now();
    let timing = Timing::default();
    let original = at_wall(
        Wall::Usage {
            reset_at: Some(now - Duration::from_secs(10 * 60)),
            reset_label: Some("a reset already passed".into()),
        },
        now,
        &timing,
    );
    let decision = decide(&original, &policy(), &timing, now, Duration::ZERO);
    assert!(decision.not_before.is_some_and(|at| at < now));
    let captured = recovery::capture(
        "the-primary-thread",
        FAILED,
        &original,
        &decision,
        false,
        now,
        EPOCH,
    )
    .expect("the immediate native usage recovery intent");
    assert_eq!(captured.point_at_ms, EPOCH);
    assert_eq!(captured.not_before_ms, EPOCH);
    assert!(matches!(
        captured.wall,
        RecoveryWall::Usage { reset_at_ms: Some(at), .. } if at < captured.point_at_ms
    ));
    let mut restored = state();
    assert!(recovery::restore(
        &mut restored,
        &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
        &captured,
        now,
        EPOCH,
        &timing,
    ));
    let after = decide(&restored, &policy(), &timing, now, Duration::ZERO);
    assert!(matches!(after.act, Act::ResumeLimit { .. }));
    assert_eq!(after.not_before, Some(now));
}

/// A MEASURED RETURN'S ACT LEAVES A RECOVERY THAT MEASURES AGAIN (B2): the API measured
/// reachable again, the wall's act is `api-back@v1`, which a check may hold back (the record's
/// `owed` then carries the blind retry); its point leaves a network recovery all the same,
/// naming the route as measured, so the process after a crash restores the wall and measures
/// its route ([`recovery::rearms`]) rather than finding no recovery and no try. Before, only
/// the blind retry left one. NEGATIVE CONTROL: a route not measured leaves a recovery that
/// measures nothing.
#[test]
fn a_measured_returns_act_leaves_a_recovery_that_measures_again() {
    let now = Instant::now();
    let timing = Timing::default();
    let what = "an API error (the API could not be reached)";
    let mut original = at_wall(Wall::Network { what: what.into() }, now, &timing);
    original.reach = crate::netwatch::Reach::Up { since: now };
    let decision = decide(&original, &policy(), &timing, now, Duration::ZERO);
    assert!(matches!(
        decision.act,
        Act::Type {
            rule: crate::words::RULE_API_BACK,
            ..
        }
    ));
    for measured in [true, false] {
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &decision,
            measured,
            now,
            EPOCH,
        )
        .unwrap_or_else(|| panic!("measured={measured}: the measured return's point recovers"));
        assert_eq!(
            captured.wall,
            RecoveryWall::Network {
                what: what.into(),
                measured,
            }
        );
        assert_eq!(recovery::rearms(&captured), measured);
    }
}

/// A RESTORED POINT LEAVES RULING F'S REPLIES A RE-EXEC SEEDED (parity review of the stage-1
/// port, test lens): the resume seeds the record's replies before a recovery or a terminal
/// wall restores its point, and both restore it as a restored end, which neither counts nor
/// lifts; the hold stands across either. NEGATIVE CONTROL: the same usage point seen live (no
/// act awaited, no one's prompt but codex-alab's) is another turn, and lifts the replies.
#[test]
fn a_restored_point_leaves_the_replies_a_re_exec_seeded() {
    use crate::replies::HeldBy;
    use crate::replies::Kept;
    use crate::replies::Replies;
    let now = Instant::now();
    let timing = Timing::default();
    let kept = Replies::of(&[Kept {
        mark: 0x5e,
        answers: 2,
        walled: false,
        tries: 0,
    }]);
    // A valid recovery.
    let mut recovered = state();
    recovered.seed_replies(kept, Some(now));
    assert!(recovery::restore(
        &mut recovered,
        &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
        &usage(),
        now,
        EPOCH,
        &timing
    ));
    assert_eq!(recovered.replies, kept);
    assert_eq!(recovered.held_by(), Some(HeldBy::SameReply));
    // A terminal wall the resumed history ends on.
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        complete(FAILED, Some(CodexErrorInfo::Other)),
    ];
    let mut resumed = ThreadState::default();
    resumed.seed_replies(kept, Some(now));
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    assert!(resumed.point.as_ref().is_some_and(|p| p.restored));
    assert_eq!(resumed.replies, kept);
    assert_eq!(resumed.held_by(), Some(HeldBy::SameReply));
    // NEGATIVE CONTROL: the usage point seen live.
    let mut live = state();
    live.seed_replies(kept, Some(now));
    live.observe_turn_end(
        TurnEnd {
            at: now,
            by_autopilot: true,
            by_person: false,
            text: None,
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall: Some(Wall::Usage {
                reset_at: None,
                reset_label: Some("the provider's original reset".into()),
            }),
            aborted: false,
            ran_tools: false,
            restored: false,
        },
        &timing,
    );
    assert_eq!(live.replies, Replies::default());
}
