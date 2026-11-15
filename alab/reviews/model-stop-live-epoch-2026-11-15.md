<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Next improvement: fence live model proof across native mutations

After original-source preservation landed at
`76390b3d67208d59b75aef990f9ef49964b2a8c5`, a read-only audit checked the live
path separately from restart import. The new origin tests defend restart
validation; they do not demonstrate permanent live invalidation after a native
host/API mutation round-trip.

The source-level sequence is: establish a real model Active-to-Blocked binding;
hold evaluation; issue an automatic/legacy native host/API Paused update and
then Blocked, persisting and applying both actual outcomes. This is not a
User-origin instruction, so the person-stop counter stays unchanged. The final
goal ID, objective, budget and status again match the old model stop. Once its
pending import has been consumed, `current_model_goal` does not reread history.
Its live standing check, and the resume closure, compare the goal snapshot and
person-stop authority without a native mutation generation. Both acknowledged
API writes leave persistence admission ready.

Decision: reproduce that sequence with the real native harness before changing
production. The source audit identifies a live fencing gap; no new runtime
regression or implementation was run in this post-landing audit. Preserve the
already-landed original durable turn and all explicit person-stop boundaries.

The existing native goal-persistence mutation generation is a candidate local
fence. `begin_mutation` changes it before SQL, and settlement changes it again;
ordinary accounting does not use that mutation path. Snapshot revision is not
equivalent because accounting also changes revision. An opaque native owner and
mutation-epoch token could be captured with the committed stop and validated at
tool completion, live lookup, and under the final mutation permit before resume.

A fresh Active-to-Blocked commit can establish a fresh token. A genuine model
duplicate would need to prove that its pre-mutation epoch matches the bound
token, then advance only that local token to its acknowledged epoch. Its entire
durable goal binding must retain the original turn, as the landed fix requires.
Any intervening external mutation must break that chain. Runtime replacement
and import require a separate conservative audit: an old source must never be
retroactively blessed with a new token after unobserved writes. The native API
does not currently expose this opaque token.

Required next native checks:

- Perform the API round-trip after a real block without restarting. Live lookup
  must reject old proof; due evaluation must leave the goal Blocked, accepted
  turns and provider request count unchanged.
- Suspend an already prepared resume before final admission, perform the same
  round-trip, then prove the pending resume is rejected.
- Preserve accounting-only updates, ordinary model duplicates, and genuinely
  fresh Active-to-Blocked commits after API actions. Keep failed/cancelled API
  intent held by the existing persistence admission fence.

This local mutation-fencing investigation is distinct from the deferred durable
request-chronology capability described in `model-stop-origin-2026-11-15.md`.
No compiler/profile exception or package installation is part of either audit.

The native RED now confirms both live paths: 35 tests pass and two fail in
2.83s. After the actual API round-trip, live lookup still returns the old goal
binding; an already-prepared resume changes the row to Active. Accounting-only
and fresh-block controls pass, as do all 33 prior recovery cases. Evidence:
`~/.ubd/codex-goal-live-epoch-red.log`; official receipt
`codex-rs/target/alab-goal-full-libs.Yd64j8/collected.json`.

Implementation decision: represent a mutation epoch with weak native-owner
identity and a unique allocation identity, separate from passive revision and
admission. Mutation intent and settlement each revoke it. Successful settlement
returns the exact before/after receipt under the persistence mutex; it never
samples a later current epoch after commit acknowledgment. Retaining a fence
does not keep a retired native owner alive.

The native commit marker carries that receipt. Process custody keeps a complete
known binding, an exact import lease, or an explicit closed state. An untouched
owner can originate import custody; a proven Blocked-to-Blocked receipt can
advance that same lease while preserving the original durable goal source.
External or unrelated mutations make the old lease stale, and reading a saved
goal cannot replace it with a newer token. A stale token remains passive so an
in-flight genuine duplicate can prove its exact transfer; it grants no action.
Runtime replacement transfers the whole known binding rather than reassigning
an epoch to equal serialized goal fields, and revalidates its history before
making it live. A duplicate arriving during that validation advances only the
exact pending lease; it does not skip the history check.

Live lookup, pace publication, compaction admission and goal resume validate
the registered native owner and epoch. Final resume scopes the immutable fence
through the runtime actually selected by GoalService. Existing hooks check it
after native permit acquisition, before preparation can disturb newer recovery,
and around intent validation before SQL. That validation itself owns a healthy
non-mutating write, so epoch equality remains separate from admission; a sticky
Held state still refuses automatic resume. Cleanup compares complete bindings,
including epochs, so an older attempt cannot retire a newer equal-looking goal.

