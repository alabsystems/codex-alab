# Live upgrade: resource custody across exec

Package admission remains the native package owner's job. Its original child
transfers an opened image and the original close-only lease, with a bounded
receipt. Codex authenticates that original child, validates descriptor identities,
acknowledges receipt, and requires successful child exit. The typed candidate
probe separately establishes compatibility with that admitted physical image.

The process that commits an upgrade must retain this exact image and lease through
exec. Re-reading the launch shim would choose a potentially different release;
letting CLOEXEC discard the lease would leave a gap in garbage-collection custody.
The original launch arguments and environment must remain intact.

`upgrade_exec_custody` prepares a Unix socket pair while the original descriptors
are still held. One complete bounded message queues the original lease, image,
and sending socket endpoint through SCM_RIGHTS. The queued endpoint keeps the
connection and its kernel peer identity alive after the ordinary sender closes.
The receiving endpoint stays CLOEXEC until the final committed exec. An abandoned
channel closes its queued rights without explicitly unlocking the lease.

The next image's first synchronous main hook enumerates inherited sockets. It
does not guess a lease descriptor or accept a descriptor number from arguments,
environment, or a pathname record. It considers only non-CLOEXEC sockets whose
kernel peer PID is this same process and whose queued marker identifies this
protocol. A forked child has a different PID and cannot adopt the source channel.
The hook receives fresh owned descriptors from the kernel, requires the exact
count and typed receipt, validates both opened and named file identities, and
requires the admitted image to be the image actually running. It closes the
transport and retains the original lease, with CLOEXEC restored, for the runtime.
The following exec therefore drops the previous lease while adopting its successor.

This is controlled same-process **resource custody**, not package admission,
source-image authentication, an execution grant, or authorization to restart.
Another program can prepare its own same-process socket before exec; that gives
it no new authority. The native constructor, signed package admission, native
thread/work barriers, goal checkpoint, and final conditional TUI decision remain
independent requirements. No permission is reconstructed from this receipt.

The compatibility response uses protocol 2 and advertises custody protocol 1 only
after the early startup hook actually ran successfully. Merely importing the
library does not advertise support. Earlier protocol-1 candidate responses are
incompatible with this transfer.

If a host cannot inspect its inherited descriptors, an ordinary launch remains
usable and advertises no custody capability. A recognized malformed handoff is
an error. Inspection does not supply permission to bypass a host boundary.

Required evidence includes real same-PID exec with the original lock continuously
held, rejection of another image and another PID, CLOEXEC on the adopted lease,
and release after final custody closes. These fixtures test kernel custody, not
signed-package admission. Actual native signed-package admission and a compiled
Codex probe are separate integration checks. This component alone does not prove
an automatic upgrade or authorize removing the active-goal guard.

## Admitted execution plan

The updater resolves the authenticated control connection's kernel peer image,
retaining its opened file and process identity while native admission runs. PATH
does not select that host. The candidate's directory number is only a request
hint: the native owner must authenticate that exact build and its returned image
must match the requested physical entrypoint. A changed or unavailable host,
claim, target, or compatibility response keeps the existing session running.

Preparation captures the native lease, typed compatibility result, exact physical
exec path and original resume arguments, environment and working directory.
The original launch shim remains argv[0] for subsequent updates. A preparation
permit follows the blocking worker and its resulting plan, so cancelling its
async waiter cannot strand a probe or admit a duplicate preparation.

At the conditional TUI decision, the candidate must still match the launch path,
its original file identities must remain valid, and the native generation and
policy must still commit. The opaque execution belongs to that same Shared
session. Storing it follows the successful native decision under one ownership
lock, with no fallible operation afterward. Teardown consumes only that committed
execution; it never reconstructs an executable from a changing shim. A later
installation cannot substitute another image after commitment. The custody socket
becomes inheritable immediately before exec, after argument construction, working
directory restoration, signal reset and child teardown.

Native package admission is replaced only in explicitly selected test fixtures.
The fixture backend is absent from normal product builds; it makes no signature
claim. The actual native admission and real CLI custody integration checks remain
separate. These changes do not remove the active-goal guard or substitute for the
native tree/input barrier and transactional goal checkpoint required next.

The production execution-path fixture commits a prepared plan through Shared,
changes the launch shim after commitment, and invokes exec_after_teardown. The
compiled CLI reports the original physical image, unchanged PID and adopted
custody; the original lease remains locked until that process exits. This is an
execution-path proof, not an automatic published upgrade or active-goal proof.

## Manager-wide start and queue admission

The tree-scoped exec fence does not cover other loaded roots. The process-handoff
barrier therefore belongs to the actual ThreadManagerState and is shared by each
root and private descendant. Its native state machine is shared with goal transfer
through upgrade_barrier; neither consumer grants execution permission.

A prepared manager transfer retains the registry read guard and its exact loaded
threads. Startup and input remain available while it is being prepared: their
admission revokes the attempt before any mutation. Cancellation cannot resurrect
that attempt. A final conditional commit serializes with those admissions and
refuses if any admitted operation is still outstanding. Only a committed source
refuses new work. Shutdown can still reach the native teardown after commitment;
before commitment it is an ordinary tracked operation that revokes preparation.

