<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Check qualification inputs through a fresh Git index

## Finding and decision

The qualification recorder trusted ordinary `git status` before and after the
gate. A tracked gate or source file marked `assume-unchanged` or `skip-worktree`
could change while status stayed clean. The recorder could execute replacement
gate bytes, get exit zero, and qualify the original committed source.

Rejecting index flags alone is insufficient. With `core.trustctime=false`, a
same-size edit and restored mtime can remain undetected. An earlier status read
can cache that changed metadata against the original blob, so restoring normal
stat settings also fails. Separately, `status.showUntrackedFiles=no` can conceal
new candidate inputs. Fixture-only reproductions confirmed these outcomes.

Compare the real staged index against the captured commit, explicitly including
intent-to-add entries. Then seed a private temporary index directly from that
commit and use Git to check the working files afresh. Disable split-index and
sparse-checkout behavior for this temporary operation, retain normal stat/mode/symlink
checks, and explicitly include untracked files and submodule changes. The
existing Git helper accepts an explicit index path after removing inherited
Git environment variables. No user configuration or real index is modified.

This preserves Git's existing filter and ignored-file semantics. A matching
working file can still qualify when its real index has an exemption: the private
comparison checked it independently. The observation remains a local Git-state
check, not a byte-level attestation or a replacement for the native gate.

New observations use schema 2. The reader refuses schema 1 because its records
may have used the vulnerable cleanliness check. Keep those logs as historical
evidence; do not silently upgrade them or infer current qualification from them.
The [subsequent root and child-source review](qualification-root-and-children-2026-11-12.md)
adds those bindings and advances current observations to schema 3.

## Validation

Against the old implementation, the new flag tests failed all six subcases;
the relaxed-stat and hidden-untracked tests also reproduced their failures.
The final implementation passed:

| Module | Passed | Seconds |
| --- | ---: | ---: |
| `test_qualification` | 21 | 16.289 |
| `test_upstream_intake` | 9 | 3.825 |
| `test_upstream_status` | 15 | 10.927 |
| `test_upstream_gate` | 18 | 26.176 |

The regression cases cover changed gate/source bytes, exemptions introduced
during gate execution, a poisoned stat cache, hidden untracked inputs, staged
changes with a reverted working file, deleted intent-to-add entries, and paths
containing tabs/newlines. A successful run with matching exempt paths and split
index configured preserves the real index bytes and creates no extra shared
index or retained temporary index directory. Old or malformed record versions
cannot populate the source-status report's `qualified_sha`.

Independent replay of five retained bad states reported `clean: false` for all
five and confirmed each real index was byte-identical afterward. Independent
review found no blocker. Whitespace validation passed. No native compiler or
full release gate ran for this Python-only change. A separate attempted snapshot
of the main checkout lacked its required `upstream/main` ref; no ref was invented
and no qualification or performance result is inferred from that attempt.

The seven-file evidence manifest is retained under
`$HOME/.aterm/.ubd/qualification-fresh-index-20261112-i9cw_m44.noindex`, with SHA-256
`4f8ff20d58176021096d35d78458fc431ff59423a9e0ab8bbfa3ecbb3823a143`.
It includes the failing baseline, final test logs, and independent replay
observations. The earlier exploratory attempts remain separate retained logs.
