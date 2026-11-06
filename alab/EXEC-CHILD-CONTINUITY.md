<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Child continuity across account replacement

## Current native verification, 2026-11-04

Source `9ab68908135c5e5306ee1fd612a0da5c78426458`, packaged privately as
`0.1.0-dev.14752+codex.0.166.0.g9ab689081`, passes the full `alab/check.sh`
gate and all thirteen final native scenarios against aterm host
`4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`:

- Ephemeral root, active child, and completed idle child followed by a new turn.
- Repeated account moves, active child with accepted mail, retained token usage,
  lost consume acknowledgement, human output, and changed model/worktree state.
- Exhausted-pool cancellation and owner Stop after consumption.
- Forty simultaneous requests using distinct login files and shared conversation
  storage, with distinct thread IDs and unchanged credential files.
- Eight concurrent sessions sharing a pool through two rotations and a delayed
  preparation reply: eight original processes and turns, nine consumed receipts,
  three retired unexecuted reservations, and a proven shared target.

The final results are retained under `.aterm/.ubd/codex-final-native-9ab6890813-v1`,
including `summary.json`. Package version/default checks, signatures and layout
passed without installation or promotion. The gate log is
`exec-child-identity-full-gate.log`: Core 3,026 passed and one ignored, Exec 71
passed, TUI 6,186 passed and five ignored, and zero ALab Tippy diagnostics
(118 upstream diagnostics set aside).

The active ephemeral-child probe first failed on `0595050658`: destination
startup could not find the child's registry identity. A memory-only root has
no stored descendant metadata to reload. The replacement now restores the
captured ID, path, nickname and role before residency admission; existing
identities must match and path collisions are refused. This changes no native
execution authority. The eleven focused child restoration/identity tests pass,
including the regression that reproduced the missing-child failure. The idle
native probe also proves that completion survives the move and that follow-up
uses the same child and parent IDs with a distinct new turn.

These are real Codex processes with isolated fixture credentials and a local
provider, using aterm's native account-selection and admission implementation.
They do not prove live OpenAI enrollment or fleet placement. Shared email and
sign-in remain aterm services, not Codex-specific mailbox code; the live Proton
API read succeeded, but the attempted OpenAI enrollment stopped at its
Cloudflare check. Review and other non-regular task modes remain outside this
native qualification. The full goal is not declared complete.

## Earlier native verification

Private package `0.1.0-dev.14739+codex.0.166.0.g3ca1e4503` (source
`3ca1e4503520d9764a5eab827a108587335136ee`) passes these real-process checks
against native host `4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`:

- Child-only quota: the waiting parent retains its PID and original turn;
  the child resumes and completes its original turn without an abort. Every
  visible JSON item closes, and the interrupted wait identifies the account
  handoff rather than a user cancellation.
- Active child with accepted mail: three parent requests and three child
  requests retain the original turns, and the mail appears exactly once.
- Eight concurrent sessions sharing a pool: all eight original processes and
  turns finish through two account moves, with nine consumed receipts. A late
  preparation reply is rejected after the selected account changes.

The corresponding retained directories under `.aterm/.ubd` are
`codex-native-child-quota-3ca1e45035`, `codex-native-active-child-3ca1e45035`,
and `codex-native-shared-eight-3ca1e45035`. The canonical package passed its
version, default, signature and layout checks without installation or
promotion. These results do not qualify ephemeral execution or live email
sign-in. Historical failures and the implementation stages follow below.

The parent completing after a quota switch does not prove that its active
children continued. `alab/smoke/exec_accounts.py` now separates two opt-in
real-process checks, both using `--pending-mail`:

- `--child-followup`: a child finishes before the switch. After recovery the
  parent sends it a follow-up through the real collaboration tool. The same
  child must make a request on the selected account and complete both distinct
  turns without error or abort.
