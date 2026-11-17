// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A recognized audio refusal owns its outcome before EOF or a later event.

use codex_api::ApiError;
use codex_http_client::ByteStream;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::RequestBody;
use codex_http_client::Response;
use codex_http_client::StreamResponse;
use codex_http_client::TransportError;
use codex_protocol::error::CodexErrorDetails;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

const WAIT: Duration = Duration::from_secs(5);
const CHILD: &str = "ALAB_SSE_AUDIO_REJECTION_CHILD";
const TARGET: &str = "codex_api::endpoint::alab_request_diagnostics";
const PRIVATE: &str = "never-log-private-audio-rejection";
const CODES: [&str; 6] = [
    "audio_too_large",
    "audio_too_short",
    "invalid_audio",
    "invalid_audio_data",
    "invalid_audio_format",
    "missing_audio_sample_rate",
];

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        cx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        cx.span(id).unwrap().extensions_mut().insert(fields);
    }

    fn on_event(&self, event: &tracing::Event<'_>, cx: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        assert_eq!(*event.metadata().level(), tracing::Level::INFO);
        let mut fields = Fields::default();
        event.record(&mut fields);
        for span in cx.event_scope(event).unwrap().from_root() {
            let extensions = span.extensions();
            for (name, value) in &extensions.get::<Fields>().unwrap().0 {
                fields
                    .0
                    .entry(name.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        self.0.lock().unwrap().push(fields.0);
    }
}

#[derive(Clone, Copy, Debug)]
enum Envelope {
    Error,
    Failed,
}

#[derive(Clone, Copy, Debug)]
enum Tail {
    Pending,
    Completed,
    TransportError,
    Eof,
}

#[derive(Default)]
struct Observed {
    sent: Mutex<Vec<Vec<u8>>>,
    tail_polls: AtomicUsize,
    dropped: AtomicBool,
}

struct TailProbe(Arc<Observed>);

impl TailProbe {
    fn polled(&self) {
        self.0.tail_polls.fetch_add(1, Ordering::SeqCst);
    }
}

impl Drop for TailProbe {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::SeqCst);
    }
}

struct GatedTransport {
    failure: Value,
    tail: Tail,
    gate: Mutex<Option<oneshot::Receiver<()>>>,
    observed: Arc<Observed>,
}

fn sse(value: Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}

impl HttpTransport for GatedTransport {
    async fn execute(&self, _: Request) -> Result<Response, TransportError> {
        panic!("Responses uses the streaming transport");
    }

    async fn stream(&self, request: Request) -> Result<StreamResponse, TransportError> {
        assert_eq!(request.method, http::Method::POST);
        assert!(request.url.ends_with("/v1/responses"));
        let Some(RequestBody::EncodedJson(body)) = request.body else {
            panic!("the endpoint sends its encoded input");
        };
        self.observed
            .sent
            .lock()
            .unwrap()
            .push(body.as_bytes().to_vec());
        let gate = self.gate.lock().unwrap().take().expect("one actual send");
        let failure = self.failure.clone();
        let head = futures::stream::once(async move {
            gate.await.expect("release after caller context exits");
            Ok(sse(failure).into())
        });
        let probe = TailProbe(self.observed.clone());
        let tail = self.tail;
        let mut emitted = false;
        let tail: ByteStream = futures::stream::poll_fn(move |_| {
            probe.polled();
            if matches!(tail, Tail::Eof) {
                return Poll::Ready(None);
            }
            if matches!(tail, Tail::Pending) {
                return Poll::Pending;
            }
            if emitted {
                return Poll::Ready(None);
            }
            emitted = true;
            Poll::Ready(Some(match tail {
                Tail::Completed => Ok(sse(json!({
                    "type":"response.completed", "response":{"id":"later-completion"}
                }))
                .into()),
                Tail::TransportError => Err(TransportError::Network(PRIVATE.into())),
                Tail::Pending | Tail::Eof => unreachable!(),
            }))
        })
        .boxed();
        Ok(StreamResponse {
            status: http::StatusCode::OK,
            headers: [
                ("x-request-id", "req_audio-1"),
                ("cf-ray", "ray_audio-1"),
                ("authorization", PRIVATE),
                ("x-unrelated", PRIVATE),
            ]
            .into_iter()
            .map(|(name, value)| (name.parse().unwrap(), value.parse().unwrap()))
            .collect(),
            bytes: head.chain(tail).boxed(),
        })
    }
}

