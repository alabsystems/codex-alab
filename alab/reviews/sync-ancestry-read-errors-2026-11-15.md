<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep ancestry read failures distinct from superseded syncs

Review of the shared sync-history reader's callers found that
`take_concurrent_main` negated `git merge-base --is-ancestor` directly. Every
nonzero status, including a Git read failure, entered the superseded-sync path.
It exited 3 with advice to abort the sync and named any local fix-forward commits.
The function did not itself abort, but its advice rested on no ancestry evidence.

Decision: retain the query's status explicitly. Status 0 allows the ordinary merge;
status 1 alone enters the existing superseded-sync path. Other statuses fail with
a read-error diagnostic before inspecting local commits or invoking any merge,
lockfile resolution or commit operation. A missing prior sync still needs no
ancestry query, and failed history selection still stops immediately.

The regression executes the production function with recording boundary fixtures.
Before the fix, injected statuses 2, 128 and 137 all returned superseded-sync exit 3
and abort advice. After the fix they fail without that advice and record only the
ancestry query. Positive controls preserve normal merge/resolve/commit ordering
and the local-commit advice for a proven newer upstream. No fixture performs a
real source merge, application build or push.

Validation with atpkg Python 3.14: 27 sync, 29 delta-tool, 27 feature-inventory and
17 GitHub-tooling tests passed (100 total). `bash -n alab/sync.sh` and
`git diff --check` passed. Only ALab-owned tooling, tests and this record changed.
