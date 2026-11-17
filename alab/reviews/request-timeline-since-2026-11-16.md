<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Validate seconds before selecting the timeline window — 2026-11-16

## Decision and reason

Require integer stored seconds when `request_timeline.py` applies `--since`,
before ordering and limiting SQL candidates. Hostile review of the landed tool
found that SQLite's comparison rules admit malformed TEXT, BLOB and REAL values
through `ts >= ?`. Newer malformed rows then occupy the limited window and
displace known qualifying records, although projection later makes their times
unavailable. A migration-built temporary database independently reproduced the
same defect twice.

The fix is one predicate. Without `--since`, unknown timestamps remain visible
and reset the affected gap group. Inclusive integer boundaries, numeric text
stored as INTEGER by SQLite affinity, and the maximum signed integer retain
their meaning. Nanosecond validation and all read-only, privacy, schema, error,
identity and work-budget controls remain unchanged. No compatibility branch,
extra query or automatic window refill is added.

The guide also makes an existing limit explicit: `more_rows_available` means a
SQL lookahead candidate exists before canonical identity validation. It cannot
promise another usable returned row. The omission counter covers only the
limited window, excluding lookahead. Seven independent temporary SQLite probes
found no numerical gap or identity-grouping defect and established this bounded
documentation clarification.

## Regression evidence

Test-only source `3f2b5cbb194f3b6a0a55cd213c6a1e10aa116877` retains the landed
reader exactly. It ran all 31 focused methods: 30 succeeded, and the new
selection method failed its TEXT, BLOB and REAL subtests. The child exited 1.
Those are three failed subtests in one method, not three additional methods.
Boundary/affinity/maximum controls and unknown-time behavior without a filter
passed on that original implementation.

Fixed source `b4864cc6b8548dc9e6d4bd9a07d17b2a432fefd9` passed the same 31
methods with zero failures, errors, skips, expected failures or unexpected
successes; the child exited 0. The test bytes are identical between phases.
The reader's only executable change is the integer-type predicate. Subsequent
review documentation does not change qualified inputs.

Both phases used managed Python 3.14.8 and SQLite 3.53.1, real temporary SQLite
databases and controlled CLI subprocesses. Resource warnings were errors.
Private `request-log-timeline-hostile-20261116` records retain exact method and
subtest outcomes, raw outputs, interpreter identity, clean committed sources,
before/after input hashes and complete SQL migration membership. Separate RED
and GREEN records keep their own source and exit attribution; counts are not
added across phases.

## Scope and publication

This corrects the standalone diagnostic command, with all four changed paths
owned by ALab. It does not change application requests, retry policy, Rust,
upstream files, dependencies, compiler selection or installed processes. These
fixtures do not qualify an actual user database, provider, native suite or
packaged application. The original opaque HTTP400 cause remains unknown; the
unavailable certified application-build prerequisite and human hold remain.

Publish through the maintained expected-main guard. Preserve disjoint
already-main peer work exactly, without transferring these test results to its
behavior. Reject overlapping qualified inputs or protected publication helpers
until reviewed and requalified as appropriate. Bind independent source,
RED/GREEN evidence, control and any composition reviews before cleanup; preserve
other branches and the existing peer session, complete owned checks, and save
the private handoff.
