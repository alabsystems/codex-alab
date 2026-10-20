// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Which wall a terminal turn error is (`extension::classify`), with the end-to-end run's
//! two misreadings (2026-10-16) and their negative controls.

use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use pretty_assertions::assert_eq;

use crate::decide::Wall;
use crate::extension::classify;

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
    for error in [
        CodexErrorInfo::HttpConnectionFailed {
            http_status_code: Some(401),
        },
        CodexErrorInfo::ResponseStreamConnectionFailed {
            http_status_code: Some(401),
        },
        CodexErrorInfo::ResponseTooManyFailedAttempts {
            http_status_code: Some(401),
        },
        CodexErrorInfo::Unauthorized,
    ] {
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
    assert_eq!(
        classify(
            &CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(502),
            },
            &CodexErrorDetails::TurnAborted,
            /*auto_compact*/ true
        ),
        Some(Wall::Server {
            what: "an API error (502)".to_string(),
            cut_off: false,
        })
    );
}
