<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Do not turn failed stepwise reads into merge plans

The stepwise audit after `d8f6d9694a` found three linked error boundaries:
`merges_unattended` treated Git probe/index failures as ordinary conflicts and
ignored the status of `ls-files` inside process substitution; the planner ignored
frontier/range read failures; and its caller ignored planner status inside a
here-string. A partial allowed-path listing could even produce a clean result
while the read failed.

Decision: preserve known merge-tree 0/1 outcomes, but stop on probe, index or path
read failures. Capture and validate Git output before comparing paths. Check sort
and comparison statuses and remove the owned probe index on failure. Extract the
tree line without a truncating pipeline. Read the frontier and complete commit
range explicitly, reject an empty range, and check planner status before decoding
its output in the stepwise caller. Normal all-clean, all-conflict and binary-prefix
plans retain their existing behavior.

With a production-like nonempty merge configuration, the original regression
reported seven failing subcases before implementation: probe/index/path reads,
frontier/range reads, an empty range diagnostic and a failed partial plan accepted
by the caller. Recording fixtures now verify no clean result or usable plan from
those failures, no subsequent planning after a native probe failure, and cleanup
of the owned index when path processing fails.

Two additional real-Git scenarios exercise the production probe in temporary
repositories: uncovered conflicts require a resolver, fully covered conflicts are
unattended, and an already integrated target is clean. Each probe leaves the
original repository index byte-identical, leaves a clean worktree and removes its
owned probe index. No fixture merges or publishes this source checkout.

Validation with atpkg Python 3.14: 41 sync tests (including 10 stepwise read/probe
scenarios), 30 delta-tool, 27 feature-inventory and 17 GitHub-tooling tests passed
(115 total). `bash -n alab/sync.sh` and `git diff --check` passed. All changed
paths are ALab-owned tooling, tests or this record; no application build is claimed.
