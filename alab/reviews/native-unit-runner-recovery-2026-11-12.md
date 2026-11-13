<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Recover the local native unit-runner checks

## Decision and source identity

The recent Python workflow checks left 17 unit-result cases unstarted because
this machine had no prebuilt `alab-unit-runner` in the retained checkout. Build
that test adapter and exercise it locally instead of leaving the missing-artifact
condition as a permanent deferral.

The retained source was clean at
`b58c000564bd8fefa313bdbee5ed5e4108d986d7`. PATH selected a different Trust
payload from the committed lock, so the build used `alab_project_run` and native
explicit-source selection. The selector authenticated Trust 15698, tree
`9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`.
The existing containment profile, private Cargo home, cache-owner publication
and disk guardian remained in use. No compiler override, explicit native target,
source edit or installed-package replacement was used.

The local `atpkg-publish` producer was found through an actual aterm worktree
registration. Its observed SHA-256 was
`37200574a23e461117a014eb9618e28f63775ffce0a2b981fdff591403f56262`, unchanged after
the build. Its executable predates that worktree's current commit; an exact
producer source commit was not established. The successful native selection
and test executions are observations, not a new producer qualification claim.

## Private source-cache preparation

The first protected command stopped before the compiler because its log was
under the controller's read-only parent. Put the log in the already writable
target instead; do not relax the profile. Subsequent offline resolution exposed
the cold private dependency cache.

Seed only public source data required by the committed lock from existing local
caches. Copy checksum-verified crate archives and validated public sparse-index
entries; populate new private bare repositories using local fetches of exact
Git commits. Do not copy owner Cargo configuration, credentials, hooks, remotes,
or mutable extracted source directories. This prepared ten direct Git sources,
1,154 sparse-index entries covering the locked registry versions, and the 17
archive dependencies of the runner.

Cargo also resolves V8 while loading this workspace. Its 19 enabled submodule
commits were present locally and were seeded into private Git databases.
`tools/win` stayed uninitialized as the pinned `.gitmodules` declares
`update = none`. A first attempt to fill only the working checkouts did not
satisfy Cargo: it recreated the incomplete checkout. Preparing the bare databases
let Cargo perform its own normal checkout; no completion marker was forged.
All dependency-source transfers were local. No dependency fetch went directly to GitHub.

## Validation and limits

The runner built offline with the explicit unverified lane in 18.89 seconds,
exit 0. Cargo's JSON identified the real executable, whose SHA-256 was
`eb83cda1b09cd68e5c10fae21a27ee094565560c0d4e33a61d6e23e0ea982287`.
The source, lockfile and compiler pin were unchanged.

Under fresh leases for the same selected compiler and the existing protected
profile, all 17 unit-result tests passed in 5.306 seconds and all three native
Cargo-context tests passed in 4.877 seconds, with no skips. These include real
harness retirement, process-context preservation, signal/deadline attribution,
matching-runner conflicts and refusal of missing receipts.

The compiler warned that nested unverified-lane temporary transport could not
be created under its default temporary root, and its optional memory-peak record
could not be persisted. Both were kept as warnings; no broader permissions were
introduced. The explicit outer lane and commands completed successfully. This
work makes no Rust formal-proof, full release-gate or installed-adoption claim.

The test adapter, context artifacts and private source cache are retained for
future checks. No production runtime or installed Codex binary changed.

The lint-step fixture suite chooses its own containment per case. An initial
invocation wrapped the whole harness in an extra outer profile, changing the
intended setup and causing failures; that incomplete run was stopped and kept
as an invocation failure, not a product regression. All four observed failing
cases passed on unchanged source with the documented plain-Python invocation
and explicit runner path (two pairs, 23.260 and 39.707 seconds). Their individual
`contained=True` cases still exercised the actual sandbox.
