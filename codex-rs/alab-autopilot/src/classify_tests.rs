// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Which wall a terminal turn error is (`extension::classify`), with the end-to-end run's
//! two misreadings (2026-10-16) and their negative controls.

use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::ProviderRejection;
use codex_protocol::error::ResponseIncompleteError;
use codex_protocol::error::UnexpectedResponseError;
use codex_protocol::protocol::CodexErrorInfo;
use pretty_assertions::assert_eq;

use crate::decide::Wall;
use crate::extension::classify;

#[test]
fn a_cyber_policy_refusal_has_its_own_bounded_independent_work_wall() {
    assert_eq!(
        classify(
            &CodexErrorInfo::CyberPolicy,
            &CodexErrorDetails::Stream("provider refusal".into()),
            true,
        ),
        Some(Wall::Policy)
    );
}

#[test]
fn other_usage_policy_refusals_remain_terminal_with_their_own_cause() {
    for error in [
        CodexErrorInfo::BioPolicy,
        CodexErrorInfo::MisalignmentPolicyViolation,
    ] {
        let Some(Wall::Fatal { why }) = classify(
            &error,
            &CodexErrorDetails::Stream("provider refusal".into()),
            true,
        ) else {
            panic!("{error:?} must remain a fatal wall");
        };
        assert!(why.contains("usage policy"), "{error:?}: {why}");
        assert!(!why.contains("cyberPolicy"), "{error:?}: {why}");
    }
}

#[test]
fn a_model_the_provider_does_not_offer_is_terminal_not_a_retry() {
    let error = CodexErr::new(CodexErrorDetails::ModelNotFound {
        message: "Access to this deployment ended.".into(),
        status: None,
    });
    let Some(Wall::Fatal { why }) =
        classify(&error.to_codex_protocol_error(), error.details(), true)
    else {
        panic!("a missing model must be a fatal wall");
    };
    assert!(why.contains("model"), "{why}");
}

#[test]
fn a_stream_cut_after_cores_retries_is_a_cut_off_server_wall() {
    let wall = classify(
        &CodexErrorInfo::Other,
        &CodexErrorDetails::Stream("stream closed before response.completed".to_string()),
        /*auto_compact*/ true,
    );
    assert_eq!(
        wall,
        Some(Wall::Server {
            what: "an API error (the reply was cut off)".to_string(),
            cut_off: true,
            retry_at: None,
        })
    );
    // Negative control: an `Other` whose details name no wall is still no wall.
    assert_eq!(
        classify(
            &CodexErrorInfo::Other,
            &CodexErrorDetails::TurnAborted,
            /*auto_compact*/ true
        ),
        None
    );
}

#[test]
fn a_401_on_any_request_lane_is_the_login_wall() {
    let mut errors = status_errors(401).to_vec();
    errors.push(CodexErrorInfo::Unauthorized);
    for error in errors {
        assert_eq!(
            classify(
                &error,
                &CodexErrorDetails::TurnAborted,
                /*auto_compact*/ true
            ),
            Some(Wall::Login),
            "{error:?}"
        );
    }
    // Negative control: another status stays the server's ladder.
    assert!(matches!(
        classify(
            &CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(502),
            },
            &CodexErrorDetails::TurnAborted,
            /*auto_compact*/ true
        ),
        Some(Wall::Server { cut_off: false, .. })
    ));
}

fn status_errors(code: u16) -> [CodexErrorInfo; 4] {
    [
        CodexErrorInfo::HttpConnectionFailed {
            http_status_code: Some(code),
        },
        CodexErrorInfo::ResponseStreamConnectionFailed {
            http_status_code: Some(code),
        },
        CodexErrorInfo::ResponseTooManyFailedAttempts {
            http_status_code: Some(code),
        },
        CodexErrorInfo::ResponseStreamDisconnected {
            http_status_code: Some(code),
        },
    ]
}

#[test]
fn permanent_client_refusals_do_not_enter_the_server_retry_ladder() {
    for code in [400, 402, 403, 404, 405, 413, 422, 451] {
        for error in status_errors(code) {
            assert!(
                matches!(
                    classify(&error, &CodexErrorDetails::TurnAborted, true),
                    Some(Wall::Fatal { .. })
                ),
                "{error:?}"
            );
        }
    }
}

#[test]
fn terminal_http_refusals_keep_status_over_capacity_body_text() {
    for status in [401, 403] {
        for code in ["server_is_overloaded", "policy_capacity_unavailable"] {
            let error = CodexErr::UnexpectedStatus(UnexpectedResponseError {
                status: status.to_string().parse().expect("fixture HTTP status"),
                body: serde_json::json!({"error":{"code":code}}).to_string(),
                user_message: None,
                url: None,
                cf_ray: None,
                request_id: None,
                identity_authorization_error: None,
                identity_error_code: None,
            });
            let terminal = error.to_error_event(None);
            let info = terminal.codex_error_info.expect("terminal error identity");
            assert_eq!(
                info,
                CodexErrorInfo::HttpConnectionFailed {
                    http_status_code: Some(status),
                }
            );
            let wall = classify(&info, error.details(), true).expect("terminal refusal wall");
            if status == 401 {
                assert_eq!(wall, Wall::Login);
            } else {
                assert!(matches!(wall, Wall::Fatal { .. }));
            }
        }
    }
}

