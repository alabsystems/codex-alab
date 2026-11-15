<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Rejected goal recovery cannot erase a successor's custody

The next audit found another instance of the publication race fixed for model
tool completion and exact goal retirement. After a goal resume returned `NotNow`,
the evaluator checked authority twice, then cleared its wall/model bindings and
published its pace without holding the runtime registration lock. A replacement
could take over between the last snapshot and publication. The old evaluator
would then erase the replacement's durable model-stop binding under their shared
thread key. A newer binding in the same runtime was also cleared indiscriminately.

Decision: remove that obsolete unconditional retirement. `NotNow` can mean either
a changed goal or revoked decision authority; it does not prove which stored
binding should be retired. Keep custody until the next healthy exact-goal read
validates or retires it. Schedule that read through the guarded decision recheck at
its original immediate deadline, ordering the timer with native point ownership
and runtime replacement. Unread-goal retries retain their separate 60-second delay.
This preserves ordinary policy, input, budget and person-stop checks.

The same audit found unguarded pace publication after hour updates, compaction
booking, ordinary submission and goal-resume booking. Fix the shared boundary:
ordinary `sync_pace` now acquires registration custody, while callers that already
hold it use an internal method requiring the actual `MutexGuard`. A copied list
of threads cannot provide that witness. Resume seeding follows the same rule.
Existing pure fixtures now register their runtimes, as native production does;
there is no unregistered-runtime exception for tests or compatibility.

The regression pauses a real rejected native goal recovery after its last
authority snapshot, publishes a successor's binding, then releases the old
evaluator. It must leave the successor's pace intact and arm no stale timer. A
same-runtime successor must likewise keep its new binding. A direct stale pace
writer control also preserves a replacement's binding with or without its old
runtime marked stopped. Both native rejection regressions failed on the landed
code while its 16 existing recovery tests passed.

The first guarded version reused the unread-goal retry deadline and inadvertently
delayed a known changed-goal redecision by 60 seconds. The complete autopilot run
caught this through its existing usage-resume hang detector: 997 tests passed and
that single case failed. The shared guard now takes an explicit deadline, so known
goal changes redecide immediately while read failures retain their paced retry.

Final validation on `701c99cc164b83dc04621ba6778a46166832072a`:

- Both new native races first failed as intended while the 16 existing recovery
  tests passed. The guarded implementation passes all 18 focused tests in 2.26s.
- After restoring the immediate deadline, the existing usage-resume redecision
  case passes in 0.46s. The complete autopilot suite passes 998 tests, with four
  intentionally ignored, in 214.82s. No source changes occurred during the run.
- Canonical all-target Tippy passes in 2m03s with zero ALab findings and 41
  upstream diagnostics set aside. Certified CLI construction then exits 2 at the
  unchanged proc-macro TCB restriction. No compiler, profile or proof exception
  was introduced; runtime tests use the explicit unverified lane, not a shipping
  application build.
- Independent call-site and lock-order review found no remaining unguarded pace
  publication or introduced lock across an await. File-scoped Trust formatting
  and Git whitespace checks pass.

Local evidence: `~/.ubd/codex-goal-rejected-resume-red.log`,
`codex-goal-rejected-resume-green.log`, `codex-goal-rejected-resume-full.log`,
`codex-goal-rejected-resume-usage-final.log`,
`codex-goal-rejected-resume-full-final.log`, and
`codex-goal-publication-custody-gate-final.log` in the same directory.

Other operations remain deferred rather than escalated: the GitHub issue listing
was refused with HTTP 403 because the connected token lacks `issues=read`; no
alternate identity or transport was attempted. Application release still requires
the certified compiler support already documented in the original recovery review.
