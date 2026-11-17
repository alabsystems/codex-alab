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

## Initial live-epoch implementation and validation

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

## Supersession by exact mutation chronology

The live-epoch implementation above established the necessary process-local
fence and selected dispatch-overlap as its next availability investigation.
The subsequent recovery work below subsumes that design into exact mutation
transitions, durable crash chronology, and lifecycle-wide publication ordering.
It preserves the earlier owner, epoch, history-validation, mode-revocation,
and fail-closed custody invariants while extending them across process and
upgrade boundaries.

## Expanded exact-mutation and crash-chronology design

The first native check failed exactly at the missing boundary: after the real
Paused-to-Blocked round trip, `current_model_goal` returned the original
`WallGoal`. The final snapshot and person-stop count were identical, proving
that snapshot comparison alone cannot distinguish acknowledged native intent.
A second regression suspends recovery after its preparatory read and performs
the same round trip before final admission.

The chosen fence is an opaque, process-local `MutationEpoch`: weak identity of
the exact persistence `State` plus a checked `u128` sequence. Successful native
mutations return their exact `{ before, after }` transition. Accounting-only
writes do not change it; failed, overlapping, or exhausted mutation settlement
returns no witness and holds admission. Epochs are never serialized.

Live model custody now pairs the durable `WallGoal` with an epoch. A fresh
Active-to-Blocked tool mutation binds `after`. A duplicate Blocked-to-Blocked
mutation retains the entire original `WallGoal` only when the prior binding is
at `before`, then advances only its epoch to `after`. Tool completion, live DB
standing, compact admission, and the final resume callback all validate the
epoch against the currently registered native runtime while holding its goal
mutation permit. Retirement compares the complete binding, so an old reader
cannot erase a newer duplicate with the same durable fields.

Restart import remains conservative. It can bind only across a pristine new
runtime, or across an explicit contiguous bridge of exact duplicate
transitions that began pristine before the durable source arrived. History,
the Blocked row, person-stop authority, runtime owner, and epoch must all remain
unchanged through publication. A host/API mutation closes that bridge; the
current epoch is never stamped onto old durable proof after the fact.

The owner audit found a prerequisite registry race: duplicate thread startup
could replace a live `GoalRuntimeHandle` before `ThreadManager` rejected the
duplicate, giving the two handles different semaphores and persistence states.
Registration now refuses replacement of a live different owner. Unregistration
drains the matching owner's raw mutation permit before removal, and every
normal permit rechecks current registration after acquisition. This makes the
epoch's owner identity correspond to the runtime that actually sequences SQL.

Exact call identities in the short-lived committed-mutation queue now remain
bounded tombstones after consumption or collision. A reused
`(thread, turn, call)` therefore supplies no second authority rather than
replacing an earlier fact.

## Crash chronology and settlement

The process-local epoch is intentionally not restart authority. Host set and
clear acquire a separate append-only v3 chronology journal before their
authoritative post-prepare row read and retain its stable lock through the
pre-SQL instruction, database mutation and settlement. The random 256-bit
generation is carried by recovery proof observed at that exact journal serial.
A definite rollback truncates exactly its own arm to the validated predecessor;
an unknown outcome leaves the arm standing. A successful mutation clears only
the unresolved state after the rollout instruction/outcome and SQLite row have
both crossed their durability barriers, while retaining that generation as the
latest durable serial. Malformed, truncated, stale or oversized journals fail
closed.

Every scoped host mutation owns a stable per-thread lock file for its complete
classification-to-settlement interval. The lock file is never unlinked:
replacing its inode while a process held the old one would let another process
bypass the exclusion. Contention fails closed, and a task-local rollback or
settlement retains its exact lease after a file-synchronization error. This
closes both the cross-process append-versus-truncate race and inverted SQL
settlement order left by a process-only goal permit.

A first-ever definite rollback has true absence as its predecessor. After
validating the exact partially or fully appended arm, it removes only the
journal marker while retaining the separate stable lock and syncs the parent
directory. Leaving a zero-length marker would be indistinguishable from
arbitrary truncation and must remain invalid; a crash during removal can expose
only the old fail-closed arm or the safe absent predecessor.

Model-stop and provider-wall admission require the exact latest generation
their source observed, including the initial `None` before any host mutation.
Settlement does not erase that serial, so a second process cannot round-trip a
goal to identical durable fields and revive old proof. The final wall-resume
callback repeats that check under the native permit, closing the preparatory
read-to-dispatch race. A durable user Active instruction also revokes an older
model source in history even when its SQL update definitely rolls back and
therefore emits no outcome; it remains distinct from the wall-stop counter.

