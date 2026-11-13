<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Bind preservation policy to the requested clone

## Finding and decision

`retention.cleanup_allowed(repo)` passed the caller's Git environment to its
common-directory lookup. An inherited `GIT_DIR` or `GIT_COMMON_DIR` could direct
that lookup to an unmarked clone. It then allowed cleanup even though the
explicitly requested clone had an `alab-preserve-builds` marker. The same wrong
binding could unnecessarily preserve an unmarked clone.

Remove inherited `GIT_*` variables for this one read-only lookup, disable global
and system Git configuration, and suppress lazy fetch and optional lock writes.
The requested repository and its registered common directory determine policy.
The existing behavior still preserves work whenever the marker exists or the
lookup cannot establish its absence. No marker contents are read or changed.
Linked worktrees and external common directories retain their shared policy.

## Validation

A private marked/unmarked clone pair reproduced the incorrect cleanup
permission. The new regressions failed before the fix in all six redirected
policy contexts and in the standalone CLI case. The corrected implementation
passed:

| Module | Passed | Seconds |
| --- | ---: | ---: |
| `test_retention` | 18 | 12.799 |
| `test_prune` | 11 | 8.483 |
| `test_local_update` | 16 | 12.801 |

The tests cover both directions of the marked/unmarked pair, directory and
common-directory redirects, the copied standalone script, linked worktrees,
external metadata directories, and the existing package/scratch cleanup paths.
Independent review found no issue with those ownership semantics.
No live build, package, account or preservation marker was removed or changed.
The observation concerns policy selection; the fixture did not delete live
user work to demonstrate the old result.

The failing baseline, passing logs and original reproduction are retained at
`$HOME/.aterm/.ubd/retention-git-context-20261112-doq2y6uc.noindex`. The five-file
manifest has SHA-256
`85902c78e4b21e1d8c83327da7d0d943b28e3c291e102d3150f97675df3af61b`.
This change does not alter source-worktree custody, which has its own validation
and cleanup rules.
