<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Give the transient workspace-routing timeout a stable wire reason

The bounded startup retry added in `workspace-routing-timeout-2026-11-15.md`
already preferred a structured `workspaceRoutingDiscoveryTimeout` reason, but
the app server did not produce that reason. Production therefore depended on
the exact English legacy message. A wording-only upstream change could silently
remove the recovery and return the person to the original launch failure.

Decision: in active ALab mode, add `data.reason` only when
`AccountReadError::Routing` contains `WorkspaceRoutingError::DiscoveryTimeout`.
Keep JSON-RPC code `-32603` and the existing message unchanged. Vanilla mode,
invalid-account errors and every other workspace-routing failure retain their
upstream unstructured response. The TUI still requires the exact `account/read`
method, code and reason; structured data remains authoritative, and the legacy
message is accepted only when data is absent. No retry category or routing
fallback was added.

The discriminator and mapping live in ALab-owned core code. The upstream app
server has one marked match-arm hook and one marked include hook. The consumer
uses the same constant from ALab core, eliminating producer/consumer string
drift. Focused unverified Trust tests pass: two producer tests preserve the
legacy error and prove vanilla/non-timeout responses remain unstructured; five
TUI tests cover bounded fresh-request retries, non-timeout failures, structured
reason precedence, and rejection of the right reason on the wrong method or
code. Scoped all-target Tippy for `codex-alab-core`, `codex-app-server` and
`codex-tui` exits zero in its dedicated cache. It reports only pre-existing
upstream diagnostics outside the changed glue and hook lines.

Independent hostile review found no blocking issue. A full serialized
app-server assertion was considered and deferred: the ordinary test server does
not activate ALab, a real timeout costs the production 15-second deadline, and
covering the production match by extraction would move upstream invalid-account
mapping into ALab glue solely for test reachability. The direct mapping tests
plus compilation of the one-line production hook are proportionate. A
server-side deadline reset was also rejected: waiting for the keyed semaphore
is deliberately inside the request deadline, and resetting afterward could
turn a 15-second read into 30 seconds or more. Correct shared-result
coordination would require substantially broader lifecycle work.

A later consumer-policy audit found that every matching-reason control still
used the legacy message, while the sole present-data rejection control supplied
an explicitly different reason. It therefore did not prove that the reason
alone was sufficient or that data lacking a reason disabled legacy fallback.
Decision: add a changed-message/matching-reason control and exact-message
controls with empty, unrelated and mismatched structured data. This locks the
stable reason as authoritative while keeping legacy message fallback
fail-closed whenever data is present. No production behavior changes.

The focused unverified TUI lane passes all ten startup-account-read tests,
including the expanded authority matrix, with 6,436 unrelated tests filtered
out.
Scoped all-target Tippy exits zero; its warnings remain confined to unchanged
upstream sources.

After clean composition onto the then-current remote main, an exact focused
rerun was admitted but spent more than ten minutes behind Cargo's package-cache
lock, held by a concurrent `targo clean`, without reaching compilation. It was
stopped rather than bypassing that shared lock. The incoming commit set did not
touch this glue or review; the evidence above applies to this topic before that
non-overlapping composition. No merged-tree rerun is claimed.

Application certification remains unavailable at Trust 15698's independently
reproduced in-process proc-macro admission boundary, documented in
`targo-macro-admission-2026-11-14.md`. These are source, runtime-test and lint
results only; this record makes no certification, package, installation or
running-process adoption claim.
