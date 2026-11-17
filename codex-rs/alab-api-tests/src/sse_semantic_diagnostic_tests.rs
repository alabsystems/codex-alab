// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Typed semantic categories must survive the public SSE diagnostic boundary.

use super::*;
use codex_protocol::protocol::MisalignmentErrorDetails;
use codex_protocol::protocol::MisalignmentSteer;

const MISALIGNMENT_TYPE: &str = "never-log-private-misalignment-type";
const MISALIGNMENT_EXPLANATION: &str = "never-log-private-misalignment-explanation";
const MISALIGNMENT_STEER: &str = "never-log-private-misalignment-steer";
const MISALIGNMENT_TARGET: &str = " RB/opaque== ";

fn assert_misalignment_details(details: Option<&MisalignmentErrorDetails>, complete: bool) {
    if !complete {
        assert!(details.is_none());
        return;
    }
    let details = details.expect("complete typed misalignment details");
    assert_eq!(details.error_type.as_deref(), Some(MISALIGNMENT_TYPE));
    assert_eq!(
        details.detailed_explanation.as_deref(),
        Some(MISALIGNMENT_EXPLANATION)
    );
    assert_eq!(
        details.steer.as_ref().map(|steer| steer.message.as_str()),
        Some(MISALIGNMENT_STEER)
    );
    assert_eq!(details.review_target.as_deref(), Some(MISALIGNMENT_TARGET));
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Quota,
    Usage,
    Budget,
    RateLimit,
    Overloaded,
    PolicyCapacity,
    Audio,
    Model,
    Access,
    Inference,
    ContentPolicy,
    Cyber,
    Bio,
    Misalignment,
    MisalignmentComplete,
    MisalignmentMalformed,
    Flex,
    ContentFilter,
    InvalidPrompt,
    Retryable,
}