SQLite runs WAL with `synchronous=NORMAL`, so transaction success alone is not
the power-loss boundary used here. Every acknowledged goal mutation and usage
charge now completes a full, non-busy checkpoint before its exact in-memory
snapshot, runtime effects, metrics, event or fence are settled. A failed
checkpoint makes the outcome permanently ambiguous and leaves admission
closed. The session record similarly syncs its temporary file and, on Unix,
each renamed directory entry through `$CODEX_HOME`.

Persistence status distinguishes a sticky failure from a healthy owner whose
frozen additive snapshot has advanced ahead of SQLite settlement. Failure,
retirement and permanent holds still dominate, but any otherwise healthy live
owner remains `Writing`; only an ownerless unreconciled snapshot is `Held`.
This ordering matters because an observer may disappear after dispatch: the
owned write can still settle successfully, and its paced admission recovery
must remain alive to resume the goal. Snapshot and direct admission queries use
one classifier so their externally reported phases cannot diverge again.

Set and clear return deferred exact-effect outcomes. A direct caller may apply
one only after the first database durability barrier; the app server retains it
through rollout persistence, a second database barrier and chronology-fence
settlement. Any later failure drops the unapplied outcome, which deliberately
holds admission instead of publishing an effect from incomplete chronology.
Authentication deferral, restore/accept, cancellation and retirement use the
same database boundary before changing live admission, and fork inheritance
crosses it before the target runtime or RPC can acknowledge the copied row.
Standalone authentication capture and native-turn cleanup remain deliberately
barrier-free because losing either can only remove recovery proof or preserve
an obsolete hold; neither grants continuation authority.

Later accounting still invalidates every destructive runtime effect. One
narrow exception acknowledges a same-goal no-op when callbacks advanced only
the unreconciled accounting-data generation after the SQL commit: owner,
mutation sequence, failure generation and accounting-write generation must
still match, with no failure, ambiguity, retirement, open clock or active
writer. The acknowledgement neither resets a baseline nor publishes metrics,
steering or continuation; the late usage remains for ordinary additive
accounting. A different goal association quarantines the original boundary.

An ownerless mutation intent attaches its exact pre-SQL write to the first live
successor. While that intent is pending, later set/clear calls now reject under
the registry-to-gap lock before runtime preparation, accounting or instruction
recording; otherwise even a rejected overlap could invalidate the older exact
witness. Cancellation before SQL releases its persistence hold but keeps the
successor epoch non-pristine, recording that intent chronology without
inventing a failed storage command. Runtime retirement similarly closes new
authority first while retaining already-admitted persistence until its final
drain and effect application finish.

## Source and lifecycle ordering

Mode changes, carried-source changes and durable source publication share one
process-wide source fence. An Enhanced/Vanilla boundary first retires all model
sources and synchronously writes that retirement; a failed write leaves the old
mode effective. ALab identity starts and owns the session writer in Vanilla as
well as Enhanced mode, so an initial Vanilla boundary cannot acknowledge an
in-memory retirement while leaving an older Enhanced record importable. The
session writer always takes the source fence before its own state mutex.
Same-thread record rebuilds use a bounded reentrant depth, so a publication can
synchronously flush the writer without either self-deadlock or the cross-thread
source/state inversion found during hostile review.

Thread archive, revert, resume replacement and terminal listener shutdown now
hold the lifecycle exclusion through runtime shutdown, registry removal,
rollout ownership and reload. An unloaded thread obtains its recorder before
SQL and keeps it through terminal outcome persistence. App-server replies and
notifications use bounded per-thread FIFO ownership, so cancellation cannot
orphan committed effects or reorder the externally observed chronology. Once
a service result may represent a commit or durable instruction, errors occupy
that same FIFO position and the outer request handler emits no duplicate.

A committed process upgrade now composes with runtime retirement instead of
racing it. Retirement atomically closes the upgrade barrier in every phase: if
it wins while a checkpoint is open or prepared, later preparation and commit
are refused; if commit wins, the registry key remains reserved for that exact
transfer. After Core producers and source readers quiesce, final capture drains
late retained turn and idle usage, freezes a fixed clock cutoff, and only then
reads the transferable row and clock. Deferred cleanup is write-free, releases
no registry key while its failure-prone work runs, and reports a prepared
result. One registry coordinator then prevalidates the whole forest, merges all
Ready carries and removes all exact keys under the registry-to-gap lock order,
and publishes every transfer plus the source result without an await. A task
lost before that point overrides provisional success and gives every successor
a Permanent carry; a partial internal commit does the same before either lock
is released. The outer source task owns the goal guard, terminal guard and watch
sender together, so cancellation restores terminal modes before observers can
see failure and no independently detached inner task can publish a goal forest
whose terminal result was lost.