struct NoAuth;

impl codex_api::AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut http::HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

async fn run_case(code: &str, envelope: Envelope, tail: Tail, expect_audio: bool) {
    let detail = json!({"code":code, "message":PRIVATE});
    let failure = match envelope {
        Envelope::Error => json!({"type":"error", "error":detail}),
        Envelope::Failed => json!({"type":"response.failed", "response":{"error":detail}}),
    };
    let (release, gate) = oneshot::channel();
    let observed = Arc::new(Observed::default());
    let client = codex_api::ResponsesClient::new(
        GatedTransport {
            failure,
            tail,
            gate: Mutex::new(Some(gate)),
            observed: observed.clone(),
        },
        codex_api::Provider {
            name: "audio-rejection-fixture".into(),
            base_url: "https://never-log-private-destination.invalid/v1".into(),
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
    let request = json!({
        "model":"gpt-audio-fixture",
        "input":[{"type":"message", "role":"user", "content":[
            {"type":"input_audio", "file_id":"never-log-private-audio-file"}
        ]}]
    });
    let expected = serde_json::to_vec(&request).unwrap();
    let capture = Capture::default();
    let mut stream = {
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        let thread = tracing::info_span!("session_loop", thread_id = "original-audio-thread");
        let _thread = thread.enter();
        let turn = tracing::info_span!(
            "turn",
            thread.id = "original-audio-thread",
            turn.id = "original-audio-turn"
        );
        let _turn = turn.enter();
        tokio::time::timeout(
            WAIT,
            client.stream(
                request,
                http::HeaderMap::new(),
                codex_api::Compression::None,
                None,
            ),
        )
        .await
        .expect("audio endpoint opening deadline")
        .expect("HTTP 200 opens before rejection")
    };
    // The subscriber guard, ancestor guards and span handles are all gone.
    assert_eq!(
        capture.0.lock().unwrap().len(),
        usize::from(codex_alab_core::active())
    );
    assert_eq!(*observed.sent.lock().unwrap(), [expected]);
    assert_eq!(observed.tail_polls.load(Ordering::SeqCst), 0);
    assert!(!observed.dropped.load(Ordering::SeqCst));
    let started = Instant::now();
    release.send(()).unwrap();
    let event = tokio::time::timeout(WAIT, async {
        loop {
            match stream.rx_event.recv().await {
                // The public endpoint emits its opening rate-limit snapshot
                // before consuming SSE. Never skip completion or another event.
                Some(Ok(codex_api::ResponseEvent::RateLimits(_))) => {}
                outcome => break outcome,
            }
        }
    })
    .await
    .expect("selected outcome deadline");
    if expect_audio {
        let error = match event {
            Some(Err(error @ ApiError::InvalidAudioInput { .. })) => error,
            other => panic!(
                "{code}/{envelope:?}/{tail:?}: expected original typed audio rejection, got {other:?}"
            ),
        };
        let ApiError::InvalidAudioInput {
            message,
            per_recording,
        } = &error
        else {
            unreachable!()
        };
        assert_eq!(message, PRIVATE);
        assert_eq!(*per_recording, code != "audio_too_large");
        assert_eq!(Instant::now(), started, "no virtual idle wait");
        let mapped = codex_api::map_api_error(error);
        let CodexErrorDetails::InvalidAudioInput {
            message,
            per_recording,
        } = mapped.details()
        else {
            panic!("expected mapped typed audio rejection");
        };
        assert_eq!(message, PRIVATE);
        assert_eq!(*per_recording, code != "audio_too_large");
        assert_eq!(mapped.to_string(), PRIVATE);
        assert!(
            mapped.retry_delay(1).is_none(),
            "audio validation is not retried"
        );
    } else {
        assert!(
            matches!(event, Some(Ok(codex_api::ResponseEvent::Completed { response_id, .. })) if response_id == "later-completion")
        );
    }
    assert!(
        tokio::time::timeout(WAIT, stream.rx_event.recv())
            .await
            .expect("selected task closes its channel")
            .is_none()
    );
    assert!(observed.dropped.load(Ordering::SeqCst));
    if expect_audio {
        assert_eq!(
            observed.tail_polls.load(Ordering::SeqCst),
            0,
            "refusal ends reading before the contradictory tail"
        );
    } else {
        assert!(observed.tail_polls.load(Ordering::SeqCst) > 0);
    }
    let records = capture.0.lock().unwrap();
    assert_eq!(
        records.len(),
        if codex_alab_core::active() {
            1 + usize::from(expect_audio)
        } else {
            0
        }
    );
    if codex_alab_core::active() {
        let shape = &records[0];
        assert_eq!(shape["message"], "Responses request shape");
        if expect_audio {
            let failed = &records[1];
            assert_eq!(failed["message"], "Responses request failed");
            assert_eq!(failed["local_attempt"], shape["local_attempt"]);
            assert_eq!(failed["request_id"], "\"req_audio-1\"");
            assert_eq!(failed["cf_ray"], "\"ray_audio-1\"");
            assert!(
                !failed.contains_key("http_status"),
                "opening 200 is not the error status"
            );
        }
        for record in records.iter() {
            assert_eq!(record["thread_id"], "\"original-audio-thread\"");
            assert_eq!(record["thread.id"], "\"original-audio-thread\"");
            assert_eq!(record["turn.id"], "\"original-audio-turn\"");
        }
    }
    let text = format!("{records:?}");
    for record in records.iter() {
        for field in [
            "code",
            "error_code",
            "error_message",
            "error.message",
            "body",
            "payload",
        ] {
            assert!(
                !record.contains_key(field),
                "provider payload fields are not logged"
            );
        }
    }
    for private in ["never-log", "authorization", "x-unrelated"] {
        assert!(!text.contains(private), "metadata excludes private content");
    }
}

async fn isolated(name: &str) -> bool {
    if let Some(child) = std::env::var_os(CHILD) {
        assert_eq!(child.to_str(), Some(name));
        tokio::time::pause();
        return false;
    }
    let exact = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &exact, "--nocapture"])
        .env(CHILD, name);
    let report = crate::test_child_process::run(command, Duration::from_secs(30))
        .await
        .unwrap_or_else(|failure| panic!("isolated audio child: {failure}"));
    report.assert_test_success(&exact);
    true
}