The submission sender retains native admission in the Submission itself, through
bounded-channel backpressure, forwarding, assignment deferral and dispatch. A
request already accepted remains tracked if its caller disconnects. The dispatch
loop explicitly keeps the admission outside the closure that consumes the Op, so
partial field moves cannot release it before an awaited handler completes.
Forwarding preserves the actual manager owner. Fresh isolated owners for raw
channels exist only in unit-test builds.

This barrier inventories every loaded root and private ephemeral member; it does
not establish idle state, flush writers or restore their state. The updater must
combine it with that evidence and the native goal checkpoint before committing.
It is not yet wired into the updater's final decision, and the active-goal guard
remains. In particular, a primary-thread-only resume is not full-tree continuity.

While this registry guard is held, adapters must use its captured thread handles
rather than reacquiring the manager registry. Tokio's writer preference can put
such a new read behind a startup writer that is itself waiting for this guard.
The startup already revokes preparation, so the correct response is rollback,
not another registry lookup. Existing facts collection must be adapted to consume
this inventory before the barrier is wired into the updater.

Validation for this component: four new Core checks pass; the 30 selected native
tree-capture, child-restoration and forwarding cases pass with the gate's existing
`RUST_MIN_STACK=8388608`; 81 autopilot/update/goal checks and seven core admission
checks pass. The final scoped all-target Tippy run has no owned, spanning, failing
or missing-build-script findings (25 upstream diagnostics set aside). The initial
ad hoc restoration run omitted the gate's stack setting and overflowed; no source
or assertion was changed to resolve that runner mismatch. These are component
checks, not a canonical release qualification or a published automatic upgrade.

## Updater integration of the native inventory

After target admission and owed-act selection, the updater resolves its actual
primary Core thread and retains that manager's barrier. Its final asynchronous
facts scan uses only the captured native handles; it never reacquires the registry
behind a pending startup writer. The conditional TUI callback combines the native
manager transfer with the existing supervisor policy and Shared-generation commit.
New work invalidates the transfer even if it completes before that callback.
Cancellation drops the barrier while a stale TUI event can still be queued. An
unavailable inventory schedules the existing bounded re-look rather than relying
on another package installation notice.

An empty Core automatic-mail probe used to acquire start admission after emitting
the turn's idle lifecycle event. It could revoke an otherwise safe upgrade queued
by that observer, despite having no work to start. The ALab hook now checks for
pending work before taking admission; actual extraction still occurs only under
both native start and assignment admission, in their original order. The existing
readiness test failed before this correction and passed afterward. Its assertions
remain unchanged, with diagnostic context added on failure.

The final callback also compares the prepared resume arguments, environment and
working directory against the current native session plan. The physical image is
validated separately by the admitted execution path. A conversation switch after
target preparation cannot silently authorize resuming the previous conversation
merely because the later Shared generation is current.

These changes preserve the active-goal guard. Native idle-state persistence,
complete private/ephemeral thread restoration, direct mailbox custody, and the
transactional persisted goal handoff remain required before enabling active-work
updates. The manager's loaded-thread inventory alone does not prove those facts.

Validation of this integration: 85 autopilot/update/goal cases and seven shared
admission cases pass; 32 Core tree-capture, restoration, forwarding and pending-work
cases pass with the gate's existing 8 MiB stack. This includes the production
conditional updater with native input but no Shared-generation change, cancellation
with a stale queued TUI event, a startup writer waiting behind the captured
inventory, and a conversation switch between target preparation and inventory.
The Core checks distinguish an empty post-turn probe from accepted pending mail
and verify that refused extraction retains the queued message. Scoped all-target
Tippy reports no owned, spanning, failing or missing-script findings; 25 upstream
diagnostics are set aside. These remain component checks, not a release gate or
proof of a published automatic update with active work.

## Idle-root queue custody

An idle primary has no suspended turn to serialize. The input batch now has an
explicit idle-root decoder instead of fabricating an active turn. It requires one
root controller owner, distinct member identities and exact queue boundaries.
All member queues share the same publication identity table, including accepted
mail for unloaded recipients and original receive IDs. Existing active-root data
keeps its encoding; idle children do not acquire controller ownership.

The source preflight requires its actual prepared manager and exact thread
handles. After permanent commitment, retained inventory releases the registry
read lock so shutdown and eviction can finish while those handles keep the
queues alive. Root sealing freezes shared controller custody; accepted eviction
transfers can finish into it. Capture requires completed native session loops,
and the caller must join the shared runtime shutdown. Inter-agent delivery now
acquires manager input admission before publishing observers or locating a
loaded recipient, covering unloaded-recipient acceptance as well.

Restore installation retains controller data without publishing its pending
watch state. Explicit activation publishes that state after installation. The
new notification-order test initially failed because a shared restore helper
published unconditionally; the implementation now respects quiet installation.
The assertion was retained. Root controller data is refused on a child, and
installation is once only. This is data custody, not fresh execution admission.

This component remains separate from the production updater. Full forest
configuration and history restoration, fresh native admission, persisted goal
commitment and TUI input fencing are still required. The active-goal guard stays
in place; these changes do not establish seamless published automatic adoption.