#[test]
fn temporary_status_failures_keep_their_retry_and_cutoff_behavior() {
    for code in [408, 409, 425, 429, 500, 502, 503, 504] {
        for error in status_errors(code) {
            let expected_cutoff =
                matches!(error, CodexErrorInfo::ResponseStreamDisconnected { .. });
            assert!(
                matches!(
                    classify(&error, &CodexErrorDetails::TurnAborted, true),
                    Some(Wall::Server { cut_off, .. }) if cut_off == expected_cutoff
                ),
                "{error:?}"
            );
        }
    }
}

#[test]
fn terminal_details_do_not_look_like_successful_turn_ends() {
    for details in [
        CodexErrorDetails::InvalidRequest("invalid request".to_string()),
        CodexErrorDetails::InvalidImageRequest(),
        CodexErrorDetails::ToolCollision("duplicate tool".to_string()),
        CodexErrorDetails::Fatal("configuration refused".to_string()),
        CodexErrorDetails::ContentFilter,
    ] {
        assert!(
            matches!(
                classify(&CodexErrorInfo::Other, &details, true),
                Some(Wall::Fatal { .. })
            ),
            "{details:?}"
        );
    }
    // A cancelled turn is handled by the abort callback, rather than a refusal wall.
    assert_eq!(
        classify(
            &CodexErrorInfo::Other,
            &CodexErrorDetails::TurnAborted,
            true
        ),
        None
    );
}

/// A reply past its output token maximum is aterm's Overlong wall, retried in smaller pieces
/// (`decide_tests`); its words name the cause, never the error's text. NEGATIVE CONTROL:
/// another reason a response stopped short is no retry's
/// (`remaining_terminal_errors_never_look_like_successful_work`).
#[test]
fn a_reply_past_its_output_token_maximum_is_the_overlong_wall() {
    let error = CodexErr::new(CodexErrorDetails::ResponseIncomplete(Box::new(
        ResponseIncompleteError {
            partial_output: Vec::new(),
            reason: "max_output_tokens".into(),
            response_id: Some("incomplete".into()),
            token_usage: None,
        },
    )));
    assert_eq!(
        classify(&error.to_codex_protocol_error(), error.details(), true),
        Some(Wall::Overlong {
            what: "an API error (the reply was over its output token maximum)".to_string(),
        })
    );
}

#[test]
fn remaining_terminal_errors_never_look_like_successful_work() {
    for details in [
        CodexErrorDetails::ResponseIncomplete(Box::new(ResponseIncompleteError {
            partial_output: Vec::new(),
            reason: "some_other_reason".into(),
            response_id: Some("incomplete".into()),
            token_usage: None,
        })),
        CodexErrorDetails::Spawn,
        CodexErrorDetails::LandlockSandboxExecutableNotProvided,
    ] {
        let error = CodexErr::new(details);
        assert!(
            matches!(
                classify(&error.to_codex_protocol_error(), error.details(), true),
                Some(Wall::Fatal { .. })
            ),
            "{error:?}"
        );
    }
    for info in [CodexErrorInfo::TooManyDenials, CodexErrorInfo::SandboxError] {
        assert!(matches!(
            classify(&info, &CodexErrorDetails::TurnAborted, true),
            Some(Wall::Fatal { .. })
        ));
    }
}

#[test]
fn exhausted_response_processing_failures_use_the_retry_ladder() {
    for details in [
        CodexErrorDetails::Timeout,
        CodexErrorDetails::Io(std::io::Error::other("response read failed")),
        CodexErrorDetails::Json(
            serde_json::from_str::<serde_json::Value>("invalid response")
                .err()
                .unwrap_or_else(|| panic!("invalid JSON")),
        ),
    ] {
        let error = CodexErr::new(details);
        assert!(
            matches!(
                classify(&error.to_codex_protocol_error(), error.details(), true),
                Some(Wall::Server { cut_off: false, .. })
            ),
            "{error:?}"
        );
    }
    for details in [
        CodexErrorDetails::TurnAborted,
        CodexErrorDetails::Interrupted,
    ] {
        let error = CodexErr::new(details);
        assert_eq!(
            classify(&error.to_codex_protocol_error(), error.details(), true),
            None
        );
    }
}

#[test]
fn inference_hook_denials_never_enter_retry_or_independent_work_continuation() {
    for auto_compact in [false, true] {
        let details = CodexErrorDetails::InferenceHookDenied {
            message: "server_is_overloaded; workspace hook refused".into(),
        };
        let Some(Wall::Fatal { why }) =
            classify(&CodexErrorInfo::InferenceHookDenied, &details, auto_compact)
        else {
            panic!("inference-hook denial must remain terminal");
        };
        assert!(why.contains("workspace inference hook"));
    }
}

#[test]
fn streamed_access_program_denial_never_enters_network_recovery() {
    let error = CodexErr::new(CodexErrorDetails::AccessProgramNotEnabled(
        ProviderRejection::Stream("This access program is disabled.".to_string()),
    ));
    let event = error.to_error_event(None);
    let info = event.codex_error_info.expect("terminal error identity");
    assert_eq!(
        info,
        CodexErrorInfo::HttpConnectionFailed {
            http_status_code: None,
        }
    );
    for auto_compact in [false, true] {
        let Some(Wall::Fatal { why }) = classify(&info, error.details(), auto_compact) else {
            panic!("an explicit program refusal must never schedule a network retry");
        };
        assert!(why.contains("access program"), "{why}");
    }
    // An ordinary status-less connection failure still needs network recovery.
    assert!(matches!(
        classify(&info, &CodexErrorDetails::Stream("connection lost".into()), true),
        Some(Wall::Network { .. })
    ));
}
