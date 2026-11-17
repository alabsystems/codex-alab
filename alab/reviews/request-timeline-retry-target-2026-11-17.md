<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Ordinary Responses retry metadata in the timeline — 2026-11-17

## Decision and reason

Extend the bounded metadata-only reader to the exact codex_core::responses_retry
target. Source inspection identifies WARN events for ordinary sampling retries,
foreground remote-compaction V2 retries and unbounded connection waits. The
existing four-target allowlist includes ALab capacity events but excludes this
parent module. This is a deliberate reporting-scope extension, not a demonstrated
emitter defect or a diagnosis of the original opaque HTTP400.

Add the literal target and derive its SQL placeholder count from the trusted
TARGETS tuple. Values remain bound parameters. Keep the level policy, seven-column
projection, authorizer, identity/BLOB caps, schema/encoding admission, query/lock
budgets and latest-window rules. No log body, span payload, error text or native
emitter changes. The guide explains the expanded scope and its limits.

Metadata cannot distinguish those warning types or identify their attempts,
retry numbers or turns. Within one exact process/thread/target/level group,
mixed warning types can share selected-row gaps; those gaps are not retry
intervals, pacing or current-turn progress. Other legacy compaction targets and
older emitters using different names remain excluded. Missing or invalid thread
identities still cannot form shared gaps.

## Controlled baseline and regression

At baseline 8a414af19a5abc66e280ac551766f85e3cdb7f39, create a private SQLite
database from the two committed migrations with 13 controlled rows. Six API and
six actual CLI calls agree: ordinary target rows are omitted in every matching
scope, while original/capacity rows, prefix exclusions and scope controls remain
correct. Database bytes and all 12 bound source/migration inputs stay unchanged;
private body/span sentinels never enter outputs or metadata SELECTs. These calls
are probes, not unittest method counts, and use no user database.

Tests-only source d7f440c0799377ae1cd92d2bbad12f82f3d4cbd6 adds two methods
and renames one count-specific method without changing any of the 37 prior
bodies or helpers. A strict single-name mapping plus normalized AST comparison
checks that preservation. The actual RED run executes 39 methods: all 37
baseline methods pass and the two new methods fail, with two failure entries,
zero subtest failures and zero errors. API row IDs are [1,3] instead of
[1,2,3,4,9]; the CLI limited window is [1,3] instead of [3,4]. Privacy, read-only
and query-projection checks before those assertions succeed. Later gap/window
assertions are not credited to RED.

Fix source 05e0a02dd07f21066c0a8ec5ccc6ba3549c633dd changes only the reader
and guide; test bytes stay identical. Its initial GREEN run passes all 39 methods
once, with no failures, errors, skips, expected failures or unexpected successes.
New controls cover exact-target admission, non-prefix matching, process/thread
intersections, PID reuse, unknown threads, distinct target/level groups, limited
windows, private-body exclusion and an actual CLI document.

## Composed-source qualification

Remote main advances to a4c6d583e9f0cd68e2168e2c364ee45e03e959dc with 925
already-landed paths. None touches the three runtime-owned files or five Python
qualification inputs. Relevant static review confirms the retry producer,
module name, writer and filter remain compatible. TUI logging installation moves
to an owned helper, so its file and module declaration expand the static guard
from six to eight paths. No native or peer behavior qualification is inferred.

Merge source edaecf4415db16ac7cebce53558dbc6eb9a10598 preserves every peer
byte and all three owned files. A fresh composed-a4c GREEN run passes the same
39 methods once, with all nonpass counts zero. Its source, explicit phase,
qualification digest, eight static context paths and independent review are
required by closing gates; an old review copied to the child path cannot satisfy
them. Original RED and initial GREEN remain immutable historical records, with
an 84-file snapshot. Results are not added across phases.

Both qualifications use isolated managed Python 3.14.8, SQLite 3.53.1, disabled
bytecode writes and ResourceWarning as an error. Runtime inputs are the reader,
tests, CLI helper and both migrations. Rust source guards provide static context
and publication protection, not additional executed inputs or native evidence.

## Publication and limits

The private request-timeline-retry-target-20261117 stage retains actual raw runs,
source/runtime/control hashes, static reviews and independent evidence receipts.
The final decision record is the only change after composed qualification.
Publish through the maintained expected-main guard, preserve disjoint peer work
and retain composed-source attribution through branch/worktree/process cleanup.

This adds diagnostic metadata availability. It does not repair an unobserved
request-generation defect, establish a backend cause, certify the application or
update an installed session. The original opaque HTTP400 cause remains unknown;
the source-selected application prerequisite and human hold remain unchanged.