- `--active-child`: the provider holds the child's second request in flight,
  after its mail to the parent was acknowledged. It releases that response only
  after the parent makes a request on the selected account. The original child
  turn must complete exactly once, with no abort or replacement turn. The
  fixture accepts one selected-account retry carrying the original completed
  tool output, without adding a follow-up prompt. The parent stays alive until
  the original child reaches a terminal event, so ordinary parent shutdown
  cannot itself interrupt a successfully resumed child.

Both modes also retain the ordinary parent checks: original PID and turn,
exactly-once accepted mail and completed tool outputs, native receipt
consumption, and JSON or human output. The child oracle reads the child's
persisted lifecycle events, checks errors, and records them in
`child-continuity.json` and `result.json`. A failed child check makes the whole
probe exit nonzero even if the parent succeeded.

Run either mode with the existing required `--host`, `--package` and fresh
`--output` arguments. These fixtures use isolated local accounts and a local
provider. They do not exercise live email authentication.

On private package source `767a333a1622e30fcf4054e37aeb362d59b1faf1`
and native host source `975a0a0654421f9afb387b1c4019a04155eaa8bc`,
the completed-child check passes. The active-child check fails: the parent
recovers, but the original child emits `turn_aborted` with reason `interrupted`.
This is an outstanding runtime defect, not an expected-success exemption.
That package predates the original-child continuity implementation; the
corrected native result is recorded below. Results are in local run directories
`codex-maintained-child-followup-767-v2` and `codex-maintained-active-child-767`
under the machine's `.aterm/.ubd` directory.

The recovery-aware fixture was checked against the same `767a333a1` package:
completed-child follow-up still passes, and the active-child run reaches parent
recovery but fails on the original child's recorded abort. Five local fixture
tests also check a successful same-turn continuation, rejection of a successful
replacement turn after abort, waiting only for the original turn's terminal
event, retention of the completed tool output in a selected-account retry, and an
incomplete UTF-8 append while watching the live child rollout.
The run directories are `codex-resumable-fixture-child-followup-767` and
`codex-resumable-fixture-active-child-767` under `.aterm/.ubd`.

## Cold restoration in Core

The child recovery worktree adds `CodexThread::prepare_exec_child_restore`.
It restores captured children beneath an already prepared replacement parent,
using that parent's local runtime, authentication manager, executor policy and
fresh environment bindings. It keeps the logical child ID and original-turn
metadata, and returns an idle child: each original turn still needs separate
live admission. Idle mailbox data remains owned by the returned handle until a fresh native
admission callback permits queue restoration.

Preparation requires the replacement root's pending original input and the
captured parent-child relationship. Startup checks the canonical stored history
before opening a writer, validates capability and dynamic-tool availability,
and intersects local permissions with the new parent's authority. The captured
history prefix must remain exact. Core may append effective-settings records
while resuming; only records matching this runtime's observed startup or final
settings are accepted. Changed original records and unrelated appended events
are rejected. A cancelled waiter can rejoin the same owned startup task.

The production tree bootstrap uses this Core component. The historical
failure above was subsequently corrected and qualified in the native result
below. Remote permission changes require
renewed owner validation; idle ephemeral restoration is implemented below and still needs native qualification.

Focused validation: the two new Core tests pass, covering an active internal
child, an idle internal child, fresh authentication/runtime ownership,
cancelled-waiter retry, missing native admission, invalid environment ownership,
changed configuration and history rejection. The tree regression filter passes
50 tests, input-state passes 12, and child suspension passes 5. The public V2
child residency path also has direct coverage: an active and an idle public
child are registered in the source graph, captured, and restored in the fresh
parent's runtime. The test verifies that the activation pin blocks eviction
until released. Root bootstrap reads model context rather than latest-turn
evidence, preserving the V2 metadata needed to reload the child registry. Four
tests pass under the `child_restore` filter (three restoration tests and one
existing configuration test). These results do not qualify the native
end-to-end account switch.

## Native member admission protocol

