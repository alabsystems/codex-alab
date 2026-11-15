<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Diagnose the selected WebSocket result

Cancellation cleanup can fail after a response completion has arrived. The
existing stream preserves that completion and its usage, retires the socket,
and returns success. Logging the cleanup error before selecting that result
incorrectly reported a failed request. Conversely, cancellation expiring while
waiting for the socket mutex and finding an already closed socket selected
errors without recording their request attempt.

## Decision and reason

Observe the selected terminal result at all three error send sites. The
ALab-owned helper uses `inspect_err` and returns the original result unchanged.
A preserved completion produces no request-failure record; an actual selected
error retains the existing local request attempt. This follows the SSE
observer's distinction between a selected failure and a superseded error.
Observation does not claim the receiver accepted delivery when cancellation
races with a send.

Keep completion, usage, socket retirement, interruption deadlines, error values
and retry policy unchanged. A consumer dropped before the request task starts
selects no error and produces no failure record. No request body, raw error or
new sensitive field is retained.

## Validation and delivery boundary

The first parallel API run passed the new WebSocket regression but exposed an
existing direct request-shape test capturing no events. Its thread-local
subscriber shared tracing callsite interests with other tests. Isolate this
recorder in an exact subprocess with a child-only global subscriber, keeping
all original correlation and privacy assertions. The failure is consistent
with concurrent interest registration; the precise interleaving was not
reproduced. The correction changes test isolation without weakening assertions
or adding production cache rebuilds.

All 258 API library tests passed in the source-selected Trust 15698 explicit
unverified test lane. The public WebSocket entry fixture drives an actual
create, cancellation, completion with usage, and failed cancellation
acknowledgment through the event parser. It also covers the closed-socket,
queued cancellation deadline and dropped-consumer selections. Focused Tippy
for all API targets passed; `alab/lint.jq` reports no owned, hook-spanning,
failed-target, missing-script or upstream findings.

Application packaging remains deferred at the procedural-macro certification
boundary recorded in `bad-request-2026-11-13-evening.md`. No application package
was built or installed, and the certified-build requirement remains intact.
