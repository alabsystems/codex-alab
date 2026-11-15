<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Source custody after an unfinished native suspension

Continue the bounded investigation selected at landed `945cd2ab37`. The eight
native partial-finish controls prove that accepted suspension drains the
original claimed completion before retiring its owner, even when the public
caller disappears. They do not follow its saved source into a fresh runtime.

Decision: add two native successor controls before changing production. Reuse
the actual positive caller-drop suspension and its closed rollout. Capture the
published source through the real Pace record codec, preserve the home and
goal database, then drop the old harness and GoalService. Resume with fresh
Shared state, GoalService and native owner. Offer only the saved WallGoal
through the existing qualified-record callback seam; never carry the old
Binding, mutation epoch, invocation or pending completion marker.

The source contract does not require FunctionCallOutput or a completed turn.
It requires the original native TurnStarted, matching current goal and budget,
unspent allowance, fresh owner's initial mutation lease, and no later person
stop or conflicting native host/API update. A committed source may therefore
survive the unfinished history. Execution has a separate contract: Running
history supplies no ended point, and saved source supplies no carried act,
recovery or authenticated unfinished-turn admission. Import must remain idle.

The normal upgrade-test harness seeds a synthetic ended point and owed act.
Add a default-preserving test option that omits those for these successors,
so neither can manufacture permission to run. Require the actual running
history, absent original FunctionCallOutput, preserved goal identity and
usage, no active turn, no accepted turns and no provider requests before and
after evaluation.

For the negative control, save a source that was actually accepted, then
persist and apply a real API pause in the new owner before offering it. A
pause before the original pre-Auto finish would create no source to save.
Require both the durable history veto and refusal of the stale source; the
pause remains authoritative. The qualified-record callback is exercised here,
not the record writer's independent handoff-selection rules.

No production, upstream hook or dependency change is planned. Previously
recorded certified-build and protected nested-sandbox limitations remain
separate. Do not retry them or weaken their compiler/profile/permission
boundaries for this test-only investigation.

An authoritative open-issue read for selecting follow-up work was unavailable:
Agh returned HTTP 403, `forbid.permission`, `side_effect=none`, for the
repository's issue list. Defer that read without another identity, transport
or permission workaround; continue the local source-based investigation.

The first focused run at `cd2593ce6f` completed with nine passes and one
failure in 1.27s. It exposed an overly early test assertion, not a production
failure: `resume_scan` sets `scanned` after an unavailable read, while this
harness binds the thread manager after Core returns from resume. That attempt
may precede manager binding. API pause does not clear the runtime task. The
official failed report is `codex-rs/target/alab-goal-full-libs.XWH3iV/collected.json`.

Correct the test to prove the original task from actual raw history, and
require a restored runtime task only after the positive source import's
mandatory full history read. A negative stale-epoch rejection may correctly
return before adopting history. Keep every no-point, no-owed and no-work
assertion; never seed a task or invent a timing wait to make the test pass.

The correction is committed at `d960fbf82a`. Its ten focused native controls
pass in 1.03s, and the full Autopilot library passes 1,073 tests, zero failed,
four ignored in 205.91s. Official artifact/runtime/summary reports:
`codex-rs/target/alab-goal-full-libs.jamXf4/collected.json` and
`codex-rs/target/alab-goal-full-libs.8o3j9Y/collected.json`. Scoped all-target
Autopilot Tippy completes in 50.59s with zero ALab findings.

Main advanced to `90248de15e` while those checks ran. After their original
controllers fully exited, merge it as `6014bca5dc`. Incoming production changes
are confined to ALab's Recovery/InFlightRetry clock readers. The Pace source
codec and conversions are byte-untouched. The record-contract crate is dev-only
and the lockfile adds its nine owned lines; no upstream package versions or
features change. Reuse main's already recorded default/unified graph contract,
Core and TUI evidence for those unchanged inputs; validate our composition.

On clean composed `6014bca5dc`, the full Autopilot library passes 1,073 tests,
zero failed, four ignored in 234.36s. All 71 native model-block controls are
included. Official report:
`codex-rs/target/alab-goal-full-libs.QS6cBX/collected.json`. Scoped all-target
Autopilot Tippy completes in 57.41s with zero ALab findings. Each run's three
deprecated-fetch_update diagnostics are in unchanged upstream diagnostics/lib35,
executed_tool_calls401 and apps_test_server568, verified against integrated
upstream `ce3031018ef1`. Inspect actual diagnostics, not just exit status.

Logs: `~/.ubd/codex-goal-suspended-import-{focused-corrected,full,tippy,composed-full,composed-tippy}.log`.
All original controllers have fully exited. Optional build-memory peak
accounting could not update its protected cache; the official runtime receipts
and passing summaries were collected. This is test evidence, not application
certification. Prior certification and nested sandbox limitations retain their
original attribution and are not retried or bypassed.

Decision: production correctly preserves the genuine suspended source without
inventing execution authority and rejects the actual API pause. Land the native
controls with their default-preserving fixture seam. A separate source audit
found no create/update divergence: creation is always Active and subsequent
blocking enters the same validated update/resume path. Existing native tests
already cover that convergence; no additional permutation is justified.

Next improvement selected from the actual failed run: retain failure diagnostics
beyond outer containment cleanup. Harness TempDir retention cannot prevent
`alab_contained_scratch_cleanup` from removing its parent scratch. Allocate an
exclusive diagnostic leaf under the admitted target in the qualified unit
results path, and hand only the selected library/round's leaf to native tests.
After owned producer shutdown/flush, export registered fixture roots while
still contained, with bounded files/bytes/depth, anchored no-follow opens,
no overwrite, and an explicit copied/omitted/error manifest. Keep socket
fixtures in short scratch. Index the manifest as diagnostics, never proof.
Test that the export survives normal scratch cleanup, including outside-root
symlink and budget controls. Do not disable cleanup, add a profile exception,
or copy arbitrary paths parsed from stdout outside containment. This is
selected follow-up work; it is not implemented by this change.