`alab-aterm::exec_grant::tree` adds the client side of a separate native tree
contract. `tree-info` reports `tree-contract=1`. The reserved source image sends
`bind-tree` with its native ticket and a hex-encoded manifest containing version
1, the capsule SHA-256 digest, root thread/turn, and an ordered list of members
(thread, parent, optional original turn). The root is first; every other parent
must already be in the list. Duplicate threads and changed manifests are
refused. The manifest is request data, not an execution grant.

After loading the replacement image, each child uses `admit-member` with the
same ticket, digest, thread and original turn. An idle member uses `turn=-` and
has no invented original turn. Both first admission and retries reach native
policy, owner, image, account and receipt validation. A consumed tree uses its
own selected-account receipt; another session changing the pool's next-launch
selection does not select a different account for this tree. Identity changes,
account holds, unavailable quota or policy changes still refuse admission.

The client rejects replies for a different digest, member or turn. Tests cover
ordered membership validation, exact retry matching, idle members, and a native
refusal after an earlier successful reply. The production capsule producer and
consumer still need to call these methods and connect them to per-child Core
recovery. This protocol work does not change the real-process result above.


## Idle queue restoration

`ExecRestoredChild::restore_idle_inputs` owns the admission and publication task
independently of its waiter. A refused callback leaves captured mail uninstalled;
a successful retry of the same operation reports its result without duplicating
mail. Publication keeps captured mail ahead of newer arrivals and records no
synthetic original turn or accounting interval. It neither wakes delivery nor
releases the residency pin: bootstrap must restore all idle queues before
activating children, then release their pins explicitly.

Focused validation passes five tests under the `child_restore` filter and
three idle-input tests, including admission refusal, dropped-waiter reconciliation, queue order,
and V2 pin retention. The adjacent tree and input-state regressions pass 52 and
13 tests respectively. The native protocol counterpart is on aterm main at
`4e90b1b7ce`, composed from the fully verified `100f8e6ad5` without overlapping
changes. Production capsule transport and the final activation phase remain
unfinished; no package was promoted.

Core-only Tippy (`--no-deps -p codex-core --lib`) passes with no ALab
diagnostics. Recovery helpers now return errors for impossible missing frames
or tasks instead of panicking; the child-suspension regression passes five
tests. A dependency-inclusive lint run still encounters upstream network-proxy
findings, so this is not a claim that the final integrated gate has passed.

## Destination launch ordering

`ExecLaunchBarrier` binds active original turns to their freshly loaded Core
sessions. A turn-only `ExecLaunchPermit` can be attached during recovery; the task waits after
its `RunningTask` is registered and before entering its task body. The controller
waits for every active member to be ready, finishes restoring delivery state,
and explicitly releases the group. Readiness alone cannot run model work.
Aborting a waiting group fails its tasks before their bodies run; a permit cannot
be reused for another runtime or turn. Native per-member admission is still
required separately.

A cold-restored descendant carries `ExecRestoredMember` through exec-host setup
so its original-turn recovery marker is retained. Ordinary child and internal
sessions still follow the existing host policy. This marker is volatile
provenance, not execution authority.

Focused tests pass: three launch-ordering tests, 52 tree regressions, 13 ordinary
task tests, and five exec-host lifecycle tests. The launch tests exercise real
Core task registration, explicit release, abort before and after readiness,
and a foreign runtime carrying the same logical thread and turn IDs.
Core-only Tippy (`--no-deps --locked -p codex-core --lib`) also passes with no
diagnostics in ALab sources or the task hook.

`ExecRecoveryRequest::new_for_launch` now accepts a permit bound to the original
thread and turn, rejects mismatched options, and installs it alongside the
independent live admission callback. Its request identity is fixed at construction,
so retrying a clone reconciles the same admitted launch. Four app-server recovery
tests pass, including a captured-tree case that restores accepted input, rejects
native refusal even with a permit, registers the original task without sending a
provider request, and completes only after explicit release. The existing compacted
and ephemeral recovery cases also pass. This verifies the recovery adapter; it
does not yet verify multi-member activation through the native capsule.

## Coordinated tree activation

