<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve a committed model stop when its completion is interrupted

The cancellation audit found no loss at successful goal reactivation: native
resume reloads persisted Active goals, and transient native admission failures
retain their retry. It did find a separate boundary after a model stops a goal.
The goal writer commits Blocked and publishes its exact call marker before Core
runs PostToolUse hooks. Interrupting a pending hook produces an Aborted lifecycle
completion. The recovery observer consumed the committed marker but rejected that
outcome, leaving the saved goal blocked without recovery custody.

Decision: also retain a stop reported through Aborted when, and only when, the
native writer already confirmed that exact thread, turn and call committed it.
This is a known completed database mutation, not an ambiguous write. Keep Failed,
Blocked and unsuccessful completion behavior unchanged. An interruption before
commit provides no marker and no recovery. Paused goals likewise provide none.

An ordinary Esc interrupts a turn in Cruise/Land; only an explicit goal pause
stops the objective. Retaining custody does not bypass the existing human grace,
policy hold, person-stop count, current-runtime, exact-goal or budget checks.
Source evidence remains distinct from permission to reactivate the goal.

Native regressions run the real goal tool with a trusted scratch hook, wait for
PostToolUse to start, then interrupt through Core. They inspect the persisted
goal and actual Aborted lifecycle outcome before checking same-goal recovery.
Negative controls cover pre-handler interruption, model Paused, and an explicit
person pause while the post-tool hook waits. No production hook configuration or
permission profile is changed.

Application deployment remains deferred at the independently reproduced compiler
certification limit recorded in the original recovery review.

An opaque backend HTTP 400 interrupted the assistant during fixture preparation.
Recovery first inspected the worktree and owned process state: the fixture edits
were present, production behavior was unchanged, and no validation was running.
The next action was therefore the pending RED run, without duplicating edits or
restarting a healthy agent.

Validation on `55e6e8eaf7c54a60fc61301451823fcb0770e760`:

- RED reproduced the lost binding after actual HookStarted, committed Blocked and
  native Aborted delivery: the positive case failed; all 21 other focused tests
  passed, including the three new stop controls.
- GREEN passes all 22 focused tests in 2.12s. The complete autopilot library passes
  1,002 tests with four intentionally ignored, zero failures, in 226.63s. Native
  artifact and runtime receipt collection passes. The canonical 65,536 file limit
  and selected Trust 15698 protected test lane were retained throughout.
- Canonical all-target Tippy passes in 2m15s with zero ALab findings and 41 upstream
  diagnostics set aside. Certified CLI construction then exits 2 at the unchanged
  proc-macro TCB restriction, without authenticated proof inventory. No compiler,
  profile or proof exception, unverified shipping build, or package installation
  was used. Runtime tests use the explicit unverified lane and make no proof claim.
- Independent boundary review approves only successful completion or Aborted with
  an exact committed marker; all other outcome behavior remains unchanged. Trust
  formatting and Git whitespace checks pass. Runner and hook children fully exit.

Evidence: `~/.ubd/codex-goal-committed-abort-red.log`,
`codex-goal-committed-abort-green.log`, `codex-goal-committed-abort-full.log`, and
`codex-goal-committed-abort-gate.log` in the same directory. The final native runtime
receipt is in this worktree's
`codex-rs/target/alab-goal-full-libs.E1DU74/collected.json`.
