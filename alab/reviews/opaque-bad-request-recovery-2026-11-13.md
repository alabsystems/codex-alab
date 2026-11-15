<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Opaque bad-request recovery

The reported running session ended a turn with `{"detail":"Bad Request"}` and
protocol category `Other`. The HTTP bridge maps an ordinary 400 to
`InvalidRequest`, whose retry policy is `Never`; Autopilot then treats that
`Other` detail as a fatal request error. This explains the interruption. The
backend supplied no diagnostic identifying why it rejected the request.

An isolated loopback reproduction using installed package `2026111301` produced
one request, exit 1, and the same terminal error despite a nonzero stream retry
budget. No real account or external model endpoint was used.

## Change

Recognize only the single-field opaque envelope, rejecting duplicate keys,
additional fields, and specific provider errors. Route that HTTP 400 through
core's bounded retries and transport fallback. If those attempts end the turn,
use Autopilot's paced server retry, without the repeated-cutoff hold. A saved
legacy `Other` completion with the exact envelope restores that server wall.
Explicit validation, policy, access, and configuration refusals retain their
existing behavior; current retry policy still governs continuation.

The existing affected tab's goal remained stalled after a manually continued
turn succeeded. `/goal resume` restored its existing goal, and the tab reported
`Goal active` and resumed execution. Its application binary was not replaced.

## Validation and delivery boundary

The envelope unit test, 25 recovery tests, and 28 HTTP-bridge tests passed using
source-selected Trust 15698's explicit unverified test lane. The native core
fixture `native_turn_retries_opaque_http_400_but_not_a_specific_refusal` also
passed: a real loopback HTTP 400 followed by SSE completion took two model
requests and completed the same turn; a specific validation error took one
request and failed the turn.

The full native gate on `1eff1aebac8e46fb892be8d8100c10102e69d661` passed Tippy
with no ALab findings (41 upstream diagnostics set aside). Its certified
application build then failed before compilation: Trust refuses the existing
in-process procedural-macro dependency graph and consequently emits no
identity-bound proof inventory. The retained observation is under
`.git/alab-qualification/1eff1aebac8e46fb892be8d8100c10102e69d661/run-0tpns5xh/`.

This is not a successful full gate or a certified application build. Preserve
the certification requirement and its failure; do not substitute an unverified
application build or change the pinned compiler to claim delivery. A new
package and the fixed-binary reproduction remain blocked on that compiler
capability. The source fix and native-core runtime test do not claim that the
installed application has been upgraded or that the backend's own rejection
cause has been repaired.
