<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# A duplicate block cannot close an unread source

The startup audit confirmed a gap after the earlier late-import repair. The
record writer prepares in the background, while native work may begin before
its sweep. A model can repeat Blocked before saved goal provenance arrives.
With no live or pending proof, its completion previously closed both import
latches. The later qualified source could never restore the original goal.

Decision: distinguish prior Active, Blocked and other statuses in the committed
native marker. Active-to-Blocked establishes fresh model origin. A genuine
Blocked-to-Blocked preserves matching known proof, or leaves an as-yet-unknown
source open without granting recovery authority. Other transitions close old
proof. No path reopens a closed latch or a person's stop tombstone.

Late proof also needs a stronger history veto. The existing person-stop reader
recognizes host user-goal instructions but does not reject intervening native
Paused/Complete status or goal edits recorded by the native host/API. Model
recovery must reject those matching thread events, including edits later restored
to their old values. Matching
Active and repeated Blocked events alone do not revoke the source.

The durable binding identifies a native turn, not an exact goal-update event.
Validation therefore starts at that turn's first start, as the existing human
stop check does, and fails closed on ambiguous same-turn pauses or edits. Picking
the latest Blocked event or using second-resolution timestamps could cross a
real hold. More precise adoption would require additional ordered native source
identity, not an inferred compatibility exception.

Native regression setup uses a fresh Shared/Autopilot and a genuine resumed Core
thread, retaining the original goal ID, database and rollout. Reusing the old
Shared would retain its import tombstone and misrepresent a process restart.
Proof is withheld until after the duplicate tool completes; no source means no
recovery. Pause, replacement and person-stop controls remain authoritative.

Fixture audit found that model GoalEventEmitter updates are notifications, not
durable rollout events. An intermediate fixture explicitly appended those model
snapshots and incorrectly described that as production behavior. The final
fixture removes that assumption: its historical pause calls the native
GoalService, appends the actual outcome item, then applies runtime effects, as
ThreadGoalProcessor::set does. The positive records no extra model events. The
history veto covers recorded host/API updates and host-annotated person stops;
model-only notifications are not durable evidence. Live tool completion
retirement and the current native goal checks remain separate guards. A model
request alone is not proof that its mutation committed.

The certified application-release operation remains deferred at the compiler
limitation already reproduced on pristine upstream. No permission boundary or
unavailable issue-read capability is bypassed to advance this source work.

Validation on `8c71538d7a3628a05e46e5f1350fe0abf88231e5`:

- The initial regression reproduced the lost late import (27 other focused tests
  passed). All seven pure history controls passed. Fixture audit then removed
  the unsupported model-event persistence assumption before final checks.
- The corrected native focused suite passes all 28 tests in 4.07s. Full
  Autopilot passes 1,015 tests with four intentionally ignored, zero failures,
  in 223.87s. Core passes all 291 tests in 0.55s. Official artifact/runtime
  collection returns zero; the FD limit remains 65,536 throughout.
- Canonical all-target Tippy passes in 7m45s with zero ALab findings and 41
  upstream diagnostics set aside. Certified CLI construction then exits 2 at
  the unchanged in-process proc-macro TCB refusal, without authenticated proof
  inventory. No compiler/profile exception or unverified application shipping
  was used. Runtime tests explicitly use the unverified test lane, without a
  proof claim. No package was installed.
- Independent reviews approve the production authority checks and the corrected
  host/API fixture. Trust formatting and Git whitespace checks pass; validation
  and hook processes fully exit.

Evidence under `~/.ubd/`: `codex-goal-late-duplicate-red-runtime.log`,
`codex-goal-late-duplicate-history-red-selected.log`,
`codex-goal-late-duplicate-final-green.log`,
`codex-goal-late-duplicate-final-full.log`, and
`codex-goal-late-duplicate-final-gate.log`. The full runtime receipt is
`codex-rs/target/alab-goal-full-libs.0763nd/collected.json` in this worktree.

Before publication main advanced to `207f853cd6a07eb16ceec545d8f525074147f2d1`,
bringing boot-clock recovery and callback-runtime ownership changes into shared
Core/Autopilot/TUI paths. Decision: compose and check the combined behavior rather
than publish against the stale parent. The preceding results remain evidence
for the exact source named above; integration validation follows separately.

Combined validation on `24ca0ed9bd964ae1f50478e378721717d7b6c608` passes:

- All 28 focused native recovery tests pass in 2.76s.
- Full Autopilot passes 1,020 tests (four ignored) in 235.89s; Core passes 295
  in 0.56s; TUI passes 387 (five ignored) in 86.25s. Zero failures. Official
  receipt collection succeeds at
  `codex-rs/target/alab-goal-full-libs.kJ9SoC/collected.json`.
- Canonical all-target Tippy passes in 6m23s with zero ALab findings and 41
  upstream diagnostics set aside. Certification still exits 2 at the same
  proc-macro TCB restriction, without authenticated inventory. No exception,
  shipping build or package installation occurs.
- Independent composition review approves the goal proof, clock serialization,
  callback ownership and merged fixture defaults. Both controllers fully exit.
  Evidence logs use `codex-goal-late-duplicate-composed-{green,full,gate}.log`
  in `~/.ubd/`.

Final coverage refinement separates the two predecessor-pause controls. A real
notification-only model pause now precedes the first Blocked call in one fresh
image; the fixture explicitly verifies that history alone does not reject its
old source, so the prior-status guard must do that work. Only the historical
pause case uses the persisted host/API outcome, followed by another model block.
This prevents one guard from masking the absence of the other. Production code
remains identical to the fully checked combined source; the focused native suite
and all-target lint are repeated for this test-only refinement.

The final refinement at `861da05c64ed4ef581a5c080086a05356a7ba5d7` passes all
28 focused native tests in 2.82s, including the assertion that a model pause has
no recorded host/API veto to mask the predecessor-status guard. Official receipt
collection succeeds at `codex-rs/target/alab-goal-full-libs.6v9Sy9/collected.json`.
All-target Tippy passes in 46.50s with zero ALab findings (41 upstream diagnostics
set aside). Certification remains refused at the unchanged proc-macro TCB
boundary; no application is installed. The production files are identical to
the combined source with all 1,702 tests passing and nine intentionally ignored;
the broad suites are therefore not repeated for this isolated fixture edit.
Independent final review approves the separated controls. Both final processes
exit, and whitespace checks remain clean. Evidence:
`codex-goal-late-duplicate-isolated-{green,gate}.log` in `~/.ubd/`.
