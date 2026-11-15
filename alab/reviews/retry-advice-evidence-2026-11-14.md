<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Retain typed retry-advice presence in selected request evidence

Selected failure INFO records have original request/thread/turn context, a local
attempt and safe server IDs. They do not retain whether the selected error had
typed provider advice. Later core logs describe chosen waits; transient retries
or recovered successes need not become durable Autopilot walls, and current
records update or clear. A recovery wall can retain a provider deadline for its
current failed turn, but does not reconstruct every earlier request's origin.

## Decision and reason

Add only optional boolean `retry_advised` to the existing selected-failure event.
Mirror the five advice-bearing cases in the API bridge: `Transport(Http)`,
`Retryable`, `RateLimitExceeded`, `ServerOverloaded` and
`PolicyCapacityUnavailable`. For those types, `Some` is true and `None` is false;
other representations omit the field. Record a primitive optional boolean,
not a formatted Option or error.

Zero or expired advice remains a retained value and is true. False means the
typed slot is empty, not that the server sent no raw header. Omission is unknown
at that representation; even a terminal semantic refusal may have discarded
wire retry metadata. This flag authorizes no replay and claims neither expiry
nor a positive/chosen delay. Preserve original error values, advice, mapping,
selection, IDs and context.

Do not add remaining duration or deadline sampling. `RetryAfter.deadline()` and
`remaining_delay()` use the shared wake reader, which updates process wake
observations. The evidence helper only matches variants and calls `is_some`;
it does not read, recapture or arm clocks, parse headers/body, format private
text, inspect error sources or change policy. Existing core and persisted
recovery timing remains authoritative.

## Reproduction and validation

RED source `d951c0a407865366aeb1f6a9cdc15b41f8ddaf90` reaches every represented table row's
original error/advice/map/context/privacy checks, then fails the HTTP and SSE
groups only because the new field is missing instead of false. The noncarrying
and superseded-completion control passes. Cargo stops after the public package
failure; no later package execution is claimed for that RED invocation.

Public tests reuse the existing HTTP and semantic SSE fixtures. They cover all
five carrying types with advice and absence, zero advice, genuinely expired
retained advice, and a valid raw HTTP header with a contradictory typed None.
An invalid-prompt semantic type with raw retry metadata must omit the field;
a generic advised failure superseded by completion must emit no failure. All
rows preserve actual serialized input/send counts, original values and mapping,
telemetry, selected event count, original span/attempt/server IDs and private
content exclusion before checking the new field. Only isolated children pause
time; parent wait/drain and kill/reap bounds are unchanged.

Independent source review confirms that the helper is read-only and the existing
classification/status/opaque/header/context and delivery paths are unchanged.
Fixed source `28a4eccb8e17d5ef8a801de422f32644ca5c0318` passes all 37 public API
tests and 260 API units (297 total). Both crates pass scoped all-target Tippy
with zero ALab diagnostics. The private common Git validation archive retains
exact-source logs, diff and hashes. Runtime uses the same combined API package
scope; all lint artifact paths were verified under the separate `target/tippy`. No Python workflow code,
native adapter or dependency/pin changes here; the prior configured Python sweep
remains 1,003 tests with eight skips, not a new-source rerun.

Application delivery remains deferred at the confirmed
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md). These are
source/runtime checks, not a qualified package or running-process replacement.
