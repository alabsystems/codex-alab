<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Native context fixture owns its Cargo output layout

Follow the improvement selected at landed `f925d14a54`. The context fixture
sets an independent `CARGO_TARGET_DIR` but inherits `CARGO_BUILD_BUILD_DIR`.
An unrelated inherited build directory can therefore receive its native
executable, which the strict result preparer correctly rejects as outside the
fixture's declared target. The prior diagnostic validation required its private
controller to coordinate those two paths for the fixture.

Decision: bind both paths at the fixture boundary. Canonicalize the fixture's
target, set its own `build.noindex` child in the Cargo environment, and require
the discovered actual executable to resolve inside that child. Preserve the
production preparer/runner target guard, actual source-selected Trust compiler,
protected profile, original 60-second deadline, and all same-invocation process,
signal, descriptor and exact environment checks. Only test infrastructure changes.

Regression: supply an exclusive unrelated inherited build directory under the
already admitted target, with one sentinel. Run the existing three real native
context controls and require the foreign directory still contains only that
unchanged sentinel. Keep the directory persistent on failure so no new executable
is deleted as test scratch. Assert actual artifact placement, not merely an
environment assignment. Reuse the existing timeout-capture tests to preserve
their error/evidence behavior; no unrelated Core/Autopilot or application build
is warranted.

On original `f925d14a54`, the hostile-inheritance run executes three tests and
fails all three prepare steps in 3.541s. Each rejection names the real native
executable under `alab-context-foreign-build.974viZ`, outside the fixture target.
This reproduces the ownership bug without relaxing its guard. The private
controller's subsequent heredoc-based witness check cannot allocate its shell
temporary under the protected profile. Preserve both outcomes; pass that check's
Python code as an argument instead, without changing any permission rule.
Log: `~/.ubd/codex-native-context-layout-red.log`.

On clean corrected `5fa707a517`, all three real native context controls pass
in 1.469s with a fresh hostile inherited directory. The foreign directory
contains only its unchanged sentinel. The actual executable resolves under the
fixture's own `build.noindex` and matches its native runtime receipt. Exact
PID/parent/session/group, descriptors, signal state, inherited file bytes/offset,
and same-invocation environment comparisons remain active; cfg conflicts,
triple-runner precedence and the Python-startup negative control pass.
The four timeout-evidence controls pass in 0.003s, preserving partial/empty
output, original exception identity and completed-result behavior.

Green private evidence `alab-diagnostics-native-context.V67Xsv` has 33 files,
including `foreign-build-control.json`; independent read-only review verifies
its artifact/control/context assertions without printing environment values.
Red evidence `mtNXoe` has 13 files and the persistent foreign build directory
contains `.rustc_info.json`, `debug` and its unchanged sentinel, confirming the
pre-fix write into the wrong output tree. Logs:
`~/.ubd/codex-native-context-layout-{green,timeout-tests}.log`.

Decision: land the fixture-owned layout and actual-artifact guard. Both native
and pure tests pass on the committed source; all original controllers fully
exit before this evidence record changes. No production guard, runtime, timeout,
compiler/profile, dependency, application certificate or installed artifact is
changed. Previous application/nested-terminal limitations remain separate;
unrelated Core/Autopilot suites and broad lint are not repeated for this Python
fixture-only change.

Next useful verification selected: complete the full Python workflow that
previously ended with a native-context setup error. Run modules sequentially
in fresh isolated interpreters under the source-selected outer driver; their
containment self-tests own the original inner profiles. Run native-context
tests separately through the protected native lane with the unchanged deadline.
Leave unrelated native-package/cache/controller/failure-probe opt-ins unset and
report actual skips. Ordinary workflow success with opt-in skips is evidence
of that workflow, not named-feature admission or an application shipping gate.
No additional source defect is demonstrated and no Core/Autopilot replay is
justified by this next Python-only verification.
