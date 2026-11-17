<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep the workspace-routing retry reason out of the terminal error

The active app-server adds `workspaceRoutingDiscoveryTimeout` to the JSON-RPC
error data so the TUI can distinguish the one transient routing failure from
other `-32603` responses. After the third timeout, the TUI rendered the typed
server error directly. Its generic display includes all JSON-RPC data, exposing
that internal classifier beside the useful message and code.

Decision: remove only the recognized `reason` key at the final TUI rendering
boundary. Classification still requires `account/read`, code `-32603`, and the
exact structured reason (or the exact legacy message when data is absent).
The person still receives the routing-timeout message, code, and one method
prefix. Any additional server data remains visible, and every non-timeout error
is unchanged. The retry policy, request IDs, delays, and fail-closed routing
behavior do not change.

The focused TUI tests cover current structured exhaustion, legacy unstructured
retry compatibility, the three-request bound, distinct request IDs, exact
classification, and preservation of unrelated terminal diagnostic data. Static
review found no upstream hook change or broader error-redaction path: the change
is confined to ALab-owned startup glue.

Managed Trust 27009 ran the explicit unverified focused library-test lane:
all six startup-account-read cases passed, with 6,436 unrelated TUI tests
filtered out. This is runtime regression evidence, not an application proof or
package qualification. Scoped all-target Tippy also exits zero with no finding
in ALab-owned code; its warnings are confined to unchanged upstream sources.

A later independent hostile review found no production defect in the timeout
classification, retry bound, cancellation behavior, or terminal redaction. It
did find that Tokio's paused clock let the bounded-retry test auto-advance over
any finite delay, so the test did not enforce the intended one-second and
four-second schedule. It also compared only adjacent request IDs. Decision:
keep the paused runtime runnable, poll the read future explicitly at 999/1 ms
and 3,999/1 ms boundaries, require exactly five seconds overall, and compare
all three ID pairs. This makes immediate, excessive, or reordered delays fail
without changing production behavior. The corrected focused lane passes all six
cases again, with 6,436 unrelated TUI tests filtered out.

A subsequent cross-layer review found that preservation of unrelated server
diagnostics was proved only against the rendering helper, not through final
retry exhaustion. A future return-path refactor could therefore discard all
JSON-RPC data while both tests still passed. Decision: make the third timeout
carry both the internal reason and an unrelated request ID, then require the
returned terminal error to hide only the reason and retain that ID. Keep the
reason-only helper case explicit so empty sanitized data still disappears.

The next cancellation audit found the production path safe because the caller
owns `read()`, which in turn owns each request future and its backoff sleep, but
that ownership was not locked by a regression test. A future detached retry
task could outlive TUI startup cancellation and issue an account read against a
closing session.
Decision: poll the first timeout into its one-second sleep, drop the boxed read
future, advance across both retry boundaries while keeping paused time under
test control, and require the still-connected request channel to remain empty.
The expanded focused unverified test lane passes all seven
startup-account-read tests, with 6,436 unrelated TUI tests filtered out.
Scoped all-target Tippy exits zero; its warnings remain confined to unchanged
upstream sources.

A later error-phase audit found that startup runtime tests exercised only
server errors. Lower-level controls covered transport and deserialize display,
and one remote-client test reached transport, but no control drove either
failure through `read()`. A future retry or wrapping change could therefore
blur the failing phase or duplicate the method prefix without breaking the
startup suite. Decision: inject one native transport failure and one
successful-but-invalid response through the live cloud command channel,
require immediate error completion with no request already queued, and require
the returned typed and rendered errors to retain their distinct phase and
diagnostic marker. This adds no retry behavior and changes no production path.

The expanded focused unverified test lane passes all ten
startup-account-read cases, with 6,436 unrelated TUI tests filtered out.
Scoped all-target Tippy exits zero; its warnings remain confined to unchanged
upstream sources.

Application certification remains a separate delivery step. The committed
`trust.lock` still selects Trust 15698. A concurrent authenticated adoption of
Trust 27009 reached past the old proc-macro boundary, then failed closed because
required verification is unavailable for build-script execution. The exact
compiler evidence and decision not to repeat that unchanged attempt are in
`trust27009-adoption-build-script-refusal-2026-11-16.md`. No new package or
installed-artifact claim follows from these focused checks.

Post-merge follow-up ran the repository's widest gate on exact source
`98ca741f4124decfd7b61ff81eb758c199c432d6`. A freshly rebuilt controller from
the previously qualified aterm source `c0a183cc1c15d9de9f4b1a76b8d53d82f992fc9c`
(binary SHA-256 `2e2badb1958e682bec99d22897d7509319022b56c37bd2d18f0f2ca962796d8f`)
authenticated Trust 15698 and the committed tree before the run. The wide Tippy
stage found nothing in ALab's code and correctly set aside 44 upstream
diagnostics. The certified CLI build then failed closed at Trust 15698's exact
in-process proc-macro TCB boundary, before runtime and unit phases. Because the
authenticated Trust 27009 adoption already establishes the next unchanged
boundary—required build-script execution verification is unavailable—the
decision is to preserve the proof requirement and defer certification until a
signed Trust build implements that capability. Repeating either deterministic
refusal would add no evidence, and no package or installed-artifact claim is
made from this run.
