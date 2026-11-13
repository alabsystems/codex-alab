<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Restore the native completion-order regression fixture

A broader TUI check at `8598668193` ran all 263 tests whose names contain
`alab_`. It passed 261, ignored one helper, and failed
`a_native_finish_waits_for_tui_catch_up_before_continuing_past_an_upgrade`.
The same test failed when run alone with the repository's 8 MiB test stack.

## Cause

The fixture creates plain `old-image` and `selected-image` files and never
executes them. Its purpose is to prove that a completed native turn is not
continued while the TUI is catching up to a queued upgrade handoff.

Package admission now requires an authenticated independent native host and
an exact native package entrypoint. The dummy target does not meet those
conditions. The fixture therefore continued the task before it could test
its intended TUI handoff. The failure was an obsolete fixture boundary,
not evidence that production should accept an unsigned dummy image.

The retained isolated witness is `/tmp/ac.7jXL9j`: the delivered-first
case reported `accepted_upgrade=false` and one continuation. Its private
control transcript contained no upgrade-admission request. The broader and
isolated logs are `codex-alab-tui-wide-20261111.log` and
`codex-upgrade-finish-isolated-20261111.log` under `$HOME/.aterm/.ubd`.

## Decision

Keep production admission unchanged. Adapt only the test to explicitly use
the existing `alab-tui` test-support backend for package preparation. That
backend retains real file identity and lease ownership without claiming
signed-package admission. The test still runs the actual native turn,
thread manager, completion delivery, upgrade decision, and TUI readiness
checks. Both completion orders must still accept the handoff without
sending an extra model request.

The override is compiled only under `test-support`, installed through an
explicit scoped Rust API, and bound to the exact fixture `Shared` state.
No environment variable or production config can enable it. The fixture
must assert that preparation used it, and dropping the guard removes it
and releases any retained execution resource.

The inert target now uses `7/bin/codex`, matching the structural checks of
the existing receipt implementation. During adaptation, the preparation
diagnostic exposed the old filename as an invalid receipt target; those
checks were retained. Failed fixture outcomes now include the native update
status so a future admission change is visible at the assertion.

A separate review found a cleanup interleaving in the new helper: releasing
the registration lock before taking the retained execution would allow a
new registration to have its execution removed by the old guard. Cleanup
now captures the old execution while holding the registration lock, then
releases the lock before dropping that resource.

## Validation

The corrected isolated completion-order test passed, including the
delivered-first and held-completion cases. The broader `alab_` TUI run then
passed 262 tests with one helper ignored and no failures. The source's
production CLI dependency graph, queried with only normal/build/feature
edges, contains no `codex-alab-tui/test-support` feature.

After the cleanup-race fix, the scoped helper unit test passed, including
foreign-session rejection, overlapping-registration rejection, failed
preparation counting, and cleanup of retained execution.
The complete `alab-tui` library run then passed 353 tests with four ignored
and no failures. The native completion-order test and the 262-test `alab_`
TUI run also passed again with the final source.

The native test binary was built using Trust's unverified lane with
`--locked --offline`. Logs are `codex-upgrade-finish-layout-20261111.log`
and `codex-alab-tui-wide-fixed-20261111.log` under `$HOME/.aterm/.ubd`.
The scoped helper result is in `codex-upgrade-fixture-guard-20261111.log`.
Final-source logs are `codex-alab-tui-crate-final-20261111.log`,
`codex-upgrade-finish-final-20261111.log`, and
`codex-alab-tui-wide-final-20261111.log`.
These tests prove the ordering fixture works with explicit inert package
preparation; they do not prove live package signing or installed adoption.

The delta helper could not run because this clone has no upstream remote
or tracking ref. The only upstream-owned edit is the marked development
dependency enabling the fixture feature; the implementation and tests
remain in ALab-owned paths. No upstream lockfile entries changed.