Validation: all 48 selected Core tree-capture, manager barrier, input batch, idle
restore, controller mailbox and queue-only checks pass with the existing 8 MiB
stack setting. This includes actual committed manager capture, refusal before
native shutdown, controller mail accepted before sealing and retained through
late eviction, shared publication identity, malformed root ownership and
notification ordering. These are component checks, not release qualification.

The final scoped all-target Tippy run completed successfully. The native gate's
lint classifier reports no owned, spanning, failing or missing-script findings;
25 upstream diagnostics are set aside. This covers Core and the ALab Core, TUI,
Autopilot and CLI packages, including the CLI build script.

## Shared native idle destination restoration

Idle roots now bind their decoded controller frame to an actual Core destination
thread. Roots and descendants use one retained restore operation. Installation
still requires the caller's fresh native admission for that exact thread; no
configuration, source snapshot or queue data authorizes delivery. Activation is
separate and refused until successful installation. Native refusal is terminal
for the attempt, including retries.

An owned task retains the operation across cancelled waiters. Repeated calls
observe its original result and do not run another admission callback or enqueue
the data twice. The destination is checked against the manager's exact Arc and
running state before admission. Because admission can await a native host, the
same registration is checked again afterward and retained through installation.
A shutdown during that wait cannot install into a retired owner. The registry
read lock is acquired only after native admission returns and held only through
local installation; its targeted lint expectation records that invariant. The
shutdown-during-admission test would time out if the host wait retained it. Root binding
rejects child session sources, mismatched thread IDs and frames without root
controller ownership. Existing child restoration delegates to this shared code.

This is the destination queue operation needed by idle-root bootstrap. The
current tree bootstrap still requires an active root; complete forest snapshots,
startup wiring, fresh SDK membership and goal/TUI handoff are pending. An
admission callback in a component test is not proof of native SDK integration
or automatic adoption from a published package.

Validation: 52 selected Core tests pass, including the new actual-manager idle
root admission/cancellation/retirement cases, existing child restore refusal and
input retention, and source capture/codec/mailbox regressions. No test is ignored.
The runtime code was fixed before this run; the subsequent source edit only
documents the targeted lock lint expectation described above.

The final scoped all-target Tippy run succeeds with no owned, spanning, failing
or missing-build-script findings; 25 upstream diagnostics are set aside. This
includes Core and the ALab Core, Autopilot, TUI and CLI packages. These checks
do not constitute release qualification or a published automatic upgrade.

## Explicit idle tree bootstrap

Tree snapshot version 4 binds an optional root turn: `Some(original)` for a
suspended active root and `None` for an idle root. It never fabricates a root
turn. Active source capture still requires its real original turn. The existing
active native grant manifest explicitly rejects an idle root pending native
protocol support; its receipt validation binds `Some(grant.origin.turn)`. Older
snapshot versions are rejected.

Idle capture selects the committed original members sharing that root's local
runtime, orders parents before descendants, and retains final configuration,
settings, tools, capability roots, persistent history evidence, ephemeral
context/history and input queues. The full snapshot requires the marker produced
only after `shutdown_exec_tree` joins both the root and all runtime producers.
Session-loop termination alone cannot establish that late deliveries settled.
The codec validates the same history, helper, parent, identity, queue ownership
and membership invariants as active snapshots. Each root keeps its own tree and
controller data even when several roots share a manager.

The embedded bootstrap now loads either persisted or ephemeral idle-root history
and stages its input without a pending active turn. Root binding lets private
children validate their actual owning idle runtime. Destination preparation
retains the shared root idle installer. Explicit idle activation requires fresh
admission for the root and each idle child, installs queues before releasing the
launch barrier, and publishes delivery afterward. An all-idle launch begins
ready but still requires explicit release and supplies no original-turn permit.
Active and idle activation entry points reject the other origin kind. Their
owned operations retain the same result across retries and cancelled waiters.

This is a tree bootstrap component, not whole-manager adoption. Full forest
preflight/atomic transfer, persisted goals, native SDK membership retirement,
TUI input fencing and production updater wiring remain required; the active-goal
guard remains. No new version has been published by this component work.

The first runtime run passed ephemeral idle bootstrap but found a durable
never-started child missing after shutdown. Diagnostics located the failure in
the source child's closed-store history, before destination bootstrap. Its
lazy live writer had not materialized a durable rollout. Committed idle sealing
now uses the existing writer's standard persistence fence and flush before
closure, for both manager-upgrade and original exec-tree sealing. Actual
ephemeral members retain their established journals. Missing durable evidence
is still an error; the test was not changed to make that child ephemeral.

That persistence fix made all three idle bootstrap cases pass. The wider review
checks then found a completed delegate whose writer was already closed. The
sealing operation now accepts that exact `ThreadNotFound` condition only when
the canonical store still returns history for the same thread with no unfinished
turn. Other persistence errors and absent durable evidence fail. This preserves
completed private delegates without reopening a writer or changing durability
mode. The normal materialization path remains necessary for lazy durable members.

