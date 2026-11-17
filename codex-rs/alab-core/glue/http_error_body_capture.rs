// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bound unconfigured HTTP rejection entities under their original network permit.
//! Buffered and streaming errors retain their original decoding and HTTP authority.

use super::HttpResponse;
use crate::HttpError;
use crate::RetryAfter;
use crate::TransportError;
use bytes::Bytes;
use bytes::BytesMut;
use http::HeaderMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::Instant;

const MAX_BODY: usize = 64 * 1024;
const BODY_WAIT: Duration = Duration::from_secs(5);

impl HttpResponse {
    // Erase source-aware futures at the same Send boundary as ordinary text().
    pub(crate) fn alab_error_text(
        self,
        retry_after: Option<RetryAfter>,
    ) -> Pin<Box<dyn Future<Output = Result<String, HttpError>> + Send + 'static>> {
        Box::pin(async move {
            if !codex_alab_core::active() {
                return self.text().await;
            }
            capture(self, retry_after, |body, headers| async move {
                let mut buffered = http::Response::new(body);
                *buffered.headers_mut() = headers;
                reqwest::Response::from(buffered)
                    .text()
                    .await
                    .map_err(HttpError::from)
            })
            .await
        })
    }

    pub(crate) fn alab_buffered_bytes(
        self,
        url: String,
        retry_after: Option<RetryAfter>,
        map_error: fn(HttpError) -> TransportError,
    ) -> Pin<Box<dyn Future<Output = Result<Bytes, TransportError>> + Send + 'static>> {
        Box::pin(async move {
            if !codex_alab_core::active() || self.inner.status().is_success() {
                return self.bytes().await.map_err(map_error);
            }
            let status = self.inner.status();
            let headers = self.inner.headers().clone();
            match capture(self, retry_after, |body, _| std::future::ready(Ok(body))).await {
                // The caller retains its original strict UTF-8 conversion.
                Ok(body) => Ok(body),
                Err(error @ HttpError::Policy(_)) => Err(map_error(error)),
                Err(_) => Err(TransportError::Http {
                    status,
                    url: Some(url),
                    headers: Some(headers),
                    body: None,
                    retry_after,
                }),
            }
        })
    }
}

async fn capture<T, Decode, Decoded>(
    response: HttpResponse,
    retry_after: Option<RetryAfter>,
    decode: Decode,
) -> Result<T, HttpError>
where
    T: Send + 'static,
    Decode: FnOnce(Bytes, HeaderMap) -> Decoded + Send + 'static,
    Decoded: Future<Output = Result<T, HttpError>> + Send + 'static,
{
    let HttpResponse { inner, permit } = response;
    let status = inner.status();
    let request_id = safe_id(inner.headers(), "x-request-id")
        .or_else(|| safe_id(inner.headers(), "x-oai-request-id"))
        .map(str::to_owned);
    let cf_ray = safe_id(inner.headers(), "cf-ray").map(str::to_owned);
    let started = Instant::now();
    let deadline = started + BODY_WAIT;
    let mut body_bytes_read = None;

    // Even a ready declared-size rejection must pass original source-use
    // preparation and the original permit's biased revocation race.
    let captured = permit
        .run(async {
            tokio::select! {
                biased;
                // The timer remains inside source admission and revocation.
                _ = tokio::time::sleep_until(deadline) => {
                    Captured::unavailable(Outcome::TimedOut)
                }
                captured = collect_complete(inner, decode, &mut body_bytes_read) => captured,
            }
        })
        .await;
    let (outcome, body_bytes, result) = match captured {
        Ok(captured) => (captured.outcome, captured.body_bytes, captured.result),
        Err(denied) => (Outcome::PolicyDenied, None, Err(HttpError::Policy(denied))),
    };
    let body_wait_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        target: "codex_http_client::response::alab_error_body_capture",
        phase = "rejection_body",
        http_status = status.as_u16(),
        body_capture = outcome.as_str(),
        body_wait_ms,
        rejection_body_bytes = body_bytes,
        rejection_body_bytes_read = body_bytes_read,
        retry_advised = retry_after.is_some(),
        request_id = request_id.as_deref(),
        cf_ray = cf_ray.as_deref(),
        "HTTP rejection body capture finished"
    );
    result
}

#[derive(Clone, Copy)]
enum Outcome {
    Complete,
    TooLarge,
    TimedOut,
    ReadError,
    PolicyDenied,
}

impl Outcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::TooLarge => "too_large",
            Self::TimedOut => "timed_out",
            Self::ReadError => "read_error",
            Self::PolicyDenied => "policy_denied",
        }
    }
}

struct Captured<T> {
    outcome: Outcome,
    body_bytes: Option<usize>,
    result: Result<T, HttpError>,
}

impl<T> Captured<T> {
    fn unavailable(outcome: Outcome) -> Self {
        Self {
            outcome,
            body_bytes: None,
            // Each wrapper consumes this static diagnostic failure while
            // preserving the original known HTTP rejection.
            result: Err(match outcome {
                Outcome::TimedOut => HttpError::Timeout,
                _ => HttpError::Build("HTTP rejection body unavailable".to_owned()),
            }),
        }
    }
}

async fn collect_complete<T, Decode, Decoded>(
    mut response: reqwest::Response,
    decode: Decode,
    body_bytes_read: &mut Option<usize>,
) -> Captured<T>
where
    Decode: FnOnce(Bytes, HeaderMap) -> Decoded,
    Decoded: Future<Output = Result<T, HttpError>>,
{
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Captured::unavailable(Outcome::TooLarge);
    }

    // Zero is measured only after the size precheck enters chunk collection.
    // This borrowed observation outlives cancellation by the original permit or timer.
    *body_bytes_read = Some(0);

    // Reqwest has already removed transfer/content coding. Bound retained entity
    // bytes before charset/BOM decoding, never just each individual chunk.
    let mut body = BytesMut::new();
    let mut chunks_since_yield = 0;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                // Include the whole returned chunk even when it cannot be retained.
                *body_bytes_read = body.len().checked_add(chunk.len());
                if chunk.len() > MAX_BODY - body.len() {
                    return Captured::unavailable(Outcome::TooLarge);
                }
                body.extend_from_slice(&chunk);
                chunks_since_yield += 1;
                if chunks_since_yield == 16 {
                    chunks_since_yield = 0;
                    // Ready empty chunks do not consume the byte budget. Yield
                    // cooperatively so outer deadline/revocation polls still run.
                    tokio::task::yield_now().await;
                }
            }
            Ok(None) => break,
            Err(error) if error.is_timeout() => {
                return Captured::unavailable(Outcome::TimedOut);
            }
            Err(_) => return Captured::unavailable(Outcome::ReadError),
        }
    }

    // A complete framing end is required even when the retained bytes happen to
    // form the opaque 400 envelope. Never promote an incomplete prefix to Some.
    let body_bytes = body.len();
    let headers = std::mem::take(response.headers_mut());
    match decode(body.freeze(), headers).await {
        Ok(body) => Captured {
            outcome: Outcome::Complete,
            body_bytes: Some(body_bytes),
            result: Ok(body),
        },
        Err(_) => Captured::unavailable(Outcome::ReadError),
    }
}

fn safe_id<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    codex_alab_core::diagnostic_id::first_safe(
        headers
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok()),
    )
}

#[cfg(test)]
#[path = "http_error_body_progress_tests.rs"]
mod progress_tests;