`ExecRecoveryTree` prepares every captured child in parent order beneath the
fresh root, retaining internal child handles directly rather than looking them
up through the public thread API. Each child's captured capability selection
must remain within the fresh parent's selection. Preparation leaves all turns
idle and keeps residency pins until activation.

Activation first obtains separate native admission for each idle member and
restores its mail without waking delivery. It then admits and registers every
active original turn behind the launch barrier. Only after all participants are
ready does it release the tasks, wake restored idle mail, and release the pins.
Fresh tasks triggered in restored idle members also wait for that release; a
controller-mail notification cannot let them run ahead of active-member admission.
The operation owns its asynchronous work; a disconnected waiter can rejoin its
recorded result. A failed member aborts the waiting group and shuts down the
prepared destination tree. Its callback is fixed when the operation is created.
Cleanup cancels the shared runtime and joins every member, including idle and
internal descendants; stopping just the root session is insufficient.

Six app-server recovery tests pass. The two tree cases include a root, an active
internal child, and an idle internal child using a configured provider. They
verify exact original input, parent-first admission with idle mail staged first,
no destination request while child admission waits, lost-waiter reconciliation,
and termination of every member with no destination request when that admission
is refused. The other four cases retain root, compacted, ephemeral, and explicit
barrier-release coverage.
The two tree cases also submit a real new turn to the restored idle child while
another child's admission is held. Success releases all three tasks; refusal
terminates all three without a destination provider request. Four Core launch
tests pass, including a directly polled idle-task body while the original-member
barrier is ready but unreleased. The 52-test Core tree regression also passes.
Core and app-server Tippy (`--no-deps --locked --lib`) passes with no ALab
diagnostics. This is focused validation, not the full integrated release gate.

## Production tree capsule

The production source now captures the whole tree through the embedded
suspension request. It binds aterm's immutable member manifest to the capsule's
SHA-256 before requesting execution. Both the ordinary exec contract and tree
contract must be available before the source is stopped.

The replacement requires the authenticated tree capsule. It restores the root
configuration, verifies the original root settings against aterm's checkpoint,
loads the root's full model context, checks its history and fresh settings, then
prepares every child through `ExecRecoveryTree`. The embedded activation request
can only name that bootstrapped root and original turn. Its immutable callback
consumes the root receipt and separately admits each active or idle member
through aterm. Callback waits and group activation share the native deadline.

The old root-only recovery endpoint, single-frame bootstrap constructors, and
renderer side-file transport have been removed. Presentation state travels in
the tree capsule.

Focused validation passes eight app-server recovery tests, three suspension
tests, the embedded-client refusal test, the exec completion test, and 91 shared
aterm-client tests (two existing tests ignored). The tree test now captures
through the production suspension operation and crosses the actual in-process
bootstrap/activation requests; it checks the original root and child completion.
The first same-PID native active-child run on `add292299e` reached the loaded
successor and consumed the root receipt, then refused the child as no longer
unfinished. Its persisted child turn was still open: it began with an
`agent_message` record, not a user-message event. The canonical latest-turn reader
had discarded that record, and the recovery predicate recognized only user input.
The run is retained under `.aterm/.ubd/codex-native-tree-active-child-add292299e`.

Recovery now retains agent-message records in the canonical turn evidence and
recognizes accepted mail only when its stored turn attribution matches the
current running turn. Missing or foreign attribution, completed or aborted turns,
and active compaction remain refused. The tree regression starts its child with
encrypted agent mail to cover the real input path. The eight app-server
recovery tests, five canonical-history tests, and fifteen resume-history tests
pass with this correction. A new native run is required before claiming
active-child continuity.
Tippy passes for Core, app-server, its client, exec, and the aterm client
(`--no-deps --locked --lib`) with no ALab diagnostics.

## Child-originated quota

`alab/smoke/exec_accounts.py --child-quota` exercises the complementary trigger:
the parent spawns a child and enters the real `wait_agent` tool. The provider
waits for the parent's incomplete wait item in live JSON output, then returns
HTTP 429 only to the child. Every parent provider response is successful. Both
original turns must complete after native account replacement, with the child
making exactly one request on each account and the parent retaining its completed
spawn and wait outputs. An old-account retry, replacement child turn, aborted
child, or parent completion without native replacement fails the oracle.