Final validation: all 15 selected app-server tests and 59 selected Core tests
pass, with no failures or ignored tests. They exercise actual embedded bootstrap
for persisted and ephemeral completed idle roots, native refusal and idempotent
activation; active tree recovery; completed/frozen and ephemeral review results;
helper account bindings; native queue capture/restore; joined shutdown; and
independent roots/private descendants. The new idle cases retain exactly one
source model request through bootstrap and repeated activation.

Final scoped all-target Tippy succeeds for Core, app-server, CLI and the ALab
Core, TUI and Autopilot packages. The gate classifier reports no owned, spanning,
failing or missing-script findings; 25 upstream diagnostics are set aside.
These remain component checks, not canonical release qualification or proof of
a published automatic same-PID update with an active goal.

## Manager-wide idle forest preparation

The source can now prepare every loaded root and its registered private members
under the actual manager barrier. It partitions by native runtime identity,
orders parents before descendants, and refuses unowned members, missing roots,
retired roots, active work and changed parent bindings. Each member completes
writer preparation and portable history/context/configuration/settings checks
before the final conditional commitment. A late admission revokes the attempt;
dropping an uncommitted plan leaves its runtimes running. The final decision
still has to include persisted goals, TUI input and native execution admission.

Committed capture retains the exact original handles, seals all queues, releases
the registry guard, joins each root's producer shutdown and validates the final
forest against the original native inventory. Its owned operation survives a
cancelled waiter and records one outcome. A capture error does not authorize
exec; production integration still needs its complete failure/recovery policy.

The forest codec stores tree records separately from one global input table.
Encoding each tree's publications independently would split consume-once
identity across roots. Its decoder restores the table once and returns each
tree with its matching prepared input, once only. Per-tree wire input subsets
exist for validation; destination integration must use the paired prepared
inputs, not decode each tree independently. The native receipt's member list
is supplied externally to validation so an omitted tree is rejected.

Ordinary explicitly ephemeral sessions now acquire canonical memory bookkeeping
before startup records, through one manager hook. This private marker grants
neither headless-exec nor quota membership. Durable sessions still require
their actual store; an absent writer is not sufficient to create memory state.
Internal inheritance of exec/quota markers retains its existing exec-parent
condition. Accepted mail still in the input channel is moved into the same
ordered pending queue during portability preflight, without publication or
consumption; opaque lifecycle state is rejected before source shutdown.

This is source forest preparation and a shared-input codec. Destination forest
bootstrap, fresh per-root native membership, whole-forest activation, goal and
TUI transfer, updater integration and published adoption remain required. The
active-goal guard remains. The existing single-tree exec bootstrap must not be
used as a TUI execution-policy substitute.

Validation: 15 app-server and 68 Core tests pass, with no ignored tests. The
ordinary ephemeral bootstrap case completes real source model work without a
headless source marker, then preserves it through the embedded boundary. Native
forest checks cover all registered roots/private members, cancelled capture
waiters, joined producer shutdown, exact inventory/primary binding, omitted
trees, active-work refusal, late-input revocation and retained opaque mail. The
input codec check consumes a publication in one decoded root and observes the
same consumed state in another root's partition.

The late-settings regression initially retried while its accepted operation
still held native admission. The final test waits for that exact pending state
to retire, then verifies active refusal and starts another real session to prove
the manager remained open. No readiness check was bypassed. Final scoped
all-target Tippy has no owned, spanning, failing or missing-script findings;
25 upstream diagnostics are set aside. These are component checks, not a
canonical release qualification or published automatic adoption proof.

## Destination forest bootstrap

The embedded runtime accepts one destination-owned root startup policy per captured
root and an external inventory of original member IDs. It decodes the forest's
shared input table once, then moves each prepared partition into its root's load.
It does not prepare a second input table from each tree's validation copy; doing
so would split publication identity across roots.

All roots and descendants load before a single launch barrier is bound to their
fresh runtimes. Activation installs every idle queue under the separate native
admission callback, waits for the shared barrier, releases it once, and only then
publishes idle mailbox notifications. A refusal shuts down all prepared trees.
An activation task belongs to the forest owner, so losing a transport waiter does
not cancel a partially installed forest. Retries reconcile the same result.

Ordinary interactive roots and their private children receive no HeadlessExec
marker from captured history. Exec identity is chosen by the destination startup
policy; active original-turn recovery continues to require that exec policy.
These APIs are embedded-only, with no new JSON-RPC method. Startup policy and the
queue codec supply neither native SDK constructor membership nor execution grants.

This component still needs the production TUI/goal handoff and actual native
reader retirement before it can support a live release transition. A passing
restore fixture is not evidence of an automatic published-version adoption.

The retained-trigger fixture exposed a missing scheduling step: notifying the
local input queue alone wakes input observers, but does not invoke submission
loop pending-work scheduling. After release, idle activation now asynchronously
runs the same pending-work scheduler under the native mail conditions (trigger
mail, wake-on-agent-messages, or durable sleep). It does not submit a fabricated
user turn. All native admissions and queue installations precede this step.