impl Case {
    fn label(self) -> &'static str {
        match self {
            Self::Quota => "quota_exceeded",
            Self::Usage => "usage_not_included",
            Self::Budget => "session_budget_exceeded",
            Self::RateLimit => "rate_limit_exceeded",
            Self::Overloaded => "server_overloaded",
            Self::PolicyCapacity => "policy_capacity_unavailable",
            Self::Audio => "invalid_audio_input",
            Self::Model => "model_not_found",
            Self::Access => "access_program_not_enabled",
            Self::Inference => "inference_hook_denied",
            Self::ContentPolicy => "content_policy_violation",
            Self::Cyber => "cyber_policy",
            Self::Bio => "bio_policy",
            Self::Misalignment | Self::MisalignmentComplete | Self::MisalignmentMalformed => {
                "misalignment_policy_violation"
            }
            Self::Flex => "flex_unavailable",
            Self::ContentFilter => "content_filter",
            Self::InvalidPrompt => "invalid_prompt",
            Self::Retryable => "retryable",
        }
    }

    fn advised(self) -> bool {
        matches!(
            self,
            Self::RateLimit | Self::Overloaded | Self::PolicyCapacity | Self::Retryable
        )
    }

    fn frame(self, wire_advice: Option<&str>) -> Value {
        if matches!(self, Self::ContentFilter) {
            return json!({"type":"response.incomplete", "response":{
                "incomplete_details":{"reason":"content_filter"}
            }});
        }
        let code = match self {
            Self::Quota => "insufficient_quota",
            Self::Usage => "usage_not_included",
            Self::Budget => "budget_reached",
            Self::RateLimit => "rate_limit_exceeded",
            Self::Overloaded => "server_is_overloaded",
            Self::PolicyCapacity => "policy_capacity_unavailable",
            Self::Audio => "invalid_audio",
            Self::Model => "model_not_found",
            Self::Access => "access_program_not_enabled",
            Self::Inference => "inference_hook_denied",
            Self::ContentPolicy => "content_policy_violation",
            Self::Cyber => "cyber_policy",
            Self::Bio => "bio_policy",
            Self::Misalignment | Self::MisalignmentComplete | Self::MisalignmentMalformed => {
                "misalignment_policy_violation"
            }
            Self::Flex => "flex_unavailable",
            Self::InvalidPrompt => "invalid_prompt",
            Self::Retryable => "unknown_failure",
            Self::ContentFilter => unreachable!(),
        };
        let mut detail = json!({"code":code, "message":PRIVATE});
        if let Some(wire_advice) = wire_advice {
            detail["headers"] = json!({"retry-after":wire_advice, "authorization":PRIVATE});
        }
        if matches!(
            self,
            Self::MisalignmentComplete | Self::MisalignmentMalformed
        ) {
            let mut details = json!({
                "error_type":MISALIGNMENT_TYPE,
                "detailed_explanation":MISALIGNMENT_EXPLANATION,
                "steer":{"message":MISALIGNMENT_STEER},
                "review_target":MISALIGNMENT_TARGET
            });
            if matches!(self, Self::MisalignmentMalformed) {
                details["steer"]["message"] = json!(42);
            }
            detail["misalignment"] = details;
        }
        json!({"type":"response.failed", "response":{"error":detail}})
    }

    fn expected(self) -> ApiError {
        match self {
            Self::Quota => ApiError::QuotaExceeded,
            Self::Usage => ApiError::UsageNotIncluded,
            Self::Budget => ApiError::SessionBudgetExceeded,
            Self::RateLimit => ApiError::RateLimitExceeded {
                message: PRIVATE.into(),
                retry_after: None,
            },
            Self::Overloaded => ApiError::ServerOverloaded { retry_after: None },
            Self::PolicyCapacity => ApiError::PolicyCapacityUnavailable { retry_after: None },
            Self::Audio => ApiError::InvalidAudioInput {
                message: PRIVATE.into(),
                per_recording: true,
            },
            Self::Model => ApiError::ModelNotFound {
                message: PRIVATE.into(),
                status: None,
            },
            Self::Access => ApiError::AccessProgramNotEnabled {
                message: PRIVATE.into(),
            },
            Self::Inference => ApiError::InferenceHookDenied {
                message: PRIVATE.into(),
            },
            Self::ContentPolicy => ApiError::ContentPolicyViolation {
                message: PRIVATE.into(),
                status: None,
            },
            Self::Cyber => ApiError::CyberPolicy {
                message: PRIVATE.into(),
            },
            Self::Bio => ApiError::BioPolicy {
                message: PRIVATE.into(),
            },
            Self::Misalignment | Self::MisalignmentComplete | Self::MisalignmentMalformed => {
                ApiError::MisalignmentPolicyViolation {
                    message: PRIVATE.into(),
                    misalignment: matches!(self, Self::MisalignmentComplete).then(|| {
                        MisalignmentErrorDetails {
                            error_type: Some(MISALIGNMENT_TYPE.into()),
                            detailed_explanation: Some(MISALIGNMENT_EXPLANATION.into()),
                            steer: Some(MisalignmentSteer {
                                message: MISALIGNMENT_STEER.into(),
                            }),
                            review_target: Some(MISALIGNMENT_TARGET.into()),
                        }
                    }),
                }
            }
            Self::Flex => ApiError::FlexUnavailable,
            Self::ContentFilter => ApiError::ContentFilter,
            Self::InvalidPrompt => ApiError::InvalidPrompt {
                message: PRIVATE.into(),
            },
            Self::Retryable => ApiError::Retryable {
                message: PRIVATE.into(),
                retry_after: None,
            },
        }
    }
}

async fn selected(case: Case, tail: Tail, completed: bool) -> Option<String> {
    selected_with_advice(case, tail, completed, case.advised().then_some("7"))
        .await
        .0
}