This is separate from `--pending-mail --active-child`, where the parent reaches
quota while the child is in flight. The new trigger is not yet qualified. Eight
local fixture/oracle tests pass, including rejection of an old-account child
retry and rejection of completed or partially written parent wait items. The
current controller still subscribes to a per-thread root quota observer; source
inspection identifies child-originated quota propagation as remaining work.

The initial child-only run against `add292299e` reached a real parent wait and
returned a child-only HTTP 429. No native switch occurred. The child's quota
notification woke the parent's wait, so the fixture now allows that parent to
wait again while retaining each distinct wait output. It still requires the
selected-account child request and native replacement. Nine fixture tests pass.
The initial run is retained as `codex-native-child-quota-add292299e`; the adjusted
fixture still needs a new native run.

## Tree quota observation

The exec host marks every live tree member for quota waiting without granting
children root-turn authority. Interactive hosts remove this marker. A member's
actual authenticated weekly-quota request publishes into the observation set
owned by its local tree runtime. Concurrent waits remain independent: replacing
one thread's wait revokes only that thread's prior observation, and retiring the
selected wait reveals another still-live member rather than losing it. Separate
root runtimes have separate observation sets.

The controller reads the original root's currently running regular-turn
settings separately from the triggering request's account identity. The native
checkpoint and suspension still name that root turn; a child's turn ID cannot
replace it. Native account selection, receipt validation, and each member's
fresh launch admission remain aterm's responsibility. No mailbox reader or
account-selection database is added to Codex.

Thirty focused Rust test results pass for the observer, tree barrier/restoration,
authenticated request attribution, host markers, existing recovery, and quota
eligibility. The authenticated child test holds a separate root request open,
triggers quota in an internal child, and verifies both the child's request
identity and the unchanged root recovery options. Native end-to-end child-quota
qualification remains pending; these unit results do not establish it.

## Native original-child continuity qualified

The unchanged `--pending-mail --active-child` oracle passes with canonical
private package `0.1.0-dev.14730+codex.0.166.0.g6fec01104` built from
`6fec0110458e63df4de673bd999c58ccc4525e71`, using native host source
`4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250` (SHA-256
`e1cf16a318fed16ec4fdb42b10d756d12615d8c69b6c2c8db7daf9eb8cb63d79`).
The run is `codex-native-tree-active-child-6fec011045` under `.aterm/.ubd`.

The parent retains its PID and original turn, makes three provider requests,
and preserves the accepted mail and JSON item identities. The child is held
in flight across replacement, makes three provider requests, and has two
starts bearing the same original turn ID followed by exactly one completion.
There is no child abort or replacement turn. Both the general oracle and the
child-continuity oracle report `passed: true`.

This establishes continuity when the parent reaches quota with an active
child. It does not establish that a child's own quota failure triggers a tree
handoff, nor live email authentication or arbitrary multi-session fleet
placement. Those require their separate fixtures and live evidence.

The same `6fec011045` package and `4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`
host also pass five additional native runs: `--drop-consume-ack`, `--chain`,
`--exhausted`, `--pending-mail --token-usage`, and
`--pending-mail --child-followup`. Their run directories are
`codex-native-6fec-{consume-ack,chain,exhausted,token-usage,child-followup}`.
The repeated rotation retains the original turn across four parent requests;
exhaustion makes no extra model request before owner cancellation. Token
accounting preserves 30 input, 10 cached-input, 5 cache-write-input, 12 output,
and 5 reasoning-output tokens. The completed child finishes its distinct
follow-up after the parent switches. All five `result.json` files report
`passed: true`. The candidate with child-originated quota observation still
requires its own native qualification.

## Child-triggered native result

