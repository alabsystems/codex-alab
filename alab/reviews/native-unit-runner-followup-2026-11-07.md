<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Standalone native unit adapter follow-up

Source checked: `cf88043be53c73afc7a5944afae45fb67f6bb0c3`. The standalone
`alab-unit-runner` now builds with the exact signed Trust build 15698 named by
`trust.lock`, under a native package lease and the unverified engineering lane.
The default Trust build 16387 stayed unchanged. Cargo's lockfile and production
sources stayed unchanged.

Populate missing Cargo Git caches from their exact locked commits. Submodule
URLs and commits come from the pinned parent's `.gitmodules` and gitlinks.
GitHub transfers use Agh; declared public Googlesource URLs use ordinary Git.
Fresh Googlesource caches are shallow pinned fetches, not full-history archives.
Respect the published `tools/win` `update = none`. No mirror substitution,
compiler override or admission-policy exception is needed.

Canonical component results, zero skips:

- 38 lint-step fixtures, including maintained macOS containment, native adapter
  preparation, pristine attribution, disk refusals and read/write boundaries.
- 17 unit-result fixtures using the actual Cargo-reported adapter executable.
- Three real-Cargo/native-entry tests: process and signal/descriptor context,
  existing runner conflicts, and missing-receipt refusal. The Python-startup
  negative control changes the native context as expected.

The first lint invocation omitted `ATPKG_BIN` while sanitizing the environment;
its existing sandbox correctly denied managed Git and Python package files.
It also placed fixtures below `.noindex`, selecting the already-nonindexed
layout rather than the plain-target layout that one test explicitly expects.
Preserve the declared tool-root variable and use a short private scratch path
below `/tmp/.ubd` without a `.noindex` ancestor for that fixture run. The
corrected invocation passes all 38 cases with the sandbox unchanged. Native
process evidence uses a credential-free environment and is retained with the
actual adapter/probe hashes and compiled binaries.

These results close the standalone adapter prerequisite recorded in the Python
launcher review. They are not a full application/project gate, verifier proof,
package-controller qualification, release promotion or benchmark result.

## Package-source reuse fixture follow-up

The opt-in real Trust package-source fixture also passes on source
`6e6d0c0881c655af72c15af058c7e791f0744f9b`, using leased Trust 15698. Its stable
checkout crosses two commits without changing the device, inode or mtime of
any compilation input. Both app and external path-dependency build units are
fresh on the second build. Two fresh-checkout controls rebuild both units.
All four native executables return the expected `13` result.

This closes the previously skipped functional source-cache fixture. The
fixture's recorded timings and disk sizes are not a performance comparison,
and its synthetic custody keys do not qualify the protected package-cache or
controller APIs. No production behavior or admission policy changed.


## Native producer and package API follow-up

The documented missing-producer build now succeeds from Aterm source
`2873e2cd171462917db95e4b1b722c5fcd0b330d`, with its exact Astream gitlink
`a9f174b8565ebc6c58db419d5b73e983da3f8a34` and pinned Trust 16387. Both
`atpkg-publish` and `atpkg` were built offline in the test profile, using the
unverified engineering lane, and run at their actual build paths. The installed
app and default compiler were not replaced. The native producer's
`toolchain run --repo DIR -- /usr/bin/true` preflight authenticated and leased
Codex's exact Trust 15698 payload.

On Codex source `cff2e50e836d7e43b6b9ae0c9be73b10f40358fa`, both previously
unrun opt-in modules now pass: two native controller tests and four native
package-cache tests, zero skips. Controls cover SIGINT/SIGTERM/SIGHUP writer
retirement, the returning-shell negative control's retained custody until
explicit release, stable-source build-unit reuse, default containment,
unrelated settlement refusal and registrar compatibility without an authority
upgrade. The real native tools' hashes stayed unchanged across the run.
Every fixture and transcript is retained. No admission rule, compiler override
policy or protected profile was weakened.

This closes the two explicitly deferred native module checks. The aggregate
record now covers all 49 Python modules through distinct scoped runs; it is
not one simultaneous full-suite run, a full application/project gate, a
compiler proof, a benchmark measurement or release qualification.