async fn audio_cases(tail: Tail) {
    codex_alab_core::activate();
    for code in CODES {
        for envelope in [Envelope::Error, Envelope::Failed] {
            run_case(code, envelope, tail, true).await;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn audio_rejections_beat_idle_timeout_without_losing_context() {
    if !isolated("audio_rejections_beat_idle_timeout_without_losing_context").await {
        audio_cases(Tail::Pending).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn audio_rejections_beat_later_completion_without_losing_context() {
    if !isolated("audio_rejections_beat_later_completion_without_losing_context").await {
        audio_cases(Tail::Completed).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn audio_rejections_beat_later_transport_failure_without_losing_context() {
    if !isolated("audio_rejections_beat_later_transport_failure_without_losing_context").await {
        audio_cases(Tail::TransportError).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_sse_failure_can_still_be_superseded_by_completion() {
    if !isolated("unknown_sse_failure_can_still_be_superseded_by_completion").await {
        codex_alab_core::activate();
        for envelope in [Envelope::Error, Envelope::Failed] {
            run_case("unknown_failure", envelope, Tail::Completed, false).await;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn inactive_audio_failure_can_still_be_superseded_by_completion() {
    if !isolated("inactive_audio_failure_can_still_be_superseded_by_completion").await {
        assert!(!codex_alab_core::active());
        for code in CODES {
            for envelope in [Envelope::Error, Envelope::Failed] {
                run_case(code, envelope, Tail::Completed, false).await;
            }
        }
    }
}

#[path = "sse_semantic_diagnostic_tests.rs"]
mod semantic_categories;
