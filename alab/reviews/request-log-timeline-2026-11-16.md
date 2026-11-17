<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Bounded request-log metadata timeline — 2026-11-16

## Decision and reason

Add `alab/request_timeline.py`, a one-shot read-only diagnostic command with an
explicit database path and process/thread scope. The blocked session's UI retry
notice and total turn age do not establish how long capacity retries have run.
Existing formatted-log queries expose bodies; the native logs client initializes
state and tails continuously. A bounded metadata view provides useful evidence
without an application build or interpreting arbitrary formatted span fields as
structured authority.

The adjacent request-classification and capacity-policy audits established no
new production defect. The original opaque HTTP400 still has no retained
rejected field, wire request or service identifier sufficient to identify its
backend cause. This command improves observation; it does not claim to solve
that unknown cause or change application recovery policy.

## Contract

The standard-library Python command requires an existing current-schema SQLite
logs database and at least one validated PID, process UUID or thread UUID.
Supplied filters intersect. PID reuse remains separated by process UUID. It
selects only seven metadata columns for four fixed diagnostic targets and five
standard levels, returning one JSON document with a bounded latest-row window.
It neither selects nor parses rendered bodies, source paths or span fields.

Read-only URI mode, query-only execution, ordinary-table/schema checks and a
column authorizer constrain database access. The reader neither initializes nor
migrates the database. Active WAL commits remain visible. This is logical SQL
projection and output control, not physical page isolation, writer
authentication, an immutable snapshot or pathname custody. It follows the
operator-supplied path and does not discover effective session configuration.

Rows return in ascending row-ID order. Gaps use stored wall-clock timestamps
only within the same validated process, thread, target and level. Missing
identities are never joined; invalid or backward timestamps do not produce a
duration. These selected-row gaps are not retry intervals, deadlines, outage
durations, complete request counts or business-progress evidence. Buffering,
retention, older emitters and omitted invalid metadata can leave incomplete
evidence.

Row/result limits and cooperative VM/time/lock budgets constrain work without a
hard I/O deadline. Errors use fixed categories and optional numeric SQLite
codes; SQLite text and partial successful output are not emitted. Database
cleanup precedes successful JSON output. An available primary failure survives
a close failure; a close-only failure remains nonzero, including inside a
caller's unrelated exception handler.

## Hostile review and qualification

Independent source/privacy review found and resolved three concrete edge cases:
column collations could expand exact filters, `INTEGER PRIMARY KEY DESC` could
retain arbitrary text under the same column metadata, and a close failure could
replace an available primary error or be suppressed by a caller's active
exception. Explicit binary comparisons, post-validation, numeric projections
and operation-owned failure tracking now have controlled SQLite/CLI regressions.

Committed source `b25a2b0e8737a72ac5799d9002dd620652b7b2be` passed all 28 focused
methods once, with zero failures, errors, skips, expected failures or unexpected
successes. Managed Python 3.14.8 and SQLite 3.53.1 ran real temporary databases
and controlled CLI subprocesses. Tests include active-WAL commits versus an
uncommitted row, database locks, large private bodies, malformed metadata,
hostile schema/collation variants, read-only preservation, budgets and cleanup
faults. Resource warnings were treated as errors.

The private `request-log-timeline-20261116` stage binds actual method outcomes,
raw outputs, interpreter identities, committed source and before/after hashes
for the reader, test, existing CLI helper and complete SQL migration listing.
The earlier author and initial committed runs are separate historical evidence;
their counts are not added to the 28 distinct methods. Subsequent documentation
does not alter qualified executable inputs.

No actual user database, Codex/provider request, network service, native suite or
packaged application was exercised by these fixtures. The unchanged certified
application-build prerequisite is unavailable; the prior human hold remains.
Do not substitute an unverified shipping build or change the installed peer,
model or account. The new source command can be used independently once its
database path and process scope are verified.

## Publication

All four owned paths are ALab-owned. There is no upstream, Rust, manifest,
lockfile, compiler pin or application behavior change. Publish through the
maintained expected-main guard. Preserve concurrent already-main work exactly;
reject changes to qualified inputs, migration membership, publication helpers or
other guarded controls until they receive appropriate review and qualification.
Record any disjoint composition with its original qualification source. Finish
owned checks before checkout/ref cleanup, retain private evidence and the
handoff, and leave other branches and the pre-existing session intact.
