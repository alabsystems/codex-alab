<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Complete the native workflow opt-ins

## Decision

Run the seven native cases omitted from the [broader workflow
check](broad-workflow-regression-2026-11-12.md), using real native tools and
retaining their provenance. The source-cache case exposed a premature target
creation in its fixture; the [correction and before/after
result](native-source-target-admission-2026-11-12.md) landed as
`dc2398e53f3794cd31b376c804b51e349ea6183c`. All seven cases now pass at that
source. No further implementation change is indicated by these observations.

## Tool provenance

Build independent copies of `atpkg-publish` and `atpkg` from aterm source
`502447d29db289056d0bd576ae9c1ac6cebf9750`, with `vendor/astream` at
`5de1089c49de2edaad5fb5d03542091bf1dd1d41`. The source and submodule stayed
clean. The locked, offline build used the test profile and the explicit
unverified Trust lane. Private Cargo state received only the required public
dependency objects, archives and index entries.

Trust build 16387 has compiler commit
`29ebb403de7a102850d6cb10a62d5cc3baf723ed` and tree
`46ca692db799c27b4874df3de5c635ddb0845d32ff2d23cb2546e613dc5626f1`.
Its `trustc` identifies itself as 0.12.0 on `aarch64-apple-darwin`.
The frozen executable hashes are:

| Executable | SHA-256 |
| --- | --- |
| `atpkg-publish` | `9c47dbaabf60aa9380eca1a0f84656d6dabfb9bcd049dea3c17e8f4427c84545` |
| `atpkg` | `a18d9f6e25cdaaa84e7f2bcbcccc98c1beff222bd0e9101c48ce4675418ba389` |

Before using the copies, focused native qualification passed 30 tests:
20 project-toolchain tests, eight cohort tests, and the two retained cache/SDK
custody tests. One additional project-toolchain child entry was intentionally
ignored; its parent invokes it as a subprocess. Both binaries built, their
frozen copies passed `--help`, and their hashes matched again after the client
tests. This is focused qualification, with no formal-proof or full-release
claim.

## Client observations

| Module | Passed | Seconds | What it exercised |
| --- | ---: | ---: | --- |
| `test_package_source` native case | 1 | 3.213 | Stable source and external path dependency reuse across commits, compared with fresh checkouts |
| `test_package_controller_native` | 2 | 42.008 | INT/TERM/HUP retirement of writers; custody retained for a returning-shell control until explicit writer release |
| `test_package_cache_budget_native` | 4 | 8.872 | Native policy target reuse, default containment and settlement, unrelated settlement refusal, old/ambiguous registrar compatibility |

The controller fixture used the frozen producer and selected this codex
source's pinned Trust 15698. The cache fixtures used the frozen `atpkg` and
the physical Trust 16387 sysroot, with its SDK lease held by the producer.
The source-cache experiment used Trust 16387 as recorded separately. These
are distinct selections; the producer's build compiler does not imply which
compiler a consumer selects.

The fixture harnesses ran host-side and owned their native containment and
process lifetimes. The default-containment case kept the existing account-home
protection. No extra sandbox was placed around the entire harness. Compiled
inputs were the fixtures' inert local crates, built locked and offline.

All three unittest logs end in `OK`, with seven passed and no skipped cases.
The original outer shell exit codes were not retained; the recorded completion
evidence is the successful unittest footers plus `aterm ctl @self sys left`
reporting nothing left running. Independent review checked the returning-shell
fixture's retained stderr and release markers against its intended sequence.

Together with the earlier record's 723 passes, this covers all 730 enumerated
test identities across the documented source revisions. It is not a fresh
730-case run of the corrected source, an authenticated feature-evidence bundle,
the full native gate, or evidence of installed adoption.

## Evidence and cleanup

Evidence is retained under
`$HOME/.aterm/.ubd/codex-native-optins-20261112-grqtbab5`. It includes the
initial refusals, source measurements and compiler logs, controller transcripts,
cache receipts, frozen executables, and source/compiler/build provenance.
The 120-file `evidence-manifest.json` hashes the selected logs and fixture inputs;
its SHA-256 is
`6c2f050411880ada51776da4eefaa70f60d92dfa27db7aafba67f650164429f2`.
The frozen executable hashes are recorded separately in that provenance.

The temporary dependency build worktree was removed after preserving the
independent copies and logs. The experiment fixtures and the existing codex
native cache remain. No installed executable, account assignment or activation
policy changed.
