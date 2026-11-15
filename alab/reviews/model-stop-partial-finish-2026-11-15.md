<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Native goal completion paused partway through delivery

Continue the bounded experiment selected at landed `2eae46837a`. Its retained
future fixes forced sampling teardown after a committed model goal stop. The
existing native fixtures hold PostToolUse before completion is claimed. They
do not independently prove the later interval: the registry has claimed its
terminal outcome, one observer has started, and Autopilot has not yet received
that completion. The generic retained-future tests preserve partial polling,
but a native adversarial fixture should verify the actual observer ordering.

Decision: add tests in ALab-owned paths before proposing another behavior
change. Register a separate test observer before Autopilot, preserving existing
post-Autopilot observers and default harness registration. Hold the real native
Completed goal callback and independently hold a later streamed assistant item.
An actual Interrupt must destroy the sampling item's future while the claimed
finish future remains alive. Release only the finish gate, then require the
original Completed outcome, the same opaque native invocation, exactly one
post-Autopilot completion and original source before TurnAbort. Verify one real
goal-triggered continuation with original identity, objective, budget and usage.

Use an ordered callback log and each callback's own returned/drop guard. Do not
consume the pending marker, infer retirement from has_active_turn(), or drain an
event queue to prove a negative. Native lifecycle completion precedes the native
TurnAborted event. Include normal release and a committed native API pause while
delivery is held; a pause must defeat recovery without inventing Aborted or
replaying the normal callback. Release every owned fixture gate before awaiting
cleanup, and retain roots on failure.

Add a native failed-outcome control on an explicitly paused baseline. A real
invalid update_goal(active) fails inside the handler before mutation preparation
or SQL. Its claimed Failed outcome must also survive teardown exactly once,
without a model-stop source or automatic recovery. Native UserInput only reads
and associates active/budget-limited goals; it does not reactivate a paused row.
Recheck that baseline at callback entry rather than relying on the fixture's
initial setup alone.

Current production is expected to satisfy this contract. A green experiment
adds missing native evidence; it is not a newly discovered failure. An actual
failure requires diagnosis before production changes. Keep current goal epochs,
owner identity, history, policy, budget and explicit human holds intact.

Existing unavailable operations remain separate: certified application packaging
refuses the Trust in-process proc-macro boundary without authenticated proof
inventory; four background-terminal integration cases cannot apply a nested
sandbox in the maintained protected lane. Their actual failures are recorded
in the forced-teardown review. Do not bypass those boundaries or repeat the
unavailable operations merely to make this test-only iteration look green.

The four fixtures and minimal default-None harness slot are committed at
`6f132e1f1d3e6891b43ef7020d10c9c09c4b9f31`. Two independent read-only reviews
approve that exact source, including per-callback drop guards, native epoch and
invocation checks, single delivery, paused failure baseline, lifecycle order,
and gate release before shutdown waits.

Validation on that clean committed source:

- All four new native cases pass in 0.47s. Official artifact/runtime/summary collection passes; report `codex-rs/target/alab-goal-full-libs.qvL0xv/collected.json`.
- Full Autopilot library: 1,067 passed, zero failed, four ignored in 234.47s. This includes all 65 native model-block cases and the existing source, budget, policy, person-stop, restart, accounting, history and pacing controls. Official report: `codex-rs/target/alab-goal-full-libs.luBv46/collected.json`.
- Scoped all-target Autopilot Tippy completes with zero ALab findings. Its three Rust deprecation warnings are in diagnostics/src/lib.rs35, core/src/tools/executed_tool_calls.rs401 and core/tests/common/apps_test_server.rs568, each independently verified unchanged from integrated upstream ce3031018ef1. The existing protected controller runs `--locked --no-deps --all-targets -p codex-alab-autopilot --cap-lints warn`; actual diagnostics are inspected, not inferred clean from exit0. Compile time is 4m01s.

Logs: `~/.ubd/codex-goal-partial-finish-{focused,full,tippy}.log`.
Both original test controllers and the scoped lint controller fully exit before
this record is updated. No production, Core, TUI, dependency, upstream hook or
compiler/profile source changes were needed. Their unchanged suites and prior
canonical findings retain their original source attribution in the earlier
review; no repeated certified application build or nested-sandbox terminal run
is represented as new evidence.

Decision: existing production satisfies the native partial-delivery contract.
Land this regression coverage and preserve the existing implementation. There
is no native failure here to justify another completion notifier, outcome
translation, replay mechanism or permission exception.

Next selected experiment is claimed goal completion through real nonterminal
`CodexThread::suspend_turn_and_shutdown`. Existing suspension tests cover held
provider requests and an agent-wait output; neither holds a claimed native goal
callback before Autopilot. Suspension removes the original task without
TurnAbort and relies on the final thread-completion barrier before ThreadStop
and persistence closure, so this is a distinct boundary from these Interrupt
cases. A bounded fixture should poll the actual suspension concurrently, prove
sampling-item destruction while its result remains pending, release only the
finish gate, then require one unchanged outcome/source before ThreadStop and
Suspended with the original turn ID. Inspect the closed rollout for no terminal
event on that turn, and use a nonempty append expecting ThreadNotFound to prove
writer closure (an empty append is an intentional no-op). Assert no replacement
request, and no live native-runtime lookup after shutdown. Do not attempt to
resume work from the stopped owner or infer new cross-process authority. This
is a selected coverage experiment, not a demonstrated production defect.
