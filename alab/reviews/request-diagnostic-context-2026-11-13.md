<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Retain the request's diagnostic context across transports

The landed WebSocket diagnostic retains an attempt number, but its spawned
task only enters the caller's span. Entering that span does not install the
caller's thread-local tracing dispatcher. After the caller's subscriber guard
exits, a selected failure can disappear or reach another subscriber without its
original thread and turn. The global-subscriber WebSocket regression could not
detect that loss. The SSE observer already preserves both handles.

## Decision and reason

Retain the dispatcher and span in the shared request diagnostic when recording
its shape. Restore them only around that diagnostic's failure event. Remove the
SSE observer's duplicate context ownership and restoration; its cloned request
diagnostic supplies the same context. All WebSocket terminal selections already
use the shared failure observer, so no additional upstream hook is needed.

Keep existing telemetry callbacks outside the restored context. Do not wrap the
whole spawned task, change stream delivery or retry policy, or retain another
copy of request content. Inactive paths return before constructing a diagnostic
and capture no tracing handles.

## Validation and delivery boundary

A real WebSocket regression uses the existing gated loopback server and tracing
Registry. It waits until the server observes the request, drops the caller's
subscriber and both parent span handles, then releases the semantic error. The
failure must retain its attempt, thread and turn; the existing telemetry callback
must still see its ordinary subscriber. Privacy and error-value assertions
remain independent of the context check.

The regression first failed against unchanged production at `7e520396c1`:
the caller received the original `InvalidRequest`, but the Registry captured
only the shape event, one record rather than two. After consolidating retained
context, all 258 API library tests and both real Registry regressions passed
with source-selected Trust 15698 in the explicit unverified test lane. The
existing SSE regression remains unchanged; the new WebSocket test also proves
its telemetry callback stays outside the diagnostic's restored subscriber.

Focused Tippy for every API and Autopilot target passed. `alab/lint.jq` reports
no owned, hook-spanning, failed-target or missing-script findings; three
upstream deprecation diagnostics are set aside. Independent review found no
required corrections, extra payload retention or intrinsic ownership cycle.
The change adds no dependencies or upstream hooks.

The source-selected Trust pin remains 15698; the existing procedural-macro
certification boundary and managed package 2026111301/index 136 are unchanged.
Application packaging remains deferred until that compiler capability is
available. No unverified application package was built or installed.