Validation: the native Trust 15698 lane passed all 18 selected app-server restore
tests and 68 selected Core tests (86 passed, zero failed or ignored). The three
new embedded forest cases retain a persisted root, an ordinary ephemeral root,
and its private ephemeral child. They assert zero destination model requests
before the final root's admission, zero after refusal, and one completed turn
after successful release, including a cancelled transport waiter. Source mail
uses the public queue-only submission operation; start-or-steer intentionally
forces work and cannot serve as an inert-queue fixture. The normal turn-complete
event precedes final task cleanup, so fixture preflight waits at the actual idle
boundary with a bounded deadline. Production admission refusals remain errors.

The all-targets scoped tippy verdict has no owned, spanning, failing-target or
build-script findings; 25 upstream diagnostics remain set aside. These checks
qualify this data/runtime component, not native SDK epoch retirement, the TUI and
goal commit, signed publication, or a live published-version transition.

## Joining source reader closure with forest custody

Source catalog retirement joins the final synchronous decision rather than
sealing constructors throughout revocable queue and goal preparation. The actual
registry closes root admission, pins its observed old domains, checks constructor
and work readiness, and runs the host decision while holding that admission gate.
A refused decision or pre-effect error explicitly reopens it. If the host callback
unwinds, the admission and census locks are released before unwinding continues,
so the staging guard can reopen the catalog without poisoning it. Acceptance
marks the old cohort retiring under the same gate and returns an opaque local
owner; dropping it cannot reopen the committed catalog.

That owner drains through one retained task. Losing a waiter does not cancel
retirement, and retries observe its recorded outcome. Existing reader retirement
still requires its actual native acknowledgment; cached manager/domain counts do
not replace that evidence. The original observed domains stay pinned through
cleanup. This controls source closure only and grants no destination constructor
or execution permission.

The cancellation/resource-body case exposed an epoch bug in the underlying
catalog: a constructor refused after closure could inherit an old resource
parent's epoch despite never being admitted. Dropping that deferred constructor
then made it an Unknown member of the old census. Refused factories now belong
to the current deferred epoch. Admitted descendants retain their actual parent's
epoch. Unknown old owners still refuse; they are never treated as retired.

`PreparedUpgradeForest::commit_with_readers_if` combines its exact manager barrier
with this process's actual global reader catalog and the caller's final decision.
The owned source capture joins all runtime producers, builds and validates the
forest data, then waits for reader drain before returning that capsule. Its
separate Core-only `commit_if` continues to mean data custody, with no SDK closure
claim. The production TUI/goal driver must choose the combined path; its current
active-goal guard stays until the complete handoff is assembled and verified.

Validation for source reader closure: the complete ALab core suite passed 261
cases. After final lifetime-ordering and Core integration changes, the selected
combined lane passed 18 catalog/native-reader, 18 app-server restore, and 69 Core
cases (105 passed, none failed or ignored). The new isolated Core process retains
three original members, proves a rejected host decision reopens the catalog and
keeps Core preparation valid, then commits and captures the exact forest only
after reader drain; retries retain one snapshot and all source roots are stopped.
The new catalog tests cover pending work/loading constructors, unknown owners,
refused and unwinding decisions, actual retained domain custody, and a cancelled
drain waiter held pending by a real resource activity.

The actual AuthManager/direct-reader adapters compile in codex-login rather than
Core. Their separate native fixture run passed 34 cases, alongside another 18
catalog cases. The final all-target scoped lint verdict has no owned, spanning,
failing-target or build-script findings, with 25 upstream diagnostics set aside.
These are source-catalog and controlled SDK fixture results. They do not prove a
fresh constructor or active-goal/TUI continuation across a real published-image
exec; that release-to-release test remains required.

## Preparing every native goal state

Goal preparation now captures a readable native row or its actual absence under
the runtime's mutation permit. It validates the expected original thread ID
before reading, flushes native idle accounting, refuses persisted deferrals and
standing turn/recovery work, and keeps a revocable goal admission token. Goal
tool availability is separate from data readiness: a private ephemeral runtime
still needs its absent goal state protected even though it cannot execute goal
tools. The existing active-only checkpoint and commit retain their execution
readiness checks. Neither state capture nor state commit grants continuation.

`PreparedGoals` retains the exact Core thread owners, actual native goal handles,
rows and transfers for a nonempty unique member inventory. Missing runtime or
unreadable storage is an error. Preparation revalidates earlier guards after
later asynchronous reads. Final commit nests all actual goal admission boundaries
around one host decision; rejection leaves the earlier guards revocable. The
forest composition checks actual Arc identity against its retained members,
rather than accepting matching thread IDs from another runtime, then joins the
Core inventory and source-reader catalog decision.

The new three-member test exposed a fixture gap: LateGoal manually initialized
the original root but never forwarded later thread starts. It now forwards the
native start input after binding, just as the application installs the extension
for every thread. The tests retain a primary with no goal, a hidden paused root,
and an ordinary ephemeral private child with no goal. They cover partial forest
inventory refusal, wrong runtime ID, duplicate/empty input, hidden paused-objective
mutation, host refusal/rollback, and closure of every goal admission after one
accepted goal decision. A separate case proves creating a goal revokes an earlier
absent-state checkpoint before persistence. Paused-to-active resume would start
real work, so the stable paused-state fixture changes the paused objective;
active-goal accounting remains covered by the existing native checkpoint tests.