The combined child-quota candidate `f12f5ef3e5191d125634c0704dbdd83a45b8b3f8`
passes `alab/check.sh`, and its canonical private package
`0.1.0-dev.14734+codex.0.166.0.gf12f5ef3e` passes package smoke, signature, and
layout checks. No package was installed or promoted.

The native `--child-quota` run with host `4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`
reaches the account handoff and completes both original turns, including the
child's selected-account request. The complete oracle still fails: the parent
emitted `item.started` for its real wait, but never emitted its matching
completion. The source wait was cancelled during transfer and its recorded tool
output says `aborted by user`, although the transfer was automatic. The root
retains its original PID and turn and makes three requests; the child has two
starts of its original turn followed by one completion. This is progress in
account selection and continuity, not a passing end-to-end result.

The retained run is `codex-native-child-quota-f12f5ef3e5` under `.aterm/.ubd`.
The next correction must make wait cancellation during handoff produce a truthful
terminal tool result and close its visible item. Do not relax the JSON-stream
oracle or invent a successful wait to hide the unfinished item.


## Core ephemeral member restoration

Live exec quota members now retain canonical in-memory history even when they
are not roots. Startup initializes an empty journal only for a configured
ephemeral member without a live writer; missing durable storage cannot create
an empty ephemeral checkpoint. The journal omits `SessionConfigured` and
`DeprecationNotice`, the startup-display records which the durable rollout
policy also treats as transient. Model input, private records, original turn
boundaries, and compaction context remain lossless.

The same memory codec now validates either an exact active turn or idle state.
Tree capture includes an idle member's model context and journal. Restoration
keeps that member idle, with no synthetic turn, until aterm admits its pending
input. The captured history prefix and runtime settings are still checked;
no new startup-history exception is permitted.

Validation passes 17 ephemeral tests, five tree-capture tests, seven child-restore
tests, and eight app-server recovery tests. Coverage includes active and
never-started idle members, missing durable storage, mismatched turn state,
cancelled waiters, and retained mail after refused idle admission. All-target
Core Tippy found no ALab diagnostics when allowed to finish reporting four
`redundant_clone` findings in upstream tests. The normal all-target invocation
stopped on those upstream findings; this does not replace the full gate or
native ephemeral qualification, which remain required.

The native ephemeral-root probe exposed a separate entry-point gate: exec still
passed `enabled=false` to its handoff controller for `--ephemeral`, so the
existing memory recovery code was never reached. The controller now includes
ephemeral regular turns. Review operations retain their existing restriction.
`exec_accounts.py --ephemeral` checks root identity, retained tool output,
selected-account use, JSON continuity and the absence of session rollouts.
`--ephemeral --child-quota` uses a separate memory-only oracle: allowlisted
provider request IDs must retain the child's original thread, turn and parent,
and `list_agents` must report that child's expected completion before the root
finishes. It rejects interrupted, failed, missing and replacement children.
Twelve child-oracle tests and eighteen shared fixture tests pass. The previous
private package times out with the controller explicitly disabled; the
entry-point correction still requires a newly built package and native
verification of both root and child cases.

`--ephemeral --child-followup` checks an idle child separately. The provider
waits for the real `list_agents` result to show original completion before it
returns the parent's quota error. After recovery, another status check must
show the child still completed; then `followup_task` must finish a new turn
under the same child and parent identities. The fixture also checks retained
original context, exactly-once tool outputs and the absence of rollout files.
Fifteen child-oracle tests and eighteen shared fixture tests pass, including
rejection of interruption, changed identity and reuse of a completed turn.
Native execution of this idle-child case remains required.

Completed idle children also need their observed status restored. Upstream
startup restores an interrupted status from history but otherwise leaves the
new runtime pending; an idle completed member therefore appeared unfinished
after a handoff. The child loader now derives the last task status from its
validated canonical history while holding the inactive-turn lock. It ignores
source-process shutdown and emits no new completion event. Running history is
rejected rather than reclassified as idle, and native admission is unchanged.
All eight child-restoration tests pass, including persisted and ephemeral
completed-child cases that failed with `PendingInit` before the correction.
