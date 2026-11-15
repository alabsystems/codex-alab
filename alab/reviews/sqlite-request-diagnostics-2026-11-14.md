<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Verify request evidence through the installed SQLite writer

The request diagnostics have endpoint and real Registry regressions, while the
existing SQLite integration checks only the opaque-recovery bridge event.
That leaves the persisted request shape, selected failure and saved context
outside the end-to-end coverage promised by the incident fix.

## Decision and reason

Exercise the public endpoint with deterministic transports, the actual dynamic
filter and SQLite writer, and real database queries after flushing. Use the
production INFO ancestor names and display-style values: `session_loop`
supplies `thread_id`, and the turn span supplies `thread.id` and `turn.id`.
Check the stored message as well as the dedicated thread and process columns.

The writer already admits these INFO targets. Its stored feedback body includes
event fields and ancestor fields, and `query_logs` exposes that body as the
message. Source review found no field-loss or filtering defect; production
changes require a failing regression rather than a speculative filter override.

Give this state-layer contract a test-only `alab-state-contracts` crate and move
the existing bridge regression with it. Keep the lean API transport tests
independent of SQLite and the persistence tests independent of Autopilot. Remove
Autopilot's direct API and tracing-subscriber test dependencies once their last
consumers move. The existing member glob and gate discover the new library.

Cover the inactive emitter with a live writer control, opaque HTTP 400 setup,
delayed SSE failure after caller context exits, and deferred failure superseded
by completion. Verify numeric metrics, safe identifier fallback, omitted unknowns
and original context. Check all diagnostic rows, including threadless ones, so
misattribution cannot hide evidence or sensitive fields.

## Validation and delivery boundary

Metadata validation preserves 1,494 existing lock entries. The only existing
entry changed is Autopilot's two removed direct test dependencies; the only new
entry is the ALab state-contract package. All new direct dependencies are
test-only. Its 515-package resolved test graph excludes core, Autopilot,
`core_test_support` and `wiremock`. The moved bridge assertions are unchanged;
only its filter import now uses the shared crate-root module.

Both library tests pass with source-selected Trust 15698 in the explicit
unverified test lane: the existing bridge contract and four endpoint cases in
the child-isolated endpoint test. The final run compiled in 1m53s and executed
in 0.21s. Focused all-target Tippy for the new crate and Autopilot passed;
`alab/lint.jq` reports no owned, hook-spanning, failed-target or missing-script
findings, with three upstream deprecation warnings set aside. This also compiles
every Autopilot target after removing its two direct test dependencies.

Both test modules share one inclusion of the installed filter at the crate root.
The initial duplicate-module lint was corrected without suppression or a copied
filter; the final runtime and lint checks include that correction. All 27
feature-inventory unit tests pass and committed ownership validates. Independent
review found no further required changes. A separate read-only hostile review of
the landed production recovery and diagnostic integration also found no new
concrete defect.

No runtime policy or filter change is proposed. The
[diagnostic guide](../REQUEST-DIAGNOSTICS.md) explains safe SQLite queries,
attempt correlation, field omissions and limits of the retained evidence.
Application delivery remains deferred at selected Targo's graph admission for
in-process procedural-macro dependencies, before solver execution or proof
collection; see the [prerequisite audit](targo-macro-admission-2026-11-14.md).
Targeted test results make no application qualification or installation claim.
