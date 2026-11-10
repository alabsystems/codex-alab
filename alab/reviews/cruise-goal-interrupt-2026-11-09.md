<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Continuous goals survive turn interruption

Source `9b3f8b70718da22056b0cb8653c8e2102f109b14` fixes the TUI converting
routine interrupts into user-origin goal pauses. One `ALAB(autopilot)` hook
calls the ALab-owned policy using the atomic harness pair. Active/Passive
Cruise and Land preserve the objective; Off, stale mode words, and ordinary
Codex retain upstream behavior. Explicit goal pause and shutdown remain stops.

The native protected `alab/check.sh` gate exited zero with the committed
Trust15698 lock and managed Python3.14 selected through `ALAB_PYTHON`.
All ALab-package tests passed. Tippy found no ALab diagnostics (41 upstream
findings set aside). The three new widget tests passed, exercising16 continuous
interrupt/question cases,10 ordinary/Off cases and4 explicit-pause cases.
One other failure passed alone;17 remaining failures reproduced on pristine
upstream `3026a8e54e0f48c4d8074cd700f35b0697abc329` and were non-gating under
the existing attribution policy. This is not a claim that every upstream test
passed, or a formally verified Rust build. Compilation used explicit unverified
lanes. Tests check widget requests, not backend persistence/shutdown end to end.

Prerequisites were repaired without changing defaults or locks: build the native
atpkg-publish producer; install the absent signed15698 build beside16387;
select an installed Python >=3.11. The first gate's Python3.9 V8-preflight failure
is retained in `codex-cruise-goal-native-check.log`; the successful run is
`codex-cruise-goal-native-check-python314.log`. Producer source was
`f1127326f8630ffe503461f99f26bb5fbbcf52f2`. Dependency preparation used the
native, contained recipe in `alab/README.md`. No guard or compiler override.

This record adds documentation only after the source-qualified gate. Local
package build, smoke, selection and restart are separate subsequent operations.

Main advanced with quota recovery `7f25a813c7` during the full gate. Lossless
merge `8503c1dd5b` retained the pause-fix Rust files unchanged. The combined
source passed the changed quota library's18 tests, all3 pause regressions
(30 cases), and quota all-target/no-deps Tippy with warnings denied. These
merged checks used the same native admission and protected containment.
The record does not claim a second full unchanged upstream campaign.