async fn selected_with_advice(
    case: Case,
    tail: Tail,
    completed: bool,
    wire_advice: Option<&str>,
) -> (Option<String>, Option<String>) {
    let (release, gate) = oneshot::channel();
    let observed = Arc::new(Observed::default());
    let client = codex_api::ResponsesClient::new(
        GatedTransport {
            failure: case.frame(wire_advice),
            tail,
            gate: Mutex::new(Some(gate)),
            observed: observed.clone(),
        },
        codex_api::Provider {
            name: "semantic-fixture".into(),
            base_url: "https://never-log-semantic-url.invalid/v1".into(),
            query_params: None,
            headers: http::HeaderMap::new(),
            retry: codex_api::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::ZERO,
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(1),
        },
        Arc::new(NoAuth),
    );
    let input = json!({"model":"gpt-semantic-fixture", "input":[{
        "type":"message", "role":"user", "content":"never-log-private-semantic-input"
    }]});
    let expected_input = serde_json::to_vec(&input).unwrap();
    let capture = Capture::default();
    let mut stream = {
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        let thread = tracing::info_span!("session_loop", thread_id = "semantic-original-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!(
            "turn",
            thread.id = "semantic-original-thread",
            turn.id = "semantic-original-turn"
        );
        let _turn = turn.enter();
        tokio::time::timeout(
            WAIT,
            client.stream(
                input,
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
        .expect("semantic opening deadline")
        .expect("HTTP 200 opens before the gated failure")
    };
    assert_eq!(capture.0.lock().unwrap().len(), 1, "shape precedes release");
    assert_eq!(*observed.sent.lock().unwrap(), [expected_input]);
    assert_eq!(observed.tail_polls.load(Ordering::SeqCst), 0);
    assert!(!observed.dropped.load(Ordering::SeqCst));
    let received_at = Instant::now();
    release.send(()).unwrap();
    let outcome = tokio::time::timeout(WAIT, async {
        loop {
            match stream.rx_event.recv().await {
                Some(Ok(codex_api::ResponseEvent::RateLimits(_))) => {}
                outcome => break outcome,
            }
        }
    })
    .await
    .expect("selected semantic outcome deadline");
    if completed {
        assert!(
            matches!(outcome, Some(Ok(codex_api::ResponseEvent::Completed { response_id, .. })) if response_id == "later-completion")
        );
    } else {
        let error = match outcome {
            Some(Err(error)) => error,
            other => panic!("{case:?}: expected typed semantic failure, got {other:?}"),
        };
        let expected = case.expected();
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&expected)
        );
        assert_eq!(
            error.to_string(),
            expected.to_string(),
            "original private message survives"
        );
        match &error {
            ApiError::InvalidAudioInput { per_recording, .. } => assert!(*per_recording),
            ApiError::ModelNotFound { status, .. }
            | ApiError::ContentPolicyViolation { status, .. } => assert!(status.is_none()),
            ApiError::MisalignmentPolicyViolation { misalignment, .. } => {
                assert_misalignment_details(
                    misalignment.as_ref(),
                    matches!(case, Case::MisalignmentComplete),
                );
            }
            _ => {}
        }
        let advice = match &error {
            ApiError::Retryable { retry_after, .. }
            | ApiError::RateLimitExceeded { retry_after, .. }
            | ApiError::ServerOverloaded { retry_after }
            | ApiError::PolicyCapacityUnavailable { retry_after } => *retry_after,
            _ => None,
        };
        assert_eq!(advice.is_some(), case.advised() && wire_advice.is_some());
        if let Some(advice) = advice {
            let seconds = match wire_advice {
                Some("7") => 7,
                Some("0") => 0,
                _ => panic!("fixture uses only controlled valid advice"),
            };
            assert_eq!(
                advice.deadline(),
                received_at + Duration::from_secs(seconds)
            );
        }
        assert_eq!(
            Instant::now(),
            received_at,
            "selection does not wait for idle timeout"
        );
        let mapped = codex_api::map_api_error(error);
        if matches!(
            case,
            Case::Misalignment | Case::MisalignmentComplete | Case::MisalignmentMalformed
        ) {
            let CodexErrorDetails::MisalignmentPolicyViolation {
                message,
                misalignment,
            } = mapped.details()
            else {
                panic!("expected mapped typed misalignment refusal");
            };
            assert_eq!(message, PRIVATE);
            assert_misalignment_details(
                misalignment.as_ref(),
                matches!(case, Case::MisalignmentComplete),
            );
            assert!(mapped.retry_delay(1).is_none());
            assert!(mapped.retry_after().is_none());
            assert!(!mapped.requires_http_fallback());
        }
        let mut expected = codex_api::map_api_error(expected);
        if let Some(advice) = advice {
            expected = expected.with_retry_after(advice);
        }
        assert_eq!(
            mapped.to_codex_protocol_error(),
            expected.to_codex_protocol_error()
        );
        assert_eq!(mapped.to_string(), expected.to_string());
        assert_eq!(
            mapped.retry_after(),
            advice,
            "mapping preserves captured advice"
        );
        assert_eq!(
            mapped.retry_delay(1).is_some(),
            expected.retry_delay(1).is_some()
        );
    }
    assert!(
        tokio::time::timeout(WAIT, stream.rx_event.recv())
            .await
            .expect("semantic task closes")
            .is_none()
    );
    assert!(observed.dropped.load(Ordering::SeqCst));
    assert_eq!(
        observed.tail_polls.load(Ordering::SeqCst) > 0,
        matches!(case, Case::ContentFilter | Case::Retryable)
    );
    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), if completed { 1 } else { 2 });
    let shape = &records[0];
    assert_eq!(shape["message"], "Responses request shape");
    assert!(!shape.contains_key("retry_advised"));
    for record in records.iter() {
        assert_eq!(record["thread_id"], "\"semantic-original-thread\"");
        assert_eq!(record["thread.id"], "\"semantic-original-thread\"");
        assert_eq!(record["turn.id"], "\"semantic-original-turn\"");
        for field in [
            "code",
            "error_code",
            "error_message",
            "error.message",
            "body",
            "payload",
            "misalignment",
            "error_type",
            "detailed_explanation",
            "steer",
            "review_target",
        ] {
            assert!(
                !record.contains_key(field),
                "provider payload fields are not logged"
            );
        }
    }
    for private in [
        "never-log",
        "authorization",
        "x-unrelated",
        "retry-after",
        "RB/opaque==",
    ] {
        assert!(
            !format!("{records:?}").contains(private),
            "private content is absent from metadata"
        );
    }
    if !completed {
        let failure = &records[1];
        assert_eq!(failure["message"], "Responses request failed");
        assert_eq!(failure["local_attempt"], shape["local_attempt"]);
        assert_eq!(failure["request_id"], "\"req_audio-1\"");
        assert_eq!(failure["cf_ray"], "\"ray_audio-1\"");
        assert!(
            !failure.contains_key("http_status"),
            "opening 200 is not an error status"
        );
        (
            Some(failure["failure_kind"].clone()),
            failure.get("retry_advised").cloned(),
        )
    } else {
        (None, None)
    }
}

