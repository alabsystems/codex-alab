<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Run the optional native package tests

After the complete tooling sweep, run the seven optional native cases
separately. All seven passed: the external-path source/cache fixture, four
native cache-protocol tests, and two native controller tests.

The source/cache fixture used the installed Trust 16387. Across two commits
with unchanged compilation inputs, the stable source retained its input
identities and both Cargo artifacts were reused. The fresh-checkout controls
rebuilt both artifacts. All four resulting programs printed the expected
`13`. This is a small controlled cache observation, not a Codex build-time
benchmark.

The other six cases used real development `atpkg` and `atpkg-publish`
executables built offline from aterm source
`6b9844f57cf` with its selected Trust 16387 and pinned astream submodule.
The tools were run from their owned worktree, never installed over the app.
The cache CLI reported `aterm-package-build-cache-v2`. Tests covered managed
cache reuse, default containment, incompatible-registrar refusal, rejected
unrelated settlement, and controller cancellation/return with live writers.
The cache family passed in 8.505 seconds and the controller family in 46.370.

A copied bundled CLI was not used: its signature check failed and its first
execution was killed. The copy remains in its owned scratch path; no signing
or installed-application change was used to bypass that failure.

The seven cases were skipped in the earlier full sweep and passed in these
separate runs. The filesystem's non-UTF8-name subcase remains unrepresentable.
No application certification or package adoption is claimed.

Logs, compiler observations, source/cache measurements, and native executable
hashes are retained in the clone's common Git directory under
`alab-validation/605fd17466ec28157952e0ed5b4a533923b83a41/native-package-20261113/`.