This fence detects mutations observed by the same registered native owner. It
does not add a durable cross-process witness or infer unrecorded ownerless SQL
history. Qualified imports retain all existing conservative history checks.

The first integrated candidate `535a01032321da5f492354f3a8dc0b3a8324360e`
compiled and passed all ten new epoch controls. Its focused suite reported 39
passes and four failures. Two restart controls exposed premature live adoption
from process-local Known custody; that path now stages the exact source and
lease for full history validation, including an early duplicate. Two other
fixtures expected publication of proof without a registered native owner, or
after an intervening API hold. They are updated to expect suppressed proof;
the held restart still receives the pre-hold saved source and must reject it.

The candidate's all-target Tippy passed in 8m14s with zero ALab findings and 41
upstream diagnostics set aside. Certification stopped at the unchanged
proc-macro TCB restriction. These are intermediate results, not final acceptance.
Evidence: `~/.ubd/codex-goal-live-epoch-{green,gate}.log`; focused
receipt `codex-rs/target/alab-goal-full-libs.XB9D6a/collected.json`.

Review also found an Enhanced off/on edge. Vanilla ALab goal intents now revoke
only passive recovery epochs, without creating an Enhanced persistence write or
changing native admission. API revocation precedes polling the instruction
future so a recorded intent that fails or is cancelled still invalidates old
model proof. SQL-stage revocation remains for a mode change during recording.
Scoped automatic model resumes additionally require Enhanced mode to remain
active. Native tests cover both API and model-tool mode round-trips and failed
or cancelled vanilla instruction recording.

Corrected validation on `c89e11600820c7f58eb949efaa6f1f01a5ca66f6`:

- All 48 focused native recovery tests pass in 34.86s, including all earlier
  regression controls and all 15 epoch cases. Official receipt:
  `codex-rs/target/alab-goal-full-libs.RFnams/collected.json`.
- All nine dedicated Core epoch tests pass in 0.01s. Receipt:
  `codex-rs/target/alab-goal-full-libs.ywwaxG/collected.json`.
- Full Autopilot passes 1,040 tests (four ignored) in 249.98s; Core passes all
  304 in 0.55s; TUI passes 387 (five ignored) in 84.32s. Zero failures: 1,731
  pass with nine intentionally ignored. Native artifact/runtime collection
  succeeds at `codex-rs/target/alab-goal-full-libs.nZZZkc/collected.json`.
- Canonical all-target Tippy passes in 3m12s with zero ALab findings and 41
  upstream diagnostics set aside. Certified CLI construction exits 2 at the
  unchanged in-process proc-macro TCB restriction, without authenticated proof
  inventory. No compiler/profile exception or application installation is used.
- Two independent bounded final reviews approve replacement history validation,
  duplicate custody, exact cleanup, native ownership checks and vanilla intent
  revocation. Formatting and whitespace checks pass. FD remains 65,536; all
  original validation/gate processes and hook children fully exit.

Evidence under `~/.ubd/`:
`codex-goal-live-epoch-{corrected-green,core,full,corrected-gate}.log`.

Before publication main advanced to
`9b3fdf89429e15e767cf060843e484cb9113eafb`: smoke teardown checks/docs and an
account-error rendering change in ALab TUI glue, without goal-fence overlap.
Decision: compose it and repeat the incremental all-target gate. The complete
runtime results above remain tied to their exact source; the goal implementation
does not change during this composition.

The composed candidate `7d5df71461f84eb9740e1c5753002fafc1b5cbd0` passes the
final all-target Tippy check in 1m56s with zero ALab findings and 41 upstream
diagnostics set aside. Certification remains refused at the unchanged compiler
boundary, without authenticated inventory. The original controller exits;
no application package is installed. Evidence:
`~/.ubd/codex-goal-live-epoch-composed-gate.log`.

Next investigation selected: reproduce fresh source publication when direct
and code-mode goal dispatch overlap. Direct calls serialize through completion,
but their dispatcher and the code-mode worker can have separate gates. If a
fresh Active-to-Blocked receipt becomes stale before its completion callback,
an already-committed duplicate may arrive without that source having been
published. The current fence deliberately fails closed. A native overlap
regression should establish whether accepted receipt chaining can preserve the
original source without granting authority across API mutations, policy holds
or rejected completions. This is a separate availability investigation, not an
unverified relaxation of the landed live fence.
