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

The implementation lives in ALab-owned TUI glue. The upstream TUI file has one
module hook and one guarded call hook, both marked `ALAB(workspace-routing)`.
Four focused tests pass on the merged source: timeout then success, bounded
exhaustion with no fourth request, another `-32603` failure without retry, and
structured-reason precedence. The tests also require distinct request IDs.

The canonical gate's complete Tippy phase passed before the last unrelated main
merge with no ALab findings and 41 upstream diagnostics set aside. Its certified
CLI phase then stopped at Trust 15698's known in-process proc-macro admission
boundary before proof collection. That same limitation is independently reproduced
on pristine upstream and documented in
`alab/reviews/targo-macro-admission-2026-11-14.md`; missing Trust JSON is a
consequence of the refusal, not a source diagnostic. This record grants no
certification claim, package build, installation, or running-process adoption.
