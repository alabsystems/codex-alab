<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Continue the original goal after an agent declares it blocked

The owner observed tab 1 stopping a multi-day goal because a temporary pinned
`build-slot` path disappeared. The agent treated a stale validation artifact as
an unresolvable constraint, marked its goal blocked, and needed repeated human
messages to continue. The native supervisor accepted every blocked goal as a
person's decision and stopped its automatic prompts.

Decision: in autonomous Cruise and Land, a successful native model
`update_goal(blocked)` is evidence of an agent-declared obstacle, not a person's
stop. Bind that event to the exact goal and turn, preserve the objective and
budget, and resume through the existing goal service and paced scheduler. The
continuation must insist on concrete original-goal progress: inspect the actual
constraint, recover temporary tooling, or choose another authorized route. A
status restatement or another handoff is not progress. Repeated agent blocks
continue through pacing rather than requiring a person to type "keep going".

The recovery must not infer provenance from a bare database `blocked` status.
Human pauses and stops, goal edits or replacement, exhausted budgets, Off mode,
holds, failed persistence, and permission refusals retain their existing force.
Recheck the identity and authority under the native goal mutation permit before
reactivation. Carry the model-stop binding in the existing durable pace so a
relaunch cannot silently convert it into a permanent park.

Validation:

- Source `110cab5847` adds the recovery and regressions. Test-only `591bf2965d`
  gives the delayed provider fallback lower priority than scripted responses.
  Lossless merge `1734ac13c047a4d78003069af9e1a24820822588` incorporates concurrent
  main `4d095aa1b37cbf066a04131635c2492a027d87a2`, including exec routing and passive
  test ownership fixes. Its tree matches the independently computed merge tree.
- All seven native recovery regressions pass on the merged source (1.38 seconds):
  paced same-goal recovery, repeated automatic self-blocks, duplicate blocks and
  create-and-block within one turn, unreadable goal recovery, pending upgrade
  notices, stopped goals without model provenance, and newer stop/edit/budget
  races. The same seven pass before the merge (1.09 seconds).
- Five distinct unit controls pass: exact committed-call marker consumption,
  three person-stop controls including model-stop revocation, and durable
  model-block serialization, malformed records and old records. The merge leaves
  these tested implementations unchanged.
- The canonical gate's full all-target Tippy phase passes on the merged source:
  zero ALab findings, with 41 upstream diagnostics set aside. The focused autopilot
  all-target check also passes after the test fixture correction. File-scoped
  Trust formatting and Git whitespace checks pass.
- The full gate does NOT pass. Its certified CLI build refuses Trust 15698's
  in-process proc-macro TCB boundary before collecting authenticated proof
  inventory. The same exact native certification refusal reproduces on pristine
  upstream `ce3031018ef130e653ed4d9c5bb971652de97161`, under the same authenticated
  compiler and protected profile. No proof waiver, compiler override or unverified
  application build substitutes for certification. Runtime tests use the explicit
  unverified lane and make no proof claim. No application package was built or
  installed; running tabs do not acquire this source change without a qualified
  release.

Initial cold-clone setup was recovered through supported paths: signed source-pinned
Trust 15698 installation succeeded after a stale lease ended, the Apple developer
tools installation completed, the native producer was built, and both exact locked
source/pristine dependency closures were prepared in the private Cargo home. No
checksum, compiler pin, native admission or permission boundary was bypassed. An
owned fleet-shell fallback was refused at `access=read needs=full`; no peer session
was driven, grant raised or alternate transport used.

Local evidence retained:

- `~/codex-alab-goal-block-recovery-merged-tests.log`
- `~/.ubd/codex-goal-core-tui-unit.log`
- `~/.ubd/codex-goal-focused-tippy.log`
- `~/.ubd/codex-goal-merged-full-gate.log`
- `~/.ubd/codex-goal-pristine-certify.log`

The remaining binary-release operation is blocked by the independently reproduced
compiler certification limitation. This record is source/runtime-test evidence,
not a source-qualified release or a formal verification claim.

## Follow-up audit: recovery belongs to the goal

The continuation audit found four missing paths in the landed implementation.
An unrelated user reply overwrote goal custody from its empty turn tracker;
interrupt/SIGTERM teardown discarded even an already persisted successful stop;
Land could record a clean repository as landed while the original goal remained
blocked; and a usage reset's `ResumeLimit` action never resolved the model-stop
binding. Four native regressions failed on the landed code for those exact
reasons, while its seven existing recovery regressions still passed.

Decision: keep one goal-owned runtime binding and remove the redundant turn-owned
copy. Only successful goal mutation, exact successful reactivation, or a healthy
mismatch retires it. Retire the old binding before runtime effects can start a
new turn. Resolve model provenance for usage resets too; replace premature Land
completion with an ordinary paced original-goal continuation using the common
policy, exclusion, wake and budget guards. Explicit human pauses remain stops.

