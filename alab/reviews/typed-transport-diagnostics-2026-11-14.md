<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve typed transport distinctions in selected failure evidence

The diagnostic distinguishes HTTP errors but labels every other typed
`TransportError` as `other`. The public endpoint still returns those variants
intact, and the API bridge maps timeout, connection and network-policy failures
to different recovery outcomes. The default local evidence loses that useful
distinction even though no message parsing is needed to retain it.

## Decision and reason

Match the transport enum exhaustively and emit fixed labels: `network_policy`,
`timeout`, `connection`, `network`, `request_build`, `retry_limit` and
`response_too_large`. Preserve the HTTP classification, status, safe IDs and
opaque-envelope observation. Leave all error values, retries, telemetry,
subscriber/span context and other API classifications unchanged. Do not format
errors, traverse their sources, copy payloads or add fields.

These labels describe types available at the selected diagnostic boundary.
Established SSE/WebSocket paths that already flattened an error into `Stream`
remain `stream`; do not guess a subtype from private text. Normal Responses
retry exhaustion returns its original final error, and the current endpoint
does not enable a response-body size cap. The two defensive enum labels do not
claim that `RetryLimit` or `ResponseTooLarge` normally occurs on this path.

## Reproduction and validation

Committed RED source `14086a403b3bdfaed4f64ee7e365d21e50da4cad` passes the existing
HTTP retry baseline and fails all four new public endpoint controls at their
final classification assertion: `other` instead of `timeout`, `network`,
`network_policy` or `request_build`. The original typed errors, retry/send
counts, encoded input/tool pair, context and privacy assertions pass first.

The timeout case includes an earlier transient network failure and verifies one
prepared request, two transport attempts and only the selected terminal failure.
The policy/build cases use the real auth interface, enable transport retries,
and require one credential application and no transport call. The network case
retains its original private message while excluding it from the diagnostic.
Exact isolated children have bounded waits and retained termination/reap handles.

Fixed source `1d64cc96d21fab4ac3fab1c2957f872376c4c175` passes 18 API contract tests and 258 API
unit tests. Both crates pass all-target scoped Tippy with zero ALab diagnostics.
The common Git validation archive retains the logs, source diff and hashes.
These are runtime unit checks, not application certification. Source review found no
reason to broaden the opaque classifier or change the explicitly tested special
WebSocket code precedence. The complete configured Python sweep remains
separately deferred for native adapter memory admission. Application delivery
remains deferred at the recorded
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