Upgrade checkpoint and final capture preserve the active clock baseline even
when no whole second is chargeable. An empty accounting result at either
boundary therefore uses `KeepActive`; clearing there would erase the clock
before the destination can adopt it. Only deferred retirement clears the frozen
source clock after the transferable snapshot has been captured.

Terminal session teardown closes native submission admission before extension
retirement: it closes the receiver, drops deferred submissions and drains
channel-resident submissions. Otherwise an automatic input queued behind
`Shutdown` can hold the goal-state permit while awaiting its reply, while goal
retirement waits for that same permit and the session loop cannot drop the
queued reply owner. Normal shutdown, local-runtime shutdown, successful
suspension and channel-close teardown all use this ordering; suspension invokes
it only after the suspension has become terminal.

Windows retains an explicit qualification: every journal and session file is
flushed through the native file API, but Windows documents no standard-user
parent-directory flush. The implementation therefore remains operational
there without claiming the Unix directory-entry power-loss guarantee.

## Verification

The results below were captured on the exact pre-merge recovery source
described in this expanded section. They are implementation evidence for that
source, not a claim that the later merge composition has already passed the
same checks.

Focused Trust runs completed during implementation:

- `targo --unverified test -p codex-alab-core --lib upgrade_barrier::tests`:
  16 passed, including provisional-release abandonment, inventory rollback and
  cleanup-owner loss after publication.
- `targo --unverified test -p codex-alab-core --lib
  final_reconciliation_accepts_only_healthy_ownerless_callback_data`: passed.
- `targo --unverified test -p codex-alab-core --lib
  delayed_runtime_effect_requires_its_exact_clean_post_commit_epoch`: passed,
  including classification of the pending-data-only non-destructive case.
- `targo --unverified test -p codex-goal-extension --test
  goal_extension_backend`: 28 passed under deterministic ALab activation,
  including late same-goal callbacks, retirement drain and resumed idle usage.
- `targo --unverified test -p codex-alab-autopilot --lib
  model_blocked_goal_tests::source:: -- --nocapture`: 15 passed.
- `targo --unverified test -p codex-alab-autopilot --lib
  goal_retained_usage_tests -- --nocapture`: 4 passed.
- `targo --unverified test -p codex-alab-autopilot --lib
  model_blocked_goal_tests::registry:: -- --nocapture`: 21 passed.
- `targo --unverified test -p codex-alab-autopilot --lib
  goal_upgrade_read_tests -- --nocapture`: 10 passed, including exact final
  clock capture and automatic-input-behind-shutdown completion.
- `targo --unverified test -p codex-core --lib
  terminal_dispatch_drops_channel_and_assignment_queued_reply_owners`: passed.
- `targo --unverified test -p codex-app-server --lib thread_fork_goal`: passed.
- `targo --unverified test -p codex-alab-tui --lib session`: 111 passed.

The registry/source hostile reviews and the final cancellation/publication
audit reported no remaining concrete defect.

The canonical `alab/check.sh` run under the source-pinned Trust 15698 completed
its full all-target Tippy phase with no ALab finding; 44 upstream diagnostics
were set aside. Its required certified CLI build then stopped at Trust's known
in-process procedural-macro admission boundary, before an authenticated proof
inventory existed. The same compiler refusal is already reproduced on the
pristine upstream base. No unverified application build substitutes for that
failed certification.

A supported adoption of the newest signed Trust, build 27009 at compiler commit
`5062eed55c0bc9df8a79eba1368d88ffde76c741`, was also qualified in an isolated
worktree. Its full Tippy phase found nothing in ALab's code, with 156 newer
upstream diagnostics set aside, and it advanced beyond the old procedural-macro
boundary. The certified build then correctly refused `anyhow`'s
`run-custom-build` control unit because the project's literal
`-Ztrust-verify=on` assertion requires verification of build-script execution.
Trust records this as open issue `BUILD-SCRIPT-ASSERTION-REFUSAL-1`; current
Trust main retains the same admission rule, and signed index 151 offers no build
newer than 27009. The later missing-transport report is secondary to that
pre-compiler planning refusal.

Removing the assertion would admit build-script execution as a disclosed
conditional TCB entry, but that is a proof-policy change, not a compatibility
fix. Preserve the existing requirement. The adoption controller therefore left
`trust.lock` uncommitted, and no package or installed Codex binary was replaced.
Application certification remains deferred to a Trust capability that can
satisfy the existing policy; the source/runtime verification above makes no
certification claim.
