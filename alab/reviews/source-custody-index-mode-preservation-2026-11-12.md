<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve intent-to-add and executable-mode work in package sources

## Findings and decision

A deleted intent-to-add path remained in a package source's real index, but the
default cached diff omitted it. The fresh comparison index also saw no file.
Source validation accepted the generation and reused it for the next commit.
The intent-to-add entry survived that checkout; ordinary Git refused the separate
fresh-worktree removal attempt. No deletion was observed for this case.

A separate fixture changed a tracked file from mode 0644 to 0755 while
`core.filemode=false`. Validation missed the executable-mode change, and Git's
final worktree removal also ignored it. The source directory and its registration
were removed. This was a mode-only change, not an observed loss of edited bytes.

Include intent-to-add entries in the real-index comparison. Require executable
mode comparison both when validating fresh worktree content and when Git performs
its final removal check. Leave the existing filter suppression and no-force
removal policy in place. These changes do not modify the caller's Git config or
adopt an existing source with uncertain ownership.

## Validation and limits

Three real-Git regressions failed against the old implementation: deleted
intent-to-add work and mode-only work were reused, and a mode change injected
after validation was deleted by the final removal. All three passed after the
fix. Reuse checks retain the old receipt, Git metadata, source content and index
entry as applicable, and obtain a different generation from the same pool. The
removal check executes Git's actual safeguard and retains the changed source and
registration.

The full source-custody module ran 30 tests in 130.188 seconds: 29 passed and the
one native compilation opt-in was skipped in that run. Independent read-only
review found no correctness or scope issue, and
`git diff --check` passed.

The focused before/after logs and fixture directories are retained under
`$HOME/.aterm/.ubd/source-custody-repair-20261112.0oRdEy`.
The initial independent reproductions are under
`$HOME/.aterm/.ubd/source-ita-evidence-179asbp4.noindex` and
`$HOME/.aterm/.ubd/source-mode-evidence-8j8iij7d.noindex`.
These are local source-custody checks, not release qualification or installed
adoption.

## Native cache follow-up

The previously skipped native test was then run explicitly on the composed
changes. It passed in 3.191 seconds through the host-side command guardian with
private Cargo/Trust state. All four fixture builds used the locked, offline,
unverified Trust lane. The compiler was Trust 0.12.0, commit
`29ebb403de7a102850d6cb10a62d5cc3baf723ed`, on `aarch64-apple-darwin`.

The stable source and external path dependency were reused across commits:
both units were fresh on the second build, unchanged source identities and
mtimes matched, and the target stayed at 999,424 allocated bytes. Both fresh
checkout controls rebuilt their units. Every built program printed `13`.
This confirms normal native reuse still works after the preservation fixes;
it is not a general performance or formal-proof claim.

The log, exact invocation, result and compiler measurements remain under
`$HOME/.aterm/.ubd/custody-native-20261112-dvhq5e0a.noindex`; the selected evidence
manifest has SHA-256
`fafeb28fec847928b8701863f64b9a2a396b8e7a006cadb8fc429a08299dfcfa`.
