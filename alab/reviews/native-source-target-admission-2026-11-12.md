<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Let native admission create the source-cache fixture's target

## Finding and decision

The opt-in native source-cache test created its target directory before calling
Targo. The native cache owner correctly refused that existing unmanaged target
with exit 75, before compilation. Remove the redundant `mkdir`: native admission
must record a fresh target's owner before the build creates it. Reused targets
remain in place with the ownership established by their first build.

Keep the test's assertions unchanged. It still compares a stable source checkout
across two commits against two fresh checkouts, including an external path
dependency, unchanged source identities/mtimes, Cargo freshness and real output.
No production admission rule is relaxed, and no target receipt is fabricated.

## Validation

The unchanged fixture failed at its first build with the unmanaged-target
diagnostic. The corrected fixture passed in 3.213 seconds using the installed
Trust compiler (`trustc 0.12.0+g29ebb403d`, host `aarch64-apple-darwin`). All four
build commands used the explicit unverified, locked, offline lane and one build
job. No registry or Git dependency was fetched.

| Build | App fresh | External dependency fresh | Program output |
| --- | --- | --- | --- |
| Stable checkout, first commit | no | no | `13` |
| Same checkout, second commit | yes | yes | `13` |
| Fresh checkout, first commit | no | no | `13` |
| Another fresh checkout, second commit | no | no | `13` |

The stable target retained 999,424 allocated bytes across both builds. The
comparison target grew from 999,424 to 1,523,712 bytes. These are local fixture
observations, not a general performance or release-qualification claim.

An initial attempt to wrap the entire test in the compiler sandbox stopped
before compilation because the source-custody owner uses the fixed system
`/bin/ps` identity probe. Run this fixture's host-side owner as documented, with
private Cargo/Trust state and an outer command guardian. Its Rust inputs are
the test's own inert crates with only a local path dependency; this does not move production
source-custody work into the compiler sandbox or broaden that sandbox.

The retained fixture is
`$HOME/.aterm/.ubd/codex-native-optins-20261112-grqtbab5/fixtures/package-source-3zbl6s_6`.
It holds all four compiler logs and `native-cache-measurement.json`. The parent
directory holds the original refusal and successful test logs. Whitespace
validation passes. This closes the one native source-cache case omitted from
the broad workflow run; controller and cache-budget opt-ins remain separate.
