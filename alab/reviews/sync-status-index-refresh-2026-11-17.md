<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep sync status from refreshing the Git index

Decision: set `GIT_OPTIONAL_LOCKS=0` only on the idle-worktree status call.
`--status` observes the repository and sync state; it must not persist Git's
optional index refresh. The shared `worktree_clean` and `in_wt` helpers also
serve mutating sync paths, so their behavior stays unchanged. The earlier
mandatory kernel sync-lock observation remains required.

At source base `74f6cb881fa141312d57906d74a6c44c5ef72c7f`, an actual public
`alab/sync.sh --status` invocation returned zero and reported an idle worktree,
but changed that worktree's index when its caller did not set optional locks.
The owned finite fixture first aligned origin/main with worktree HEAD, then
changed tracked-file mtime by two seconds without changing its contents.
Observation happened after that setup. Only index bytes changed; refs,
worktree registrations and tracked contents did not. A separate fresh fixture
with caller optional locks set to zero preserved the complete observation.
Both fixtures were retired and source hash/HEAD bookends matched. This is an
actual index-mutation observation, not an injected ENOSPC or native-admission
claim. The existing fixture's blanket optional-lock setting had masked this
public caller case.

The production change is one scoped shell assignment in the status branch.
It exports zero through the function and command substitution to Git, without
changing caller configuration or the shared helpers. Git read failures still
reach the existing explicit fatal error, including under the negated condition;
they cannot become an idle or dirty answer.

Two maintained regressions exercise the public entry with optional locks unset
and explicitly set to one. Each has an idle, metadata-stale worktree and checks
the complete before/after checkout, refs and index observation. Fixture setup
and observation keep their existing optional-lock setting. Both new tests fail
on the original source, with two assertion failures, zero errors/skips and
identical source bookends; their process exits one.

Managed Python 3.14 with `-I -B` passed all **95 unique methods** across the
18 complete `test_sync*.py` modules in **20.087 seconds**, zero failures,
errors or skips. Source/test hash and HEAD bookends matched. The verbose log's
95 test IDs are unique. `bash -n alab/sync.sh` and `git diff --check` passed.
Original sync source SHA-256:
`daba006f7919487ad6355ee378ecc48192100586d5ad0d20714c18f01aff804a`;
candidate:
`7f072d5550fbe22d0d13574102af8ff9c64a657f8a8cdfebb5ca4aab7534b030`.
Regression test source:
`b2ecdece9d2ca8fc18ae4c71aed802276d694fe05053b47e95f473f3c59602b5`.

Independent review approved the actual production/test diff. Two additional
controls ran the literal new negated assignment against the existing helper
with Git status 128 and empty or partial output: both exited one, recorded only
the status read, and never reached fallback. Their source hashes matched.
These controls are separate from the 95 unique repository methods.

Evidence is retained under
`$HOME/.codex/.ubd/sync-status-index-20261117/`: the canonical public
reproducer, regression-baseline before/result/log records, candidate before,
after/log and unique-ID records, and `smoke-fatal-read-review.json`.
The suite includes owned Git/Bash fixtures and its existing macOS resolver
containment fixture with an inert resolver. No Rust compiler, native unit
runner, application build, package or native qualification was run or claimed.

Before committing, the private branch fast-forwarded to incoming main
`36b331355cc5cc9c133968249bd77c039608d1b5`. Its changes are the requested packaged model pin,
the corresponding feature-policy text and their review note. All selected sync
source/test input hashes remain identical to the passing run; these files do
not consume the changed model policy. This alignment does not relabel the run
as having executed at a different HEAD.
