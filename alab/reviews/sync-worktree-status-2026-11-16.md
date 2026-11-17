<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Require a readable status before clean/staged decisions

The worktree-state audit after `b89c626871` found that `loose_changes` masked status
read failures with `|| true`. Its callers also compared unchecked command output
with an empty string. Failed empty status reads could reach the gate or accept
new sync work; partial failed reads could instead recommend discarding changes.

Decision: replace the loose-output helper with explicit clean and fully-staged
predicates. Capture complete porcelain output and fail on read/classification
errors before returning a Boolean answer. Use those predicates at both staged
checks, the final clean check, idle status reporting and fresh-sync admission.
A valid dirty answer retains its existing recovery path; an unknown answer cannot
be inverted or swallowed by a caller's shell Boolean context.

Four production-caller regression subcases failed before the fix: empty and
partial failed reads in finish and fresh-sync admission. They now stop before a
gate or accepted-start action and emit read-error diagnostics rather than discard
or abort advice. Positive controls retain clean finishes, dirty-work exit 4,
fully staged changes, mixed edits and untracked-file classification. Fixtures
record source-operation boundaries without invoking actual merges or publishing.

Validation with atpkg Python 3.14: 51 sync tests, 30 delta-tool, 27 feature-inventory,
31 GitHub-tooling and 33 Python-runner tests passed (172 total). The six focused
predicate/caller tests passed again after final local-variable cleanup. Bash syntax
and `git diff --check` passed. Only ALab-owned tooling, tests and this record change;
no application build is claimed.
