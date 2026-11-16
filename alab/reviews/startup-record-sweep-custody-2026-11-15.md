<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Startup record sweep retains the inspected publication

A startup sweep could delete a session record published after its initial
read, or pass the displaced record to its handoff callback. `sweep_with` read
the candidate and checked its process, invoked the verdict, then unconditionally
unlinked the original path for Take/Remove or called back immediately for Hold.
Production startup maps its own predecessor to Hold; obsolete records use Remove.
Both paths therefore needed an observation tied to the actual publication.

The fix lives entirely in ALab-owned code. The existing retirement quarantine
becomes a shared capture operation. The sweep moves a candidate into custody,
checks the observed inode, mode, size and modification time, rechecks its parsed
record and process liveness, then retires only that capture. Parsed equality alone
would miss an atomic replacement containing future fields this reader ignores.
Malformed candidates receive the same object guard before removal.

Hold restores the validated capture only into an empty original slot before
delivering its callback. A newer canonical record wins; the captured source stays
in quarantine and supplies no callback. Invalid captures are also restored without
overwriting a newer writer. The directory census is collected before captures
begin, so a quarantine retained by this sweep cannot become another observation
in the same sweep. The existing held-source retirement helper shares the custody
operation and retains its previous parsed-record matching semantics.

## Regression and verification

The first RED source `496a8eca39` ran three new regressions under the unchanged
implementation: all three failed at the expected removal-count assertion.
Extended RED source `30a8791469` includes Hold and an unknown future-field
publication. Its retained execution has four failing tests: stale Hold callbacks,
missed liveness changes, and removal of a valid replacement for malformed bytes.
Each function stops at its first failing case; RED does not claim every matrix
case reached an assertion.

Implementation and feature contract are committed at `847475145d`. The final
control at `ad12b4396a21476d09f4da91d68e7dd6acc8a946` publishes a new canonical
record during valid Hold quarantine validation. It proves no callback, no removed
record, the newer canonical intact, and exactly one retained old quarantine.
Five regression functions now cover 15 cases: canonical/temporary replacements
under Hold/Take/Remove, newly live identities, unknown fields, malformed candidates,
and the Hold restoration conflict. Existing session tests cover unchanged stale
cleanup, simultaneous relaunches and predecessor custody.

| Evidence | Result |
| --- | --- |
| Final committed source | `ad12b4396a21476d09f4da91d68e7dd6acc8a946` |
| Runtime command | `targo --unverified test --locked -p codex-alab-tui --lib session_tests` |
| Runtime result | 116 passed, 0 failed, 0 ignored; 266 filtered out; 13.21 seconds; exit 0 |
| Scoped lint | `targo tippy --locked --no-deps --all-targets -p codex-alab-tui --message-format=json -- --cap-lints warn`; exit 0; successful build-finished record |
| Lint ownership | Maintained `alab/lint.jq`: owned/spanning/failing/scripts empty; two upstream deprecation diagnostics set aside |
| Feature inventory | 91 hook topics, 14 features; ownership complete at final committed source; semantic qualification remains separate |
| Compiler | Direct stored Trust 15698 tools, matching this source's lock; `trustc 0.11.0+g20565c325`, commit `20565c3255adf151f9eb920e89cdf83c38f84abe`, aarch64-apple-darwin |

Tests and lint use distinct targets in the isolated sweep worktree. The first
all-target lint covered `847475145d`; the retained final lint and runtime run both
cover `ad12b4396a` after adding the Hold conflict test. No compiler overrides or
native admission bypass were used. This is ordinary unverified library execution
and scoped lint evidence. The initial native gate entry lacked a qualified
atpkg-publish on PATH. A later independent root probe recovered an existing
frontend and authenticated source-selected Trust 15698; that separate probe does
not relabel this bundle as application qualification. This bundle claims no
application certificate, package or installed adoption.

Raw logs, compiler identity, hook lines, feature inventory, diff and checksums are
under the common Git directory at
`alab-validation/ad12b4396a21476d09f4da91d68e7dd6acc8a946/record-sweep/`.
Its `index.json` SHA-256 is
`0a13aa7552daae96b5acb492e5f3b303bd3107cac4e592af488e745d0bb69db5`.
Final `tests.log` SHA-256 is
`5aed19e095e8ad152c2af937e330a989aac0ef62f998e9a8521a7ccacbf45c68`;
final `tippy.json` SHA-256 is
`14db9ea9343d6078086765b12a944ca87e0255bb53a13aecc60d550b7d6bb594`.

This guards startup observations. Later cleanup still retains only the parsed
held source; this patch does not claim to protect an unknown-field-only rewrite
after that completed observation. No current writer was found to make that
transition while keeping every known field unchanged. Existing later-cleanup
custody and restoration controls passed; no broader source semantics changed.
