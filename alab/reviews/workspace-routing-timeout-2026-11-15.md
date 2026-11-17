<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retry the transient workspace-routing timeout at the TUI boundary

Running `codex` could stop before drawing the TUI with `account/read failed:
workspace routing discovery timed out`. The app server serializes workspace-route
discovery, while each caller's 15-second deadline includes time spent waiting for
that serialization. Startup can issue overlapping account reads, so a slow leader
can consume the follower's whole deadline and make both reads fail together.

Decision: keep routing fail-closed and retry only the exact transient
`account/read` failure. The TUI makes at most three reads, gives every read a fresh
request ID, and waits one then four seconds between attempts. The structured
`workspaceRoutingDiscoveryTimeout` reason is authoritative when present; the exact
legacy message is accepted only when structured data is absent. Authentication,
configuration, other server failures, and every ambiguous response remain fatal.
No unrouted fallback was added.

Classify every attempt independently and never broaden the retry category. After
one classified timeout, a different `-32603` error must terminate at the one-second
boundary with its typed `account/read` method, code, message, and absent data
preserved. Advancing the remaining four seconds must issue no delayed third read.

Success terminates the retry chain. After a legacy timeout, a successful second
read returns the exact account response at the one-second boundary with a fresh
request ID. The test yields at that boundary and requires an exactly empty request
queue, then advances four seconds, yields, and checks it again to reject a delayed
third read.

Retry budgets belong to each read, including consecutive calls on one session.
After the first call times out and succeeds at t=1s, the next call issues its
initial request immediately. After its timeout, it stays pending through another
999ms with an exactly empty queue, then retries and succeeds after 1ms, at t=2s.
The test requires four distinct request IDs and empty queues after both successes.

Exhausting a call's retry budget must not poison the session. A separate test
times out three reads at t=0s, t=1s, and t=5s, requires immediate `Err` and exactly
`TryRecvError::Empty`, then drops that call's backing future. A new call on the
same session issues immediately and returns the exact account response with zero
elapsed time. All four request IDs are distinct, and yielding after success still
leaves the channel open and idle.

An immediate fatal server error must not poison later reads on the same session.
The selected-workspace-missing read returns `Err` without advancing paused time,
renders the fatal message with one `account/read failed` prefix, and leaves the
command channel exactly `Empty`. After its backing future is dropped by scope,
the next read issues immediately with a distinct request ID and returns the exact
account response with zero elapsed recovery time; yielding still leaves `Empty`.

Canceling an in-flight read must also leave its session usable. The test drops
the pending future before any response and starts a distinct read at the same
instant. Cancellation closes the first oneshot receiver, so sending its late
routing timeout must return the exact undelivered payload. The second receiver
stays open and its read stays pending with an exactly empty queue until its own
success response returns the exact account with zero elapsed recovery time.
Yielding after success and advancing the full one-second then four-second retry
schedule must leave exactly `TryRecvError::Empty`, rejecting detached retry work.

Success on the final permitted read is valid: after timeouts at t=0s and t=1s,
the third read succeeds at t=5s and returns the exact account response. The test
requires three distinct request IDs, yields at that boundary, and requires an
exactly empty queue so successful exhaustion of the schedule issues no fourth read.

Bounded exhaustion must preserve a typed `TypedRequestError::Server` with method
`account/read`, code `-32603`, and the exact timeout message. Sanitization removes
only the internal `reason` field before rendering; the test requires the remaining
data to equal `{"requestId":"keep-this-diagnostic"}` exactly.

Terminal reads must leave the command channel open and idle while the session
stays alive. Bounded timeout exhaustion and an immediate fatal server error
therefore require exactly `TryRecvError::Empty`, so both an extra request and a
disconnected channel fail the assertion.

The implementation lives in ALab-owned TUI glue. The upstream TUI file has one
module hook and one guarded call hook, both marked `ALAB(workspace-routing)`.
Focused coverage includes timeout then success, bounded exhaustion with no fourth
request, cancellation during backoff, fatal server and transport/decode errors,
per-attempt classification after a timeout, and structured-reason precedence.
The retry tests also require distinct request IDs and exact timing boundaries.

The canonical gate's complete Tippy phase passed before the last unrelated main
merge with no ALab findings and 41 upstream diagnostics set aside. Its certified
CLI phase then stopped at Trust 15698's known in-process proc-macro admission
boundary before proof collection. That same limitation is independently reproduced
on pristine upstream and documented in
`alab/reviews/targo-macro-admission-2026-11-14.md`; missing Trust JSON is a
consequence of the refusal, not a source diagnostic. This record grants no
certification claim, package build, installation, or running-process adoption.
