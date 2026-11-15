<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Claimed goal completion during nonterminal suspension

Continue the coverage experiment selected at landed `7701772e18`. The prior
native tests prove claimed ToolFinish delivery through interruption. Existing
ordinary suspension tests hold a provider request, and exec suspension tests
cover unfinished history and an agent-wait output; none holds a claimed native
goal callback before Autopilot while the original sampling task is destroyed.

Decision: extend the ALab-owned native fixture before proposing any production
change. Real suspension removes the task without TurnAbort, cancels and joins
it, and reaches the final thread-completion barrier before ThreadStop and
persistence closure. Its public future waits session-loop termination. This
boundary is distinct from an interrupted turn's abandoned-completion drain.

Two cases hold the claimed Completed goal callback and a later streamed item:
the original committed Blocked source, and a committed/applied native API pause
before suspension. Poll one actual public suspension request concurrently,
prove the item's forced Drop while that request remains pending, then release
only the finish gate. Require one unchanged outcome/invocation, post-Autopilot
source delivery before the pre-Autopilot ThreadStop observer, and Suspended
with the original turn ID. The paused case must not gain a source.

Read the actual closed rollout with the exact thread ID and zero parse errors.
Require exactly one original TurnStarted and no terminal turn event. A nonempty
append must return ThreadNotFound for that exact thread, proving writer closure;
an empty append is intentionally a no-op. Native goal-owner lookup must be
unavailable afterward, with no extra accepted turn/provider request and the
same persisted goal identity, objective, status, budget and usage. Keep the
captured immutable binding as delivery evidence, never post-stop authority.
A retained State allocation is allowed even after its registered owner retires.

A default-None test-only thread observer preserves existing harness behavior.
The only new dependency is the already present workspace codex-thread-store
crate under Autopilot dev-dependencies, needed for an exact typed error check.
The lock change is one dependency entry in ALab's own package; no upstream
package/version is edited. Before this change, delta reports the derived lock
as +285/-2 and 433 modified upstream files. No new upstream hook is required.

Release all fixture gates before cleanup awaits. Accepted suspension remains
session-owned even if an assertion drops its caller; never send a second
suspend to recover a lost reply. Preserve policy, epoch, budget and human holds.
Existing production is expected to pass; a failure must be diagnosed before
changing that behavior. Previously recorded certification and nested-sandbox
terminal limitations remain deferred, without profile or compiler exceptions.

The fixtures and typed dev dependency are committed at
`ced742fd9a9a47b1cab16914ccbcff0a9dae6e5d`. Independent exact-source reviews
approve actual polling of the public request, pending-state evidence after
item destruction, pre-retirement source capture, original history and typed
writer error, optional observer defaults, and cleanup when the caller drops.

Validation on that clean committed source:

- All six native partial-finish controls pass in 0.75s, including the two suspension cases and four unchanged interruption/normal/failure controls. Official artifact/runtime/summary report: `codex-rs/target/alab-goal-full-libs.svu4JD/collected.json`.
- Full Autopilot library passes 1,069 tests, zero failed, four ignored in 224.60s. All 67 native model-block cases, including source/epoch, budget, history, policy, person-stop, restart and accounting controls, are included. Official report: `codex-rs/target/alab-goal-full-libs.NZzU0R/collected.json`.
- Scoped all-target Autopilot Tippy finishes in 57.27s with zero ALab findings. Three deprecated fetch_update spans are on unchanged integrated-upstream diagnostics/lib35, executed_tool_calls401 and apps_test_server568, verified against ce3031018ef1. The protected controller uses `--locked --no-deps --all-targets -p codex-alab-autopilot --cap-lints warn`; the actual diagnostics are inspected. No canonical certification retry or permission change is used.
- Post-change delta reports only the expected derived-lock increment to +286/-2; the 433 modified upstream files and their hook surface are unchanged. No upstream package or version is edited. The locked test/lint metadata accepts the one internal dev-dependency edge.

Logs: `~/.ubd/codex-goal-suspension-{focused,full,tippy}.log`.
All original controllers fully exit before this record is updated. Core/TUI
production and tests are unchanged; their earlier successful checks retain
those source identities. Known unavailable certification and nested-sandbox
operations remain recorded at their original sources, not described as passing.

Decision: current production correctly drains claimed goal completion through
nonterminal suspension. Keep the implementation and land these regression
controls. No new source authority, outcome translation, recovery from a stopped
owner or production workaround is justified by this green experiment.

Next selected coverage check is an accepted suspension whose public caller
disappears while the claimed goal callback remains held. Existing dropped-caller
coverage addresses prepared-child/checkpoint ownership, not this root public
operation with native goal delivery. Poll the original request until item Drop
proves native acceptance, verify it is pending, then drop only that caller.
Observe `wait_until_terminated()` without submitting another operation and
require native ShutdownComplete, emitted after ThreadStop and writer closure;
termination alone does not explain its cause. Reuse exact source/outcome,
unfinished history, typed nonempty append, owner retirement and no-extra-work
assertions. Do not call shutdown_and_wait before these checks, because its
Shutdown submission could mask abandonment; reserve it for failure cleanup
once every gate is open. Do not send a second suspend or claim a lost Suspended
reply was received. This is a distinct coverage gap, not a proven defect.
