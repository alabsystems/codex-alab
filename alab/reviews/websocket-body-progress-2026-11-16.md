<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# WebSocket rejection entity progress — 2026-11-16

## Decision and reason

Preserve a numeric count of entity bytes accepted by the bounded WebSocket
rejection decoder. The existing complete-only count cannot distinguish a stalled
empty entity from a partial timeout or truncation. This is a diagnostic improvement,
not evidence of a recovery defect or an explanation of the original opaque HTTP 400.
That backend rejection reason remains unknown.

`RejectionMetadata.entity_bytes_accepted` snapshots the decoder body length before
incomplete-body suppression. The active handshake INFO event reports it as
`rejection_body_bytes_accepted`. A created decoder that accepts no bytes measures
zero; decoder-creation failure leaves the observation absent. The count is bounded
by 65536 bytes, excludes transfer framing and data rejected by the retention limit,
and differs from HTTP's successful-read counter, which includes a rejected chunk.
Only complete entities retain payloads and the existing `rejection_body_bytes`.

All behavior and tests live in ALab-owned paths. No upstream hook, manifest,
lockfile, dependency, decoder branch, retention/time budget, header-time advice,
error classification or policy-denial passthrough changes. Cancellation still
produces no completed handshake diagnostic. The metadata struct gains an additive
public field; the repository has one constructor and no external compatibility
requirement is established. The snapshot uses existing storage and a scalar;
subscriber formatting may allocate.

## Verification

The corrected test-only RED source is `1fd6819f5e2b6c8658504fe722579439a58dbbfe`.
The native source-selected five-library run executed 993 outer methods: 987 passed
and six failed at the late missing-metric assertion, with no ignored, measured or
filtered methods. The six exact-child failure summaries are excluded from outer
totals. Four failures expected 24 bytes, one 78 bytes and one measured 0 bytes.

The final runtime source is `6b6828f7777b5913cbc5d5bad8849f56948fc3a7`.
Its complete selected libraries passed 996 outer methods: public API 96, ALab Core 424,
API 281, HTTP 139 and WebSocket 56. No methods were ignored, measured or filtered.
The six required public methods and three new direct-handshake methods passed once
each. All-target scoped Tippy passed with zero owned, spanning, failing, build-script
or set-aside diagnostics. Unit, lint and controller statuses were all zero.

The public log regression is byte-identical to corrected RED. GREEN reaches all
eight public exchanges, including complete typed 403 and empty truncated 403 cases
that follow the first RED failure in their methods. The three additional adapter
methods execute 16 rows: absent decoder versus measured zero, partial truncation and
read failure, malformed chunk metadata and an oversized next chunk after a valid
prefix, complete chunked framing exclusion, the exact 64 KiB body boundary, and empty
or partial timeouts. Timeout tests explicitly poll the actual handshake to Pending
before advancing the paused clock; no server-write-to-client-read inference is used.
Existing policy-denial and upgrade tests also pass. Cancellation is source-reviewed
only; no cancellation runtime case is claimed.

All runs selected the pinned native Trust 15698 frontend through the project selector
and the explicit unverified test lane. Five live test binary identities, native
artifact digests, helper receipts, execution paths and diagnostic bindings were
captured independently before each rebuild; original and corrected RED plus GREEN
have separate observations. Independent source and private-controller reviews
accepted the final implementation and corrected fixture semantics. Final native
artifact qualification is recorded beside those observations in the session's
private validation stage.

Additional typed-metadata adapter tests are introduced after RED and are qualified
only by GREEN execution; no RED assertion is claimed for those methods.

The first baseline attempt produced five missing-metric failures and one earlier
fixture-oracle failure: an empty 403 receives ordinary backoff, whereas the typed
forbidden fixture is terminal. The helper now takes an explicit expected retry
classification for each response. Production recovery policy is unchanged.

An interrupted GREEN attempt requested source `ae55fabdd436fef9060ca31936f3a233a4407d51`.
The initial report inferred no execution from a queued status, but its retained log
later contained five unit suites with 987 passes and six missing-metric failures.
Checking only the terminated process group did not establish that delegated native
work had stopped before the checkout advanced. The actual executed source and
compiled-input freeze are unqualified: no final source guard, live binary custody,
unit status file, lint result or completed controller record was retained. These
observations supply no GREEN qualification. The corrected RED and final GREEN have
separate completed guards and independently captured live artifacts. Original logs,
incorrect reports, retractions and corrected observations are retained. Future
interruption recovery must establish complete native-job quiescence before changing
source; a vanished process group alone is insufficient.

## Boundaries

The tests use scripted I/O through the actual WebSocket client handshake and real
loopback public API calls. They do not establish physical network arrival timing,
host-sleep behavior, backend acceptance, or execution of a full Core/Autopilot retry
actor. The metric does not grant recovery authority or log body content.

Application certification remains unavailable under the source-selected Trust 15698
procedural-macro admission refusal. The separate unadopted 27009 candidate instead
failed required build-script-execution verification. The reviewed selector and
capability inputs are unchanged; another identical full gate would not add evidence.
No application proof, package installation, binary adoption or managed delivery is
claimed. A newly authenticated qualified capability is required before retrying
unchanged native adoption and the full gate.
