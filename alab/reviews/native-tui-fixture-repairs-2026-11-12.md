<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Deterministic native TUI fixtures

## Decision

The full TUI sweep recorded in `native-tui-wide-check-2026-11-12.md` found
six failures caused by fixture assumptions. Repair those assumptions while
retaining every existing assertion and expected snapshot. The implementation
lives in ALab-owned test support, reached through six narrow upstream hooks.

The saved-side-chat helper is called by four tests in one libtest process.
Wrap its exact inline snapshot in Insta's scoped `allow_duplicates!` facility.
Every call still compares the actual row to the same literal expectation.
The wrapper expands in the calling test crate, so the product gains no Insta
dependency. Its module is available only with `test-support`.

The startup footer normalizes its destination after rendering at width 80,
and the daemon fixture binds a native Unix socket. Give only those destinations
short private temporary directories. On Unix, check both lexical and canonical
root lengths against a 32-byte budget; this accommodates macOS's `/private/tmp`
expansion. Creation requests mode `0700` atomically through `tempfile`, and its
`TempDir` owns cleanup. An unavailable root returns an error. Other platforms
retain normal temporary-directory behavior. Rendering width, path replacement,
socket probing, and the assertions stay unchanged.

The worktree and lifecycle snapshots use a shared timestamp initialized earlier
in the suite, but their renderer reads the current wall clock. Freeze only their
synchronous rendering sections at the fixture timestamp plus 120 seconds.
The renderer's clock hook reads the real clock in product builds. Test-only
overrides are local to a thread; the guard cannot move to another thread.
Nested or out-of-order drops and unwinding restore the remaining scope.
Three tests exercise restoration and simultaneous threads. No asynchronous
boundary occurs while either snapshot render holds its guard.

## Ownership

The `qualification-fixtures` inventory includes the new support modules and
their hook topics. `alab/delta.sh --topics` was attempted before the changes;
this clone has no upstream remote, so that report is unavailable. No remote
configuration or upstream content was replaced to work around that limit.

## Support checks

Before the directory-permissions regression was added, the ALab TUI library
suite passed 357 tests with four ignored cases, including all three new clock
tests. Review then found that the initial short-root helper inherited tempfile's
default directory permissions. The new regression rejected its `0755` result.
After explicit `0700` creation, the focused test passes and verifies unique roots
and automatic cleanup as well as absent group/other permissions.

The logs are `$HOME/.aterm/.ubd/codex-fixture-support-tests-20261112.log`,
`codex-fixture-path-privacy-before-20261112.log`, and
`codex-fixture-path-privacy-after-20261112.log` in the same directory.
The failed regression was retained as evidence; it is not a passing result.

Independent review confirmed that the corrected permissions are set at creation,
the snapshot wrapper retains exact comparisons, and the clock scope cannot cross
threads. Whitespace checks pass. The complete feature-inventory command has the
same missing-upstream limitation as `delta.sh`; a direct check confirmed the new
paths exist, both new topics are declared, and no prior hook marker was lost.

## Full native sweep and focused replay

At `61a7475c3b56f2e29a491a149fd8d19d90b64b46`, tree
`d22eccc471dbeba4c91aa4b5319b458487336827`, the complete native TUI
library run passed 6,381 tests, failed four, and ignored six in 787.02 seconds.
All six failures from the earlier sweep passed. The new sweep is not a green
full-suite result.

One new failure was the same shared-timestamp issue in
`lifecycle_removes_background_and_current_tasks_without_losing_the_dashboard`:
only `2m ago` changed to `3m ago` in its confirmation snapshot. Add the second
synchronous clock scope described above; keep its expected snapshot unchanged.

The other three failures were setup/closure failures:

- `a_native_finish_waits_for_tui_catch_up_before_continuing_past_an_upgrade`
- `an_unprepared_target_reports_refusal_and_continues_native_work`
- `backend_sessions_poll_three_sources_and_isolate_closed_or_paused_streams`

The upgrade children failed before upgrade capture: their launch witness was
absent, and their private host recorded only the initial version query. The exact
connection error was not retained. The backend test reached its existing
five-second wait after aborting a proxy. Its test and polling implementation are
unchanged from imported upstream. Do not extend deadlines or change behavior
without identifying those causes.

After the full sweep ended and the second clock scope was added, all ten selected
tests passed sequentially in the same native test executable: the original six,
the lifecycle snapshot, and the three setup/closure cases. The run took 36.63
seconds, with no failures or ignored cases. Earlier isolated attempts made while
the full sweep still ran had reproduced the launch and backend deadlines; those
failures remain in the retained evidence. Successful later reruns do not rewrite
the full-sweep result or establish the exact cause of those timing failures.

Both native runs used Trust's unverified lane with `--locked --offline`, an 8 MiB
test stack, and the normal managed temporary root. The full run used four test
threads; the focused replay used one. Logs are
`$HOME/.aterm/.ubd/codex-native-fixtures-full-20261112.log` and
`codex-native-fixtures-serial-20261112.log` in the same directory. No expected
snapshots changed and no test was skipped to obtain the focused result.

The normal ALab TUI library also passes `targo tippy --locked --offline
-p codex-alab-tui --lib --no-default-features -- -D warnings`. This checks
the product clock path with `test-support` disabled. The log is
`$HOME/.aterm/.ubd/codex-native-fixtures-production-lint-20261112.log`.

## Publication composition

Main advanced to `75ca25512c` with destination input restoration and its
qualification. The fixture commit rebased cleanly. All twelve paths changed by
this topic remained byte-identical after that rebase; the intervening change
does not alter these fixtures or their clock and snapshot helpers. Retain the
tested component results without claiming a complete gate on the combined tree.
