<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retain selected invalid-prompt WebSocket correlation — 2026-11-15

Decision: include InvalidPrompt beside InvalidRequest in the selected WebSocket
outcome diagnostic match, and isolate optional nested metadata decoding so an
unusable nested shape cannot erase valid outer IDs. Keep token validation,
persistent state, callbacks and error values unchanged.

InvalidPrompt survives both standalone error and response.failed parsing, but
its final diagnostic currently drops supplied safe request_id/cf_ray. The
observer already records bounded validated metadata for both envelopes. Reusing
that state closes a logging gap without changing classification, advice or
recovery. The existing protocol/actor layers remain unchanged.

A second source-backed gap occurs with response.failed's six-field positional
error array. The upstream full Error decoder accepts this as a typed refusal,
while the one-field metadata ErrorHeaders decoder rejects it and discards valid
outer IDs. Preserve outer decoding independently. Borrow optional error/response
raw slices, require named objects for nested metadata, and decode only headers.
Never interpret error-array positions as header positions. InvalidRequest can
reproduce this loss independently of new InvalidPrompt logging eligibility.

Separate metadata passes over borrowed slices cost additional scans, while
avoiding provider-message allocation or retention. No full error/deadline parser
or clock read is added; raw views stay within extraction, and persistent state
remains at most two validated strings. No throughput benchmark is claimed.

The current selected InvalidPrompt paths originate only in the sequential text
parser, after that frame's telemetry callback and before reading another frame.
Pre-send, socket acquisition, receive and interruption failures use other API
variants; completion preservation returns Ok. The same current-frame selection
proof applies to InvalidRequest and InvalidPrompt. This does not establish
ownership of a late provider frame or two turns on one reused physical socket.

Valid wrapped HTTP errors keep their own headers and status before semantic
parsing. A semantic prompt refusal gains only bounded safe metadata, without an
HTTP/authentication inference. Completion and other error types ignore the
candidate. Existing per-text reset, distinct request state, poisoning and
inactive behavior remain unchanged; no new deadline parse or clock read is added.

Tests-only source 94e116d7c4fa3e7c0183ccec57d28af53ae3d20f reproduces
three missing-ID failures: two public prompt envelopes and the selected-outcome
unit. Of339 outer methods,336 pass and three fail, with zero ignored. Both
public failures occur at the first Nested request_id assertion, after exact
semantic error/message, Never/no advice, context, privacy and channel/peer cleanup.
Its cf_ray assertion and later matrix rows are GREEN coverage.

Expanded tests-only source 53b6590e1c98d8c185b76796b3db30c64bcc3af1 passes
336 outer methods and fails seven, with zero ignored. Both new positional public
cases reach missing outer request_id after the same earlier assertions. The
InvalidRequest case isolates the metadata-reader defect independently of new
Prompt logging eligibility. The nested-array unit fails on the first six-field
array with outer IDs; the optional-response unit fails on response42. Later
unit shapes are GREEN coverage. Child failure summaries are not extra methods.

GREEN source e3fb8162683aa9ab2f2eedfa41ae36ba88b5a18f passes75 public contracts
plus268 API units:343 passed, zero failed/ignored/measured/filtered. All four
added public and three required owned unit identities pass exactly once; the
selected-outcome unit is renamed/extended, while the other two units are new.
The original four public correlation method bodies stay unchanged. Scoped
all-target Tippy succeeds with zero owned, spanning, failed-target, missing-script
or set-aside findings.

Two strict public child runs execute15 scripted cases per prompt envelope:
14 selected refusals and one completion control each. Together with two isolated
positional refusal cases, this is32 scripted exchanges, not32 refusal outcomes.
Coverage includes nested/outer/alias/mixed-case precedence, unusable optional
headers, invalid/overlong/punctuation omission, and ignored-A to no-ID/current-B
refusal or completion. Each case observes its first outgoing response.create,
releases received frames after the caller context exits, and joins the bounded
peer before the last ID assertions. This is not a full wire inventory.

Metadata units additionally reject positional header inference in nested error
and whole response arrays while preserving valid outer tokens. The whole-response
array's decisive no-outer control was strengthened after RED and is explicitly
GREEN-only. Unusable nested scalars and duplicate nested error metadata cannot
erase outer IDs. These observer units do not admit those shapes as provider
errors. The selected-outcome unit preserves completion silence, Stream omission,
HTTP-owned metadata, exact returned Prompt values and private-message exclusion.

Both phases and the expanded RED use the same source-selected Trust15698,
qualified native project frontend/protected profile and unit runner in the
explicit unverified runtime lane. Capacity waits retain their logs; no claim or
compiler admission is bypassed. Independent static and evidence review records
bind actual sources, library/runner receipts, current artifact snapshots and
separate-target lint scope. Private evidence remains under the shared Git
alab-validation/websocket-prompt-correlation-20261115 directory.

All behavior and tests stay in ALab-owned files, with no upstream hook, protocol,
Core/Autopilot or source compiler change. This is source/API verification in the
explicit unverified runtime lane, not a new application certificate, package or
installation. The recorded application-certification prerequisite remains
unchanged. The original opaque HTTP400 backend cause remains unknown.
