// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Controlled body polls prove delivered progress, independent of TCP segmentation.

use super::*;
use crate::DestinationPolicy;
use crate::NetworkPolicy;
use crate::NetworkPolicyController;
use crate::NetworkPolicyDenied;
use futures::Stream;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use tokio::sync::oneshot;
use tracing_subscriber::layer::SubscriberExt;

const PRIVATE: &[u8] = b"never-log-progress-body";
const TARGET: &str = "codex_http_client::response::alab_error_body_capture";

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        assert!(
            matches!(
                field.name(),
                "message"
                    | "phase"
                    | "http_status"
                    | "body_capture"
                    | "body_wait_ms"
                    | "rejection_body_bytes"
                    | "rejection_body_bytes_read"
                    | "retry_advised"
                    | "request_id"
                    | "cf_ray"
            ),
            "unexpected field name: {}",
            field.name()
        );
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Fields>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() == TARGET {
            let mut fields = Fields::default();
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields);
        }
    }
}

impl Capture {
    fn take(&self, outcome: &str, complete: Option<usize>) -> Fields {
        let mut events = self.0.lock().unwrap();
        assert_eq!(events.len(), 1, "one actual completed capture");
        let fields = events.pop().unwrap();
        assert_eq!(fields.0["message"], "HTTP rejection body capture finished");
        assert_eq!(fields.0["phase"], "\"rejection_body\"");
        assert_eq!(fields.0["http_status"], "400");
        assert_eq!(fields.0["body_capture"], format!("{outcome:?}"));
        assert_eq!(fields.0["retry_advised"], "true");
        assert_eq!(number(&fields, "rejection_body_bytes"), complete);
        assert!(
            fields
                .0
                .values()
                .all(|value| !value.contains(std::str::from_utf8(PRIVATE).unwrap()))
        );
        fields
    }
}

fn number(fields: &Fields, name: &str) -> Option<usize> {
    fields
        .0
        .get(name)
        .map(|value| value.parse().expect("numeric count"))
}

fn assert_read(fields: &Fields, expected: Option<usize>, label: &str) {
    println!("ALAB_HTTP_BODY_PROGRESS:case={label}:stage=late_count");
    assert_eq!(
        number(fields, "rejection_body_bytes_read"),
        expected,
        "actual delivered entity progress"
    );
    println!("ALAB_HTTP_BODY_PROGRESS:case={label}:stage=complete");
}

fn response(body: reqwest::Body, policy: &NetworkPolicy) -> HttpResponse {
    let response = http::Response::builder()
        .status(400)
        .header("x-request-id", "req_progress-1")
        .header("authorization", std::str::from_utf8(PRIVATE).unwrap())
        .body(body)
        .unwrap();
    let response = reqwest::Response::from(response);
    let permit = policy.acquire(response.url()).unwrap();
    HttpResponse::new(response, permit)
}

async fn read(response: HttpResponse) -> Result<Bytes, HttpError> {
    capture(
        response,
        RetryAfter::from_delay(Duration::from_secs(7)),
        |body, _| std::future::ready(Ok(body)),
    )
    .await
}

struct Held {
    first: Option<Bytes>,
    entered: Option<oneshot::Sender<()>>,
    dropped: Arc<AtomicBool>,
}

impl Stream for Held {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some(first) = this.first.take() {
            return Poll::Ready(Some(Ok(first)));
        }
        // This poll can occur only after the previous returned chunk passed
        // through collect_complete. It is stronger than a server-write signal.
        if let Some(entered) = this.entered.take() {
            entered.send(()).expect("owning observer is still present");
        }
        Poll::Pending
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

fn held(prefix: Option<Bytes>) -> (reqwest::Body, oneshot::Receiver<()>, Arc<AtomicBool>) {
    let (entered, receiver) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let body = reqwest::Body::wrap_stream(Held {
        first: prefix,
        entered: Some(entered),
        dropped: dropped.clone(),
    });
    (body, receiver, dropped)
}

async fn entered(future: Pin<&mut impl Future>, receiver: oneshot::Receiver<()>) {
    tokio::select! {
        biased;
        result = receiver => result.expect("collector reached its next pending body poll"),
        _ = future => panic!("held capture completed before its body-poll acknowledgement"),
    }
}

async fn healthy(capture: &Capture) {
    let result = read(response(PRIVATE.into(), &NetworkPolicy::unmanaged())).await;
    assert_eq!(result.unwrap().as_ref(), PRIVATE);
    capture.take("complete", Some(PRIVATE.len()));
    println!("ALAB_HTTP_BODY_PROGRESS:case=healthy:stage=complete");
}

#[tokio::test(start_paused = true)]
async fn read_progress_survives_body_timeout_and_counts_complete_entities() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let _subscriber = tracing::subscriber::set_default(subscriber);
    healthy(&capture).await;
    for prefix in [Some(Bytes::from_static(PRIVATE)), None] {
        let length = prefix.as_ref().map_or(0, Bytes::len);
        let (body, receiver, dropped) = held(prefix);
        let mut future = Box::pin(read(response(body, &NetworkPolicy::unmanaged())));
        entered(future.as_mut(), receiver).await;
        assert!(futures::poll!(&mut future).is_pending());
        tokio::time::advance(BODY_WAIT + Duration::from_millis(1)).await;
        assert!(matches!((&mut future).await, Err(HttpError::Timeout)));
        drop(future);
        assert!(
            dropped.load(Ordering::SeqCst),
            "timeout retires the real body owner"
        );
        let fields = capture.take("timed_out", None);
        assert_read(&fields, Some(length), "timeout");
    }

