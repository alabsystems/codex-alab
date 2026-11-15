<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve terminal audio rejection through SSE delivery

Both statusless `error` and `response.failed` events already parse six exact
provider audio codes into `InvalidAudioInput`. The SSE loop omits that type from
its immediate-delivery lists. Pending bytes replace it with a retryable idle
`Stream`; a later completion hides it as success, and later transport failure
replaces it. Existing audio tests validate the parser directly or close the
stream after the failure, so they do not expose this delivery gap.

## Decision and reason

Add `InvalidAudioInput` to the existing ALab-owned active immediate-delivery
predicate. Use the existing selected-error telemetry, channel send and return;
do not add another parser, task, queue, retry rule or upstream hook. This ends
reading before a contradictory tail can change the recognized failure. The
same type already maps to a nonretryable protocol error, supports rejected-audio
attribution, and selects remove/replace attachment guidance in core.

Preserve the original message and `per_recording`: `audio_too_large` is false;
`audio_too_short`, `invalid_audio`, `invalid_audio_data`, `invalid_audio_format`
and `missing_audio_sample_rate` are true. The parser, HTTP status handling,
WebSocket delivery and inactive mode remain unchanged. Unknown errors retain
existing buffering and completion supersession. This is an independent typed
failure defect, not an explanation of the original opaque-400 backend cause.

The inspected local `upstream/main` reference also omits audio from its immediate
list. Drop this owned addition when an integrated upstream version delivers the
same type immediately. Existing `failure_kind` labeling is a separate concern;
this change preserves the selected typed error without parsing private text.

## Reproduction and checks

Corrected RED source `7ddcf9e29cd1b2e04677b169078cccdd94f52a89` passes unknown/inactive buffering
controls and fails three public `ResponsesClient` regressions at semantic outcome
selection: open body produces idle `Stream`, completion produces success, and
transport failure produces generic `Stream`. The first code/envelope is
`audio_too_large` in a statusless error; later matrix cases become reachable only
after delivery is fixed. The preceding opening/send/gate assertions pass.
The initial fixture selected opening rate-limit metadata incorrectly; its failed
run is retained separately and does not count as a behavioral reproduction.

The fixture drives the real endpoint and SSE loop with a gated in-memory HTTP200
transport, covering all six codes, both envelopes and all three tails. It retains
exact original serialized input, one send, message/attachment scope, nonretryable
mapping, no virtual idle wait, body drop without polling the tail, selected
failure/context/IDs and private-content exclusion after caller spans exit.
The parent uses real bounded child wait/drains and kill/reap; only children pause
Tokio time. Unknown-error completion and all inactive audio completions remain
controls. The existing owned predicate matrix includes both attachment scopes.

Fixed source `89132ddea6725984b0ba05a36ea633bb1f0d08e6` passes all 29 public API
tests and 260 API units (289 total), including all 36 active audio matrix cases.
Both crates pass all-target scoped Tippy with zero ALab diagnostics. The common
Git validation archive retains the exact-source logs, diff and hashes privately.
The earlier full Python
workflow sweep passed 1,003 tests with eight skips; no Python workflow or native
adapter code changes here. The delivery prerequisite refresh still finds no
supported source-selected macro admission. Application certification and
installation remain deferred at the recorded
[Targo boundary](targo-macro-admission-2026-11-14.md).
