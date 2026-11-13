<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep controller sources and Git metadata under creation custody

## Findings and decision

At `84004fdead048821a1c470fca6e7cf08b82c5768`, the disposable update controller
relied on ordinary Git status for fixed-source admission and cleanup. Four private
fixtures hid a changed source through assume-unchanged, skip-worktree, a stale
stat cache, or `core.filemode=false`. Status was empty, fixed-source admission
accepted the source, and cleanup removed the changed controller. A separate
fixture added a private `refs/worktree/owner` ref: status stayed empty and cleanup
removed both its source and registration. The explicit worktree-lock control was
already preserved by Git's ordinary removal safeguard. Deleted intent-to-add
work was also already refused in this updater; it was not another observed loss.

Reuse the existing package-source custody implementation instead of adding
another cleanliness observer. Allocate private nonindexed scratch outside the
active checkout, record the exact empty registration before checkout, and admit
only the known checkout transition. Keep that original binding through public
dispatch, guarded execution and cleanup. Validate its source, commit, common
directory, filesystem identities, Git registration, metadata, index and fresh
worktree comparison. Unknown state preserves the source and its registration.
Failed creation keeps even an empty reservation rather than guessing whether a
partial registration can be removed.

The updater invokes the existing helper in an isolated Python subprocess with
its sanitized Git environment. This preserves the prior explicit-repository
boundary, including inherited `GIT_CONFIG_*` isolation, without changing global
process environment or package-source callers. The handoff is bounded to 64 KiB;
serialized operation arguments or results over 128 KiB are refused. A nonzero helper result
is a failure, and each operation has a 60-second deadline. Timeout is uncertainty
and preserves source; it is not proof that external hook descendants stopped.

The public fixed-source mode now requires this pre-checkout binding. Historical
private dispatchers and public dispatchers without it fail closed and need a
source refresh. Do not infer custody from an arbitrary already checked-out tree
to keep those dispatchers compatible. Original selected-checkout/prefix checks,
explicit retention holds, and the existing guardian lifetime contract remain.

## Validation and limits

The focused seven-test run passed in 18.196 seconds. Its real-Git regressions
confirm all four ordinary-status false-clean controls, refusal of the fetched
public entry, and preservation of source bytes, executable mode and index bytes.
Additional checks cover private worktree refs, a post-checkout hook that adds a
private ref before admission, clean source/registration removal, malformed or
mismatched bindings, and historical dispatch refusal. The prefix-change control
uses a valid binding so it still reaches the original selection guard.

Full local-update discovery passed 62 tests in 102.426 seconds. The initial
full run had 61 passes and one obsolete fixture assertion counting controllers
under `.git`; the successful source now lives outside the active tree. After
correcting that assertion, the five disk tests received an additional private
TMPDIR inventory check and passed separately in 7.635 seconds. Those checks
inspect both actual scratch leaves and Git registrations, so an unregistered
failed reservation cannot disappear from the assertion. Production code stayed
unchanged throughout the successful full run and this final focused check.

The original reproductions are retained under
`$HOME/.aterm/.ubd/controller-cleanliness-evidence-skvkbcmd.noindex` and
`$HOME/.aterm/.ubd/controller-metadata-evidence-po97nqok.noindex`.
Validation logs, commands and source hashes are under
`$HOME/.aterm/.ubd/controller-custody-validation-20261112-xgsu81df.noindex`.
Independent read-only review found no production blocker. The tests use local
Git repositories, inert package-builder fixtures and mocked aterm selectors;
they perform no native build, real install, remote update or release
qualification. Successful cleanup still relies on the existing public
guardian's exit contract; this change adds no claim about independently
observing another guardian's process group.

## Final composition

The combined custody and channel-status changes passed all 67 local-update and
public-job tests in 119.891 seconds. The first combined run had 65 passes and
two test errors: composition had removed the `errno` import still needed by the
status writer's descriptor-close assertions. Restoring that import fixed both;
the final full run had no failure, error or skip. Independent comparison also
confirmed that each topic's production functions and critical tests survived
composition.

The two run logs and tested-file hashes are retained at
`$HOME/.aterm/.ubd/update-custody-composition-20261112-r5upot93.noindex`.
The evidence manifest SHA-256 is
`a86b6577cb8143a45c33fdbd956129bb7b7d5e676c9ff99e0e5a55f3ae8453d6`.