    for body in [Bytes::new(), Bytes::from_static(PRIVATE)] {
        let length = body.len();
        assert_eq!(
            read(response(body.into(), &NetworkPolicy::unmanaged()))
                .await
                .unwrap()
                .len(),
            length
        );
        assert_read(
            &capture.take("complete", Some(length)),
            Some(length),
            "complete",
        );
    }
    let body = reqwest::Body::wrap_stream(futures::stream::iter([
        Ok(Bytes::from_static(PRIVATE)),
        Err(std::io::Error::other("controlled read failure")),
    ]));
    assert!(matches!(
        read(response(body, &NetworkPolicy::unmanaged())).await,
        Err(HttpError::Build(_))
    ));
    assert_read(
        &capture.take("read_error", None),
        Some(PRIVATE.len()),
        "read-error",
    );

    let bytes = Bytes::from(vec![b'x'; MAX_BODY + 1]);
    let body = reqwest::Body::wrap_stream(futures::stream::iter([Ok::<_, std::io::Error>(bytes)]));
    assert!(matches!(
        read(response(body, &NetworkPolicy::unmanaged())).await,
        Err(HttpError::Build(_))
    ));
    assert_read(
        &capture.take("too_large", None),
        Some(MAX_BODY + 1),
        "streamed-overflow",
    );

    let body = reqwest::Body::from(vec![b'x'; MAX_BODY + 1]);
    assert!(matches!(
        read(response(body, &NetworkPolicy::unmanaged())).await,
        Err(HttpError::Build(_))
    ));
    assert_read(&capture.take("too_large", None), None, "declared-overflow");
}

#[tokio::test(start_paused = true)]
async fn read_progress_survives_policy_denial_without_logging_owner_drop() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let _subscriber = tracing::subscriber::set_default(subscriber);
    healthy(&capture).await;
    let controller = NetworkPolicyController::default();
    let policy = controller.policy();
    assert!(controller.publish(policy.revision(), DestinationPolicy::Unrestricted));
    let (body, receiver, dropped) = held(Some(Bytes::from_static(PRIVATE)));
    let mut future = Box::pin(read(response(body, &policy)));
    entered(future.as_mut(), receiver).await;
    assert!(futures::poll!(&mut future).is_pending());
    policy.invalidate();
    assert!(matches!(
        (&mut future).await,
        Err(HttpError::Policy(NetworkPolicyDenied::Revoked))
    ));
    drop(future);
    assert!(
        dropped.load(Ordering::SeqCst),
        "revocation retires the real body owner"
    );
    assert_read(
        &capture.take("policy_denied", None),
        Some(PRIVATE.len()),
        "revoked-after-read",
    );

    assert!(controller.publish(policy.revision(), DestinationPolicy::Unrestricted));
    let (body, _receiver, dropped) = held(Some(Bytes::from_static(PRIVATE)));
    let denied_response = response(body, &policy);
    policy.invalidate();
    assert!(matches!(
        read(denied_response).await,
        Err(HttpError::Policy(NetworkPolicyDenied::Revoked))
    ));
    assert!(dropped.load(Ordering::SeqCst));
    assert_read(
        &capture.take("policy_denied", None),
        None,
        "revoked-before-read",
    );

    let (body, receiver, dropped) = held(Some(Bytes::from_static(PRIVATE)));
    let mut future = Box::pin(read(response(body, &NetworkPolicy::unmanaged())));
    entered(future.as_mut(), receiver).await;
    assert!(futures::poll!(&mut future).is_pending());
    drop(future);
    assert!(
        dropped.load(Ordering::SeqCst),
        "dropping the owner retires the real body"
    );
    assert!(
        capture.0.lock().unwrap().is_empty(),
        "owner drop creates no completed phase"
    );
}