Validation: 38 focused native handoff tests passed, followed by 37 autopilot,
two app-server and three Core goal regressions. The final all-target scoped lint
verdict has no owned, spanning, failing-target or build-script findings; 25
upstream diagnostics remain set aside. These counts are separate runs with
some overlapping cases. No test is ignored to obtain these results.

These rows are preparation snapshots, not final handoff capsule values. The
native idle clock keeps advancing while shutdown and SDK retirement run. The complete driver must capture final goal
state after runtime/SDK quiescence, preserve monotonic idle-clock fractions and
the exec interval, and fence the TUI input/event producer before committing exec.
That production path and a real active-goal published-image transition remain
unproven; the current active-goal updater guard stays in place.

## Final source goal accounting

An exact transfer can now report its own committed generation. The final goal
read requires that token to belong to the actual runtime and expected thread,
with no standing turn or recovery work. It flushes native idle accounting and
rereads storage while goal mutation admission remains committed. It does not
reopen admission or grant continuation. GoalExtension's thread-stop hook itself
only retires admission recovery and unregisters the runtime; the final flush is
needed because real idle time continues after the preparation snapshot.

`PreparedGoals::into_capture` requires every goal transfer committed, source
reader commitment present, and the exact same Core member Arc identities. The
owned `GoalForestCapture` first awaits the committed Core forest, including all
source producers and reader acknowledgments, then performs each final goal flush
and read. A lost waiter cannot cancel that work. Retries retain one outcome and
one final result Arc. Preparation rows remain distinct from these final rows.

The new isolated child test joins the actual three-member Core inventory, all
native goal guards, the source catalog, and a host decision. It retains an active
primary goal with 70 seconds and 93 tokens, another root, and its private ordinary
ephemeral child. Host refusal leaves both preparations valid; acceptance commits
the combined owners. After 1100 ms of real idle time, a real scratch SQLite write
lock holds final accounting pending while the source root terminates. Cancelling
the waiter, releasing that lock and retrying produces one final result: the same
active goal ID, exactly 93 tokens, increased elapsed time, and a row equal to the
actual store. Every source member terminates. Foreign uncommitted and committed
tokens cannot perform the final read. The child prints a success marker only
after its completion assertions; the parent requires it as well as exit success,
so a renamed exact-test selector cannot pass by running zero cases.

Validation: all 39 selected native handoff tests passed without ignored cases.
Core and app-server packages were present to retain the build graph but had zero
matching runtime tests in this run. The final all-target scoped lint verdict has
no owned, spanning, failing-target or build-script findings; 25 upstream
diagnostics remain set aside. These results prove the controlled source closure
and final accounting path. A portable monotonic clock baseline, exec interval,
TUI input fence, capsule transfer/startup, fresh published-image SDK constructors,
and the actual active-goal release transition still need verification.

## Exact goal clock data across exec

