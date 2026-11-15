<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Bound the request diagnostic fixtures

The SSE context regression opens a real loopback connection before releasing a
semantic failure. Its server previously waited without deadlines for connection
acceptance, request headers and body, response release and completion. Its
client also awaited opening headers without an outer timeout. The stream idle
timeout starts later, so a transport regression could leave both sides waiting
and block the isolated child test's parent indefinitely.

## Decision and reason

Give the SSE server explicit ownership, bounded protocol phases and bounded
cleanup. Dropping a fixture, including cancellation while finishing it, must
abort its task. Bound the public client opening calls too. Keep the existing
subscriber and parent-span lifetime assertions and the independently gated
semantic failure.

Exercise an incomplete real TCP request and observe a named body deadline and
connection closure. Observe connection closure after fixture cancellation as
well. These controls check externally visible lifecycle behavior rather than
internal bookkeeping. Keep this change in the test-only API contract crate;
there is no new production timeout or retry policy.

## Validation and delivery boundary

All six API contract tests pass with source-selected Trust 15698 in the explicit
unverified test lane. The three real TCP controls observe the named incomplete
body deadline, closure after fixture drop and closure after cancelling a polled
`finish()` future. Both original context regressions still pass, including
request dispatch retention and ordinary WebSocket telemetry callback context.
The HTTP retry contract passes in the same suite; total runtime was 0.11s.

Focused all-target Tippy reports no findings after ALab classification, and
committed feature ownership validates. Independent source review found no
concrete issue. The manifest, dependency lockfile, compiler pin and production
sources are unchanged. Exact-source logs and hashes are retained with the
[retry contract validation](http-retry-diagnostics-2026-11-14.md).

The application certification prerequisite remains separately deferred. The broader handshake-correlation
question was reviewed without finding a recovery-policy defect: a handshake
precedes payload preparation, and connection IDs need a distinct design rather
than being mislabeled as per-request response IDs. No speculative handshake
instrumentation is included here.
