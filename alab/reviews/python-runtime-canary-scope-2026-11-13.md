<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Isolate the Python runtime grant from the package-store grant

The broad ALab tooling sweep ran 873 tests. Its reported failure was the
Python runtime sibling-canary test; three other families lacked the required
native runner artifact, and eight cases were skipped. This was not a passing
full suite.

The managed interpreter lives at `pkg/store/cpython/1`. The test creates its
sibling canary under `pkg/store/cpython`, while the active `ATPKG_BIN` asks
the production containment policy to grant read access to the entire managed
package store for toolchains and their libraries. The account-home canary was
denied; the sibling was readable under that independent store grant.

Set `ATPKG_BIN` empty only for this canary probe so it measures the exact
Python-prefix grant it names. Do not narrow the production toolchain grant,
skip the case for managed Python, or change the expected denial. The real
sandbox still runs the selected interpreter and refuses both canary reads.
All twelve containment-budget tests passed after this correction.

The native runner was then built with the source-selected Trust compiler in
the explicit unverified component lane to investigate the three setup errors
separately. That component build is not a certified application build.

With the actual runner supplied, all 19 unit-results tests passed. The lint-step
family still has fixture failures: its copied gate now reaches a certified
application stage that the old fake workspace does not model (starting with
the absent target directory). Those fixtures need an explicit update that
preserves the real gate; a fake success must not become qualification evidence.
The native context family's separate metadata call timed out after 60 seconds
before its probe compiled. Neither outcome is counted as a pass or attributed
to the canary change. The follow-up log is
`/tmp/codex-native-runner-families.log`; isolated successful replays are
`/tmp/codex-contained-budget-final.log` and `/tmp/codex-unit-results-final.log`.

The full initial observation is `/tmp/codex-alab-tooling-full.log`. It remains
a failed sweep plus specific passing replays, not an uninterrupted green run.
The owned child audit afterward reported nothing left running.