async fn categories(cases: &[Case]) {
    let mut kinds = Vec::new();
    for &case in cases {
        kinds.push((case, selected(case, Tail::Pending, false).await.unwrap()));
    }
    // Exercise every group's values, selection, advice, context and privacy
    // before the old catch-all exposes the missing fixed labels.
    for (case, kind) in kinds {
        assert_eq!(kind, format!("{:?}", case.label()), "{case:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn misalignment_details_survive_selected_sse_failure() {
    if isolated("semantic_categories::misalignment_details_survive_selected_sse_failure").await {
        return;
    }
    codex_alab_core::activate();
    for case in [Case::MisalignmentComplete, Case::MisalignmentMalformed] {
        let (kind, advised) = selected_with_advice(case, Tail::Pending, false, Some("7")).await;
        assert_eq!(kind.as_deref(), Some("\"misalignment_policy_violation\""));
        assert!(advised.is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_provider_refusals_keep_safe_semantic_categories() {
    if isolated("semantic_categories::terminal_provider_refusals_keep_safe_semantic_categories")
        .await
    {
        return;
    }
    codex_alab_core::activate();
    categories(&[
        Case::Quota,
        Case::Usage,
        Case::Budget,
        Case::Audio,
        Case::Model,
        Case::Access,
        Case::Inference,
        Case::ContentPolicy,
        Case::Cyber,
        Case::Bio,
        Case::Misalignment,
        Case::Flex,
    ])
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn advised_capacity_and_rate_failures_keep_safe_semantic_categories() {
    if isolated(
        "semantic_categories::advised_capacity_and_rate_failures_keep_safe_semantic_categories",
    )
    .await
    {
        return;
    }
    codex_alab_core::activate();
    categories(&[Case::RateLimit, Case::Overloaded, Case::PolicyCapacity]).await;
}

#[tokio::test(flavor = "current_thread")]
async fn selected_content_filter_eof_keeps_safe_category() {
    if isolated("semantic_categories::selected_content_filter_eof_keeps_safe_category").await {
        return;
    }
    codex_alab_core::activate();
    let kind = selected(Case::ContentFilter, Tail::Eof, false)
        .await
        .unwrap();
    assert_eq!(kind, "\"content_filter\"");
}

#[tokio::test(flavor = "current_thread")]
async fn completion_supersedes_deferred_content_filter_without_failure_category() {
    if isolated("semantic_categories::completion_supersedes_deferred_content_filter_without_failure_category").await { return; }
    codex_alab_core::activate();
    assert!(
        selected(Case::ContentFilter, Tail::Completed, true)
            .await
            .is_none()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn existing_invalid_prompt_category_and_selected_error_stay_unchanged() {
    if isolated(
        "semantic_categories::existing_invalid_prompt_category_and_selected_error_stay_unchanged",
    )
    .await
    {
        return;
    }
    codex_alab_core::activate();
    categories(&[Case::InvalidPrompt]).await;
}

#[tokio::test(flavor = "current_thread")]
async fn selected_sse_advice_presence_tracks_each_typed_slot() {
    if isolated("semantic_categories::selected_sse_advice_presence_tracks_each_typed_slot").await {
        return;
    }
    codex_alab_core::activate();
    let mut observations = Vec::new();
    for case in [
        Case::Retryable,
        Case::RateLimit,
        Case::Overloaded,
        Case::PolicyCapacity,
    ] {
        for wire in [None, Some("7"), Some("0")] {
            let tail = if matches!(case, Case::Retryable) {
                Tail::Eof
            } else {
                Tail::Pending
            };
            let (kind, advised) = selected_with_advice(case, tail, false, wire).await;
            assert_eq!(kind, Some(format!("{:?}", case.label())));
            observations.push((case, wire, advised));
        }
    }
    // Every row has already retained its original error/advice and passed all
    // correlation/privacy controls. Only then compare the new evidence field.
    for (case, wire, advised) in observations {
        assert_eq!(
            advised.as_deref(),
            Some(if wire.is_some() { "true" } else { "false" }),
            "{case:?}/{wire:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn noncarrying_and_superseded_sse_errors_do_not_claim_advice() {
    if isolated("semantic_categories::noncarrying_and_superseded_sse_errors_do_not_claim_advice")
        .await
    {
        return;
    }
    codex_alab_core::activate();
    let (kind, advised) =
        selected_with_advice(Case::InvalidPrompt, Tail::Pending, false, Some("7")).await;
    assert_eq!(kind.as_deref(), Some("\"invalid_prompt\""));
    let superseded = selected_with_advice(Case::Retryable, Tail::Completed, true, Some("7")).await;
    assert_eq!(superseded, (None, None));
    assert!(
        advised.is_none(),
        "a raw header cannot give this typed variant an advice slot"
    );
}
