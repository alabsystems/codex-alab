<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retain selected WebSocket refusal correlation — 2026-11-15

Decision: retain two validated server correlation tokens per request, and use
them only when the actual selected WebSocket error is InvalidRequest. Reuse
the existing factory hook and telemetry interface. Do not change error values,
classification, provider advice, timing or recovery policy.

The standalone refusal repair preserves the diagnosis, but a statusless frame's
headers do not survive on ApiError::InvalidRequest. The final diagnostic therefore
loses supplied request_id and cf_ray. Wrapped HTTP errors already retain those
IDs on their transport error. This is an evidenced debugging gap, not a newly
established explanation of the original opaque backend rejection.

The existing hook now obtains the diagnostic and a request-local telemetry
wrapper together. Inactive mode returns the original optional observer; there
is no correlation allocation. Active state retains at most two accepted strings.
Every received text callback replaces its candidate, including absent, invalid,
malformed and unrelated text. Only error and response.failed metadata containers
are decoded. Use the existing header converter and bounded token validator;
outer primary/alias precedes nested equivalents and cf-ray falls back separately.
Wrong-type optional outer/nested headers do not erase usable IDs in the other.

The wrapper forwards all three telemetry callbacks with their original arguments
under the invocation's subscriber/span. Locks are released before delegating or
emitting a diagnostic. A poisoned lock omits metadata while preserving callbacks
and the error. No provider-error parser, Retry-After parser or shared wake-clock
sampling is added by the observer. HTTP/SSE diagnostic state remains unchanged.

Final outcome selection authorizes the fallback: InvalidRequest can use its
candidate, while transport errors retain their own header ownership. Completion,
stream errors, cancellation and other outcomes ignore the candidate. Current
selected-error paths produce InvalidRequest only from the current sequential
text parser. This identifies the selected received frame; it does not resolve
ownership of a late provider frame. Clearing is per text callback, not a claim
about every wire frame. A new request has distinct state.

Only one existing upstream hook line changes. New behavior and tests remain in
ALab-owned glue and contracts, with no new dependency, protocol or telemetry-trait
method. Broader standalone quota/rate classification was inspected but is not
part of this topic: it couples to malformed WebSocket status recovery authority.
The chosen logging change does not widen that authority.

Tests-only source f654deb0c3a835012c2896a65f2d6ca1832f1651 reproduces three
missing-request_id failures after semantic, cleanup, context and privacy checks.
Of 326 outer methods, 323 pass, three fail and none are ignored. Nested child
summaries are not additional outer methods. Invalid-ID omission passes; ignored
IDs followed by no-ID refusal and completion pass before the current-B case fails.
RED stops at the first request_id mismatch in each method: its cf_ray check and
later precedence rows are GREEN coverage, not independently executed RED claims.

The first implementation run at 5f6022adabfa31bc5d77f8ea8ccd1991323922a3 passes
all 66 public contracts, including safe IDs and stale-frame controls. One of the
266 API units fails because its fixture saved a stack address before moving the
error into a closure. Preserve that failed run; Tippy was not reached. Fix only
the fixture with stable Box storage, retaining the strict forwarded-reference
identity assertion and moving the original error into the final outcome.

At 38d3b783fb7aa073c252123b0f39116134f8e59c all 332 methods pass, including
callback argument/context/lock checks. The lint gate still fails on the new test
enum's MixedCase variant name. Preserve that run separately. Rename the private
fixture variant MixedHeaders; do not suppress the finding or change production.

Final runtime source 5b2ed43d1ddfd4256d4365a755de60fe85f03093 passes 66 public
contracts plus 266 API units: 332 passed, zero failed/ignored/measured/filtered.
All four new public and six owned observer identities pass exactly once.
Scoped all-target Tippy succeeds with zero owned, spanning, failed-target,
missing-script or set-aside findings. Independent review checks native library
and runner bindings, current binary snapshots, both normal/test profiles and
657 canonical artifacts within the separate Tippy target. Earlier failed
attempts retain their actual sources and statuses; only the final run passes
both runtime and lint gates.

Real loopback tests observe the first outgoing response.create, release scripted
frames after the caller's subscriber and parent span handles exit, and join the
peer before checking correlation last. They require exact refusal/message,
no retry/advice, closed channel, one shape plus one selected failure, matching
attempt/context and exclusion of payload/arbitrary-header/stale-ID content.
Literal expected IDs cover primary/alias precedence, mixed header spelling,
wrong-type containers, invalid/overlong/punctuation rejection, ignored-A to
no-ID/current-B refusal, and ignored-A to completion without a failure record.
The fixture's existing one-frame entry delegates to bounded frame-sequence
support; all scripted sequences stop at their selected outcome.

Six owned behavior units cover response.failed extraction, inactive wrapper
identity, callback arguments/order/context and unlocked delegation, validator
precedence, reset and distinct request state, completion/other-error selection,
transport-header priority and poisoning. Request-state isolation is unit/static
coverage; these new tests do not execute two turns on a reused physical socket.
First-request observation is not a complete wire inventory. Safe IDs remain
request metadata, not evidence of backend acceptance or the original cause.

Both API packages stay selected for RED and GREEN under source-selected Trust
15698, the existing qualified project frontend/protected profile and native unit
runner, in the explicit unverified runtime lane. Independent source and artifact
reviews find no remaining blocker. Private exact-source evidence and corrections
are retained under shared Git alab-validation/websocket-semantic-correlation-20261115.
The diagnostics guide records the new fallback and its limits.

No Core/Autopilot actor, application certificate, new package or installation is
claimed by this topic. Earlier full actor and managed-runtime evidence keep their
own source identities. The recorded application-certification limitation remains
separate and its publication gate is preserved. The original backend reason
remains unknown; no history or credentials are changed.
