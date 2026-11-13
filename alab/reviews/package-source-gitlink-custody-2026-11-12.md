<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Observe gitlink children and preserve their cleanup boundary

## Findings and decision

At source `2268efee92`, a file added beneath an uninitialized committed gitlink
was invisible even to `git status --ignore-submodules=none`. Source validation
accepted it, and the real guardian's ordinary worktree removal deleted the file,
source and registration. This reproduced both before and after the worker's
return request. Separately, configured submodule ignore settings concealed
modified initialized children and a staged gitlink change from admission. Git's
final safeguard preserved the initialized and staged cases; those reproductions
do not establish deletion for them.

Observe each committed gitlink independently. Missing or empty uninitialized
directories remain valid ordinary checkout state. Nonempty paths must be real,
contained initialized Git roots at the exact committed child revision. Check
their real staged index with explicit submodule comparison, then compare content
against a fresh private index and recurse. Parent status ignores child reports
only because this independent observation replaces them; it cannot invoke an
uncontrolled child filter. Child observation is inside `fresh_status`, so the
legacy manifest/lock stamp checks also enforce it while allowing their known
parent-file changes.

A parent receipt has no child creation or deletion custody. Refuse removal of
every source whose recorded commit contains a gitlink, including empty
uninitialized directories. This prevents cleanup from depending on Git's blind
spot or deleting child state added after a previous observation. Do not force
removal, initialize or fetch children, or expand ownership of their Git metadata.
Existing parent-registration checks still apply to normally initialized modules.

## Validation and limits

Eight focused methods passed in 22.149 seconds. Their controls cover empty and
clean initialized children, unchanged child index bytes/mtime, ignored tracked,
untracked and ignored child files, child index flags, staged child work, wrong
child HEAD, nested hidden work, staged gitlinks, callback suppression, stamped
parent files, and real guardian preservation of an uninitialized child's file.
The same eight methods against the original source produced 11 assertion
failures and two errors in 21.490 seconds. Subtest failures and changed cleanup
policy account for multiple failures within individual methods.
The two errors reflect policy differences rather than additional data-loss
findings: the old observer refused a missing uninitialized directory, and Git
raised its own runtime error when asked to remove an initialized submodule.

An independent read-only review found no blocker. `git diff --check` passed.
No native build, live package cache or account registry was used. The initial
private reproductions are retained under
`/private/tmp/alab-gitlink-custody-audit-39nzngk0.noindex`; focused commands,
original-source runner and logs are under
`/private/tmp/alab-gitlink-checks-v212p3lr.noindex`.

A redundant pre-composition full module run was interrupted so the combined
symlink/gitlink changes could receive one full run. Its partial log is retained
without a full-suite result; a process check found no remaining command naming
any of that run's fixture roots. These are local source observations under the
existing guardian model, not a lease against unrelated concurrent host writers
or a new ownership claim over child repositories.

## Composition and controller verification

The combined implementation includes the
[symlink type checks](package-source-symlink-types-2026-11-12.md). An additional
fixture replaced a committed symlink inside an initialized child while
`core.symlinks=false` hid it from ordinary parent status. Recursive validation
refused that changed child and preserved its file. The public-controller
regression separately failed against the original implementation and passed
after the fix: the controller refuses before building or selecting, and retains
the replacement file, index and registration. Its focused fixed run passed in
2.382 seconds.

The full package-source module ran 47 tests in 258.370 seconds: 46 passed and
one native compilation opt-in was intentionally skipped. All 69 local-update
tests passed in 185.761 seconds, all 12 cache-budget tests passed in 31.652
seconds, and all seven package-version tests passed in 1.692 seconds. That is
134 distinct passing tests. The 12 new focused methods also passed on system
Python 3.9 in 30.158 seconds. Independent review of the combined code confirmed
that both symlink overrides and the earlier filter protections remain intact.

Every combined run used runtime SHA-256
`205fda022b92f3db0103fdf5e878d17d98a01e08e5468116cbe4fcce53a00370`.
Logs and tested-file hashes are retained under
`$HOME/.aterm/.ubd/source-path-composition-20261112-ufplc8zv.noindex`.
The evidence manifest SHA-256 is
`7a6d42982863f144d8e1ec49677a231e91b8da9b11fdb81d17ca6d2767a36604`.