Review of the new retirement path also reproduced two stale-instance publication
races: an old runtime could replace a newer runtime's durable pace during either
retirement or late tool completion. Both deterministic tests failed before the
fix. Binding changes and pace publication now hold the existing runtime
registration lock; rejected late callbacks still consume their own source fact.

The final recovery suite passes all 13 tests (eleven native recovery scenarios
and two runtime-publication controls). All 183 pure decision tests pass, including
continuation-off, unreadable/exhausted hourly allowance, refusal-boundary retention
and policy-work exhaustion controls for the new goal continuation. Tests use the
explicit unverified runtime lane and make no proof claim. Evidence is retained in
`~/codex-alab-goal-custody-red.log`,
`~/codex-alab-goal-custody-replacement-red.log`,
`~/codex-alab-goal-custody-green.log` and
`~/codex-alab-goal-custody-decide.log`.

Committed follow-up `cd1f4aa1c28f17f171e1080b9f0833420c332da2` also passes the
canonical full all-target Tippy phase: zero ALab findings, 41 upstream diagnostics
set aside, 2m16s. The certified CLI phase then stops at the unchanged native
proc-macro TCB restriction already reproduced on pristine upstream. No check or
compiler boundary was bypassed. The source remains runtime-tested and linted;
binary release stays unavailable pending compiler certification support.
Evidence: `~/.ubd/codex-goal-custody-gate.log`.

## Restart audit: preserve provenance while history is unavailable

The next audit found that a native reply accepted before the asynchronous resume
scan prevented model-stop restoration. Its finish then replaced the saved binding
with an empty one. A temporary rollout read failure had the same permanent effect;
the resulting runtime also lacked a point from which to retry. The record writer
could discover its source after a native reply, when the old pacing import was
already fenced out.

Decision: retain model-stop provenance separately from authority to reactivate a
goal. A pending runtime import stays in the durable pace until full native history
can validate the original turn and rule out a person's stop. Unread history retries
without reactivation; readable history and the exact native goal row are both
required. Only an unstarted runtime may reconstruct its missing baseline point.
An accepted reply keeps its own point, prompts, and pacing.

The record writer offers only model-goal provenance from its uniquely selected,
qualified source, even after native work supersedes the rest of that source. An
explicit empty source, a person's goal set, superseding native goal mutation, or
retirement closes that import for this process, including before a late source
arrives. A positive late import wakes evaluation; ordinary pace publication does
not. This keeps the existing whole-session handoff fences and avoids replaying
obsolete prompts or timing. Compaction needs no special exception: validation
reads the full durable rollout, rather than the compacted model context.

Validation on source `1ac4b40f19` plus test-import correction `3993cdeaf5`:

- All 996 enabled autopilot library tests pass (four ignored), including the three
  new native restart regressions and every existing goal, scheduler and publication
  scenario. The complete core suite passes 291 tests; TUI passes 386 (five ignored).
  That is 1,673 passing tests and nine intentionally ignored across the three
  libraries. Native helpers and the result runner came from the same source and
  selected Trust 15698 toolchain under the protected test profile.
- The upstream goal extension disables library tests and contains no standalone
  unit cases. Its hook is covered by the real native goal-tool fixtures in the
  passing autopilot suite and the all-target compiler/lint check; a zero-test
  library build is not counted as additional coverage.
- The first broad standalone runner inherited a soft file limit of 256 because
  it omitted the canonical gate's existing resource precondition. Its autopilot
  run hit `EMFILE` and cascaded; core and TUI still passed. The corrected runner
  applies the gate's 65,536 limit before selection and in its children. The full
  autopilot rerun passes in 221.21 seconds. No production or profile change was
  needed to correct that validation setup.
- The canonical all-target Tippy phase passes in 6m34s: zero ALab findings, with
  41 upstream diagnostics set aside. Its certified CLI phase then exits 2 at the
  unchanged in-process proc-macro TCB restriction. The test-import correction was
  committed before its affected test target compiled; production sources were
  unchanged throughout the gate. No proof waiver or unverified application build
  substitutes for the unavailable certified release.
- Independent hostile review checked source selection, pending history reads,
  person-stop revocation, runtime replacement, historical Esc, newer native points
  and late-import wakeups; no further concrete defect was found. File-scoped Trust
  formatting and Git whitespace checks pass.

Evidence is retained in `~/.ubd/codex-goal-autopilot-full-fd65536.log`,
`~/.ubd/codex-goal-restart-full-lib-units-green.log` (including the
file-limit failure and the complete passing core/TUI runs), and
`~/.ubd/codex-goal-restart-gate.log`. Runtime tests use the explicit
unverified lane and make no proof claim. Binary release remains subject to the
unchanged certified compiler limitation above; existing tabs retain their current
binary.
