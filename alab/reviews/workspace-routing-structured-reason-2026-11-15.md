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

Application certification remains unavailable at Trust 15698's independently
reproduced in-process proc-macro admission boundary, documented in
`targo-macro-admission-2026-11-14.md`. These are source, runtime-test and lint
results only; this record makes no certification, package, installation or
running-process adoption claim.
