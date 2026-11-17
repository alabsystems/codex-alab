<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Repeated HTTP advice before delayed opaque bodies — 2026-11-16

Decision: extend the existing owned real-TCP HTTP rejection fixture to cover
repeated Retry-After fields and a delayed complete opaque HTTP 400 body. The production
repeated-header fix already exists; this closes its actual-wire integration gap.
Keep application behavior, retry authority, manifests and upstream hooks unchanged.

The fixture sends one real Responses POST with the original input and matched
completed function-call/output pair. It sends the exact opaque JSON except its
final `}`, declaring the full Content-Length. A narrow subscriber observes only
the static Request completed message and timestamps it, without formatting or
storing URL, headers, status or other fields from that DEBUG event. Existing safe
endpoint/body-phase capture remains separate. HeaderReceipt keeps its original
atomic count for the older policy/cancellation controls.

The owning API future is driven to client header receipt and then explicitly
polled Pending before recording the upper capture bound. Source order places
transport RetryAfter capture after the synchronous header observer and before
the held body await. A retained Notify permit avoids losing notification; the
owning future continues being polled while waiting for it. Another 100ms timeout
must leave that same owning future pending. The explicit release timestamp must
be at least 100ms after the upper bound. Only then may the final byte arrive.

The fixture completes, the owning future and client are dropped, and the peer is
joined before semantic assertions. Each case requires exact ordered get_all
header values, original400/body/safe IDs, one request telemetry attempt, complete
body-phase bytes, endpoint shape/context/privacy, genuine normalized opaque Stream
identity, retryable Other protocol and unchanged mapped advice. Last, require
the selected deadline within receiptLower+7s through pendingUpper+7s. A body-time
recapture would exceed the upper bound. Earlier shorter selection fails the lower
bound. Safe static markers identify the completed single-header control and the
historical failure row without printing request/response content.

Cases run in order: `[7]`, `[3,7]`, `[invalid,7]`, `[7,3]`. The real unpaused clock
exercises timing without a simulated host sleep. The wire peer observes one POST
and closure after client drop; this is not a global socket inventory or proof of
connection reuse. No actual Core/Autopilot retry is executed by these cases.

Historical RED source d8b323ff6bd1aad296776bfd69ade1999f60702f applies only this
one-file test patch to pre-fix main f1ee445a01010b587cc4c40d8ebffd923aa764f4.
It executes 488 outer methods: 487 passed, one failed, zero ignored/measured/
filtered. Raw child markers show `[7]` complete and `[3,7]` late_bounds before
the intended deadline assertion at line 1111. Later rows are GREEN-only. Public
results are 79/1, API 271/0 and HTTP-client 137/0; nested child failure summaries
are not additional outer methods. No compilation or early fixture failure masks
the advice-selection regression.

Merged current GREEN source dbb3828d6ebb8c76a410e2ab29d2fa13d28fa219 retains that
historical commit and main 4d5db85044c44a8b01564c95407b358deeab3f21. The test file
is byte-identical to RED. Three complete selected libraries pass: 91 public,
271 API and 137 HTTP-client methods, 499 total, zero failed/ignored/measured/
filtered. All four new rows complete through the exact passing outer method and
its successful child; successful child markers are drained rather than printed.
Scoped all-target Tippy has zero owned/spanning/failing/scripts diagnostics and
zero set-aside. The source-selected Trust 15698/native frontend and unit runner
use the explicit unverified runtime lane. Historical receipt metadata binds RED;
GREEN binaries independently match all three native preparation snapshots and
receipt/diagnostic identities. Only this decision record follows that execution.


This is source/test coverage only. No new application certificate, package,
installation or managed-runtime adoption is claimed. The recorded source-selected
Trust application macro-admission prerequisite and unknown original opaque HTTP 400
backend reason remain unchanged. The recovered pre-existing tab stays on its task.