Goal accounting now uses an ALab-owned monotonic baseline rather than an opaque
Rust Instant representation. Its clock IDs match the pinned Trust 15698 standard
library source (`library/std/src/sys/time/unix.rs`, lines 55–74): Apple uses
CLOCK_UPTIME_RAW and other Unix targets use CLOCK_MONOTONIC. The same choices are
visible in the [Rust standard-library source](https://doc.rust-lang.org/src/std/sys/time/unix.rs.html).
The package's actual source excerpt is retained in the clock qualification log.
One stable import hook selects the owned type; a second hook exposes accounting
capture/adoption through ALab glue without exporting native private state.

The clock stamp is a strict domain/seconds/nanoseconds value. It preserves every
baseline nanosecond; native whole-second accounting advances that baseline while
retaining its fraction. Adopting the same source stamp again leaves subsequent
accounted progress intact, and adopting a different stamp after restoration
refuses. Wrong domains, malformed fractions and future baselines refuse. Normal
non-Unix accounting retains its standard clock, while clock handoff is unsupported
and must refuse during revocable preparation rather than after source commitment.
These values are time data; the final transport must independently establish the
original process/boot and SDK admission.

Final goal state now pairs each original thread, row and optional clock stamp in
one record. An active native clock is captured only for its actual goal ID.
Destination clock adoption requires the expected original thread, an enabled idle
native runtime, the exact current persisted goal row, and its already-initialized
native active-goal clock. A rejected stamp does not mark a goal active. The caller
still owns startup and SDK execution gates; this operation grants no continuation.
The production TUI capsule/startup path has not yet been connected to these APIs.

Validation on the final source: 44 focused cases passed with zero ignored,
including an actual same-PID exec and native persisted-goal adoption. The full
goal-extension backend suite passed all 45 cases. Scoped all-target lint reported
no owned, hook-spanning, failing-target or build-script diagnostics; 26 upstream
diagnostics were set aside. Logs: `upgrade-goal-clock-tests-final.log`,
`upgrade-goal-clock-backend-tests.log`, and
`upgrade-goal-clock-tippy-final-verdict.json` in the release qualification log
directory. These tests qualify clock custody, not a published automatic update.

## Final forest and goal data custody

The exec custody protocol is now version 2. Alongside the original image, lease
and same-process connection keepalive, SCM_RIGHTS transfers a read-only snapshot
file. The writer closes and its temporary path is unlinked before rights are
queued. A strict bounded envelope distinguishes explicit absence from a present
empty snapshot; state bytes never need to fit in a Unix socket buffer. Destination
startup rejects a linked, writable, non-regular, oversized or length-mismatched
snapshot and reads from offset zero. Only after validating the original image and
lease does it publish the lease and one-shot data together. The source snapshot is
limited to 256 MiB. This remains data/resource custody, never a native SDK grant.
`PreparedExec::with_state` attaches the settled bytes while retaining the admitted
physical image and original launch arguments/environment/directory.

The owned `upgrade_goal_state` codec encodes the complete Core forest, every
member's final optional goal row, and its optional exact clock baseline. Its
version and original primary must match. Core validates the member inventory,
root order, parent graph and common input/publication table. Goal rows must name
their actual member; a clock can belong only to an active goal. An active row
without a running native clock retains explicit clock absence. Unknown fields,
statuses, malformed stamps and omitted optional checkpoints refuse rather than
silently becoming absence. The destination still has to obtain fresh native SDK
admission and native startup policies, load every root, validate current database
rows, and restore goals before releasing the shared startup barrier.

The controlled native source-capture case now round-trips its actual final
three-member forest and goal rows, retaining exact clocks and counters. Eleven
mutated payloads exercise incomplete/duplicated inventory, wrong row ownership,
malformed clocks, unknown status/version/fields, clock-on-absent-goal and omitted
absence/budget fields. The kernel custody exec case carries 1 MiB of binary state
through actual same-PID exec, consumes it once, retains the original lease and
rejects another image/process. Its bounded parent requires an explicit child
success marker. CLI/signature integration cases require a separately qualified
CLI/native package and are not evidence supplied by these component tests.

Input producer custody remains incomplete: the exact pinned crossterm reader
spawns an unjoined thread and keeps a private partial-sequence parser. Dropping
its stream or discarding buffers is not a lossless handoff. The new data codec and
transport do not claim to solve that issue or enable active-goal auto-updates.

Final validation: 40 Autopilot cases and 19 TUI cases passed. Four TUI cases were
explicitly ignored because they require a qualified CLI/native signed package;
Core and app-server targets selected no tests and count only as graph compilation.
The settled all-target scoped lint run reported no owned, hook-spanning,
failing-target or build-script diagnostics, with 26 upstream diagnostics set
aside. Evidence: `upgrade-exec-state-tests-verified.log` and
`upgrade-exec-state-tippy-verified-verdict.json` in the release qualification log
directory. Earlier fixture failures and compile errors remain in the earlier logs;
they were corrected before both settled runs. No new release or automatic
published-image adoption is established by this change.

## Joined Unix input producer and checkpointable parser

The owned `alab-input` crate removes the detached-reader limitation for its input
source. Its reader holds an independently owned nonblocking description. Both
explicit stop and SIGWINCH wake a socket; an idle reader uses no polling timer.
Freeze sets stop, wakes and joins the actual producer, then returns the same file
and parser. The producer checks stop before polling, after waking and before its
read. Its mutex protects parser consumption, and no read can happen after the
join returns. Input arriving while frozen stays on the original PTY.

The parser preserves decoded event order together with their exact source bytes
and raw-mode policy, typed query responses, resize records, partial input and a
native monotonic Escape baseline. Destination decoding rejects incomplete or
multiple queued events and malformed state. Empty input cannot reset that clock.
The optional quarantine state retains security-screen semantics across a
checkpoint; the update path must never discard accepted input to obtain a fence.
Serialization checks a conservative expansion bound before allocating its output.

The private sequence parser and framing rules are adapted from the exact pinned
crossterm revision, with the original MIT license retained. The public event
types remain crossterm's, without enabling a new crossterm feature or editing its
upstream lock entry. The new package has only an ALab-owned lock entry. A cold
standalone build exposed existing `alab-core` library uses of Tokio select macros
that its normal dependency declaration did not enable; the declaration now names
that existing requirement instead of relying on another consumer's feature union.

This is not yet the production TUI input source. All Unix startup replay,
security-screen reads/quarantine, runtime cursor queries and the event broker
must converge on the same owned driver. Kitty reply filtering also retains its
own queue/candidate state and cannot simply be dropped during handoff. The live
upgrade must join this input fence with Core/goal/SDK commitment, wait for source
capture while its runtime is alive, and attach these bytes to the authenticated
state transport. Existing active-goal upgrade refusal remains in place.

Settled validation: all 44 input cases passed with zero ignored, including every
byte split of UTF-8, modified keys, mouse/focus, paste and query replies; native
PTY join/resume with a split paste and input arriving while frozen; twenty idle
join cycles and SIGWINCH wakeup; and actual same-PID parser exec restoration with
a required child success marker. Process-wide resize fixtures are serialized.
Reader descriptors are CLOEXEC, and the join wake retries EINTR rather than
risking a sleeping producer. Scoped all-target lint for the input and core crates
reported no owned, hook-spanning, failing-target or build-script diagnostics and
zero set-aside diagnostics. Logs: `upgrade-input-driver-tests-settled.log` and
`upgrade-input-driver-tippy-settled-verdict.json`. These are component checks;
the production TUI and published-image transition remain unverified.

## Production TUI input ownership

The ALab binary now routes Unix TUI event streams, startup replay, protected
startup reads/quarantine and runtime cursor queries through one owned broker.
Binary identity selects this infrastructure consistently through enhanced/vanilla
mode changes. Other library consumers and Windows retain their existing reader.
Five upstream files have only small marked calls/source-type hooks; parser,
reader, broker and frontend implementation remain ALab-owned. No upstream lock
entry changed.

Subscriptions retain the original terminal description and parser across pauses.
The final stream drop joins the actual producer, while another active subscriber
keeps it alive. Typed cursor queries filter their own response without consuming
user keys; poll peeks instead of removing an event. Wakeups fan out to every
registered consumer, and departing queries and rollback wake remaining waiters.
The input factory matches crossterm's selection of a terminal on stdin or its
controlling-terminal fallback, opens an independent nonblocking description and
checks a named stdin terminal against the original held descriptor identity.

Reader construction transfers the parser only after its thread starts and
acknowledges initialization. A rejected constructor leaves caller data intact;
joining after an I/O failure also recovers accepted parser data while the broker
retains the original input description. Preparation joins the producer, retains
a bounded checkpoint and fences every new subscription/replay. Its conditional
commit keeps that input gate held through the caller's final synchronous decision.
False decisions remain revocable; committed input never silently reopens.
Preparation follows the actual source subscription, not a guessed global reader.
The caller still checks higher filter/composer state under the owning TUI lock.

A controlled test runs the actual production event-source factory and broker on
a fresh controlling PTY. It establishes native ALab identity, verifies the owned
source variant, receives the first key and preserves a partial paste. While
holding the TUI broker lock it checks that its higher filter has no candidate or
ready event and prepares that exact subscription. Pause then drops the source.
The parent writes the paste suffix while frozen; a non-consuming kernel query
proves it remains on the PTY. After rollback/resume, the production source returns
the complete paste followed by the next key. The parent requires the explicit
child success marker and successful exit, with a bounded watchdog and reaping.
The separate frontend child exercises replay, peek/read, cursor filtering with
user keys, protected OSC quarantine/typeahead and a fresh key afterward.

Validation before integration with later main changes: 47 input cases and all
352 ALab TUI cases passed (four package/CLI cases require explicit qualified
fixtures and remained ignored). The final focused TUI run passed all 46 cases,
including the controlling-PTY production test, with zero ignored. Scoped
all-target lint for input, core, ALab TUI and TUI reported no owned, spanning,
failing-target or build-script diagnostics; 24 upstream diagnostics were set
aside. Logs: `upgrade-input-broker-owned-tests-verified.log`,
`upgrade-input-broker-tui-tests-custody-final.log` and
`upgrade-input-broker-tippy-custody-final-verdict.json`.

The first broader TUI run passed 6,366 cases and failed four. All three snapshot
cases passed when rerun separately on this source. An exact unchanged-base
comparison at `67a030915baa3d880ee1787bbae4076f92bbef8c` reproduced the owned
native UI catch-up positive-control failure and the two snapshot failures that
followed the first snapshot case in its shared process. The catch-up fixture
completed its native turn but accepted no upgrade on either tree; its dummy
image fixture is not a signed-package transition proof. This existing failure is
recorded, not waived or presented as a passing full gate. Evidence is in
`upgrade-input-broker-unchanged-base-tests.log` and the three
`upgrade-input-broker-isolated-*.log` files. The new source was restored after
that comparison and every saved tracked-file hash was verified.

The full updater still has to join this source fence with Core/goal/SDK capture,
await capture before TUI/runtime teardown, transport the complete state and
restore it before work can run. Destination terminal startup must also respect
restored partial input: an unaware fresh query probe could mistake paste-body
bytes for query replies. Higher kitty-filter state is not serialized by this
change; an eligible handoff must check the actual filter before fencing, as the
native test does. The active-goal guard and end-to-end published-transition
qualification remain outstanding. This change alone does not establish the full
self-update objective or a new published release.

The final integration preserves remote main through
`d12c7bd97098fc4f459c11a17a412b0e188c8d54`, including the intervening defaults,
account-registry work and reasoning-selection hook. The clean combined tree at
`c9784acded0350b379a181e3ad6e99b69ad72607` passed all 264 core, 47 input and
352 ALab TUI cases, plus the 46 focused TUI cases. After merging the additional
reasoning-selection hook, the final combined tree at
`9e0ec85bae2cd12cea9dd59b9ac7cddfb390f5ee` passed 49 focused TUI cases including
its reasoning checks and the production PTY handoff, with zero ignored. The
final all-target scoped lint verdict is clean with 24 upstream diagnostics set
aside. Four explicit package/CLI integration fixtures remain unexecuted. These
results do not supersede the recorded pre-existing catch-up-fixture failure or
prove a published-image transition. Logs: `upgrade-input-broker-merged-*`,
`upgrade-input-broker-final-main-tui-tests.log` and
`upgrade-input-broker-final-main-tippy-verdict.json`.
