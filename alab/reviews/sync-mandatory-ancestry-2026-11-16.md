<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Require known ancestry before sync mutations

The caller audit after `a8ece33d62` found that `run_sync` treated a failed
`--is-ancestor` read as a negative answer, then entered `in_wt clean -fdq` and
checkout. Other mandatory checks could similarly report rewritten history, suggest
aborting a merge, enter concurrent merging, or start a stepwise round after a Git
read failure. The recording regression reached the cleanup boundary for statuses
2, 128 and 137 before the fix; it performs no real cleanup.

Decision: share `alab_ancestor_check` in the ALab library. Execute the supplied Git
or worktree command and return only its 0/1 ancestry answers. Other statuses invoke
the existing fatal diagnostic, including inside `if`, negation, OR lists and while
conditions where shell errexit would otherwise be disabled. Apply it to mandatory
sync checks and explicit delta baseline admission. Remove the superseded local
status handling so callers share one failure contract.

The post-publication update of an optional, unchecked-out local main remains best
effort: an unknown answer there skips the local pointer write after the remote has
already landed. It does not authorize source mutation or change publication truth.

Regression fixtures execute the production sync functions with recording command
boundaries. Read failures stop the guarded path before its next cleanup, checkout,
fallback/retry or merge action, and before a subsequent publication attempt. Positive controls retain the existing paths for known
answers. Push-race errors no longer claim history was rewritten; delta read errors
no longer claim a baseline was proven unrelated.

Validation with atpkg Python 3.14: 31 sync tests, 30 delta-tool tests, 27 feature
inventory tests and 17 GitHub-tooling tests passed (105 total). Individual Bash
syntax checks passed for lib, sync and delta; `git diff --check` passed. Changes
are confined to ALab-owned tooling, tests and this record, with no application
build qualification claimed.
