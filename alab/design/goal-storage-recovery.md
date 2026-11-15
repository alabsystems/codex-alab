# Goal persistence after storage failure

Decision, 2026-11-13: treat failed goal persistence as backpressure. A saved
`active` row is insufficient authority for another automatic model request when
its last usage or stop write failed. Person input continues through Core's normal
native path.

## Reproduced cause

SQLx's SQLite worker tracks transaction depth separately from SQLite. A bounded
`PRAGMA max_page_count` test produces `SQLITE_FULL` (13); SQLite implicitly rolls
back the transaction, while the queued SQLx rollback fails because no native
transaction remains. SQLx retains nonzero tracked depth. A later `BEGIN IMMEDIATE`
on the same physical connection fails with `InvalidSavePointStatement`, whose
message is `attempted to call begin_with at non-zero transaction depth`. Reads can
still succeed.

The pool's ALAB `after_release` callback drains an ordinary queued rollback using
`ping` before inspecting tracked depth. A still-tracked transaction discards that
physical connection. A healthy ordinary dropped transaction retains its physical
connection. This applies to the existing pool initializer without replacing the
transaction or database layer.

Before this change, failed usage accounting prevented `update_goal` from reaching
its requested terminal status. The idle continuation then read the unchanged
active row and admitted another turn. Some interrupted/refused-turn error paths
also discarded unflushed accounting state.

## Write ownership and admission

The shared ALAB persistence state belongs to native goal accounting, so runtime
and tool mutations see the same barrier. Dispatched additive usage writes own
both the native progress semaphore and baseline finalization through a spawned
task. Cancelling the observer does not cancel that task or charge its snapshot
again. Failed writes retain turn usage, descendant usage, and wall-clock
baselines. New person turns do not discard older failed turn records.

A failed or unfinished write holds native goal continuation, ALab's automatic
input permit, and upgrade checkpoint readiness. A healthy unfinished write uses
the existing single paced admission recheck, because its cancelled observer may
produce no later idle event. Failed persistence retires that recheck. Successful reads and unrelated
successful usage writes cannot erase a failed stop intent. Existing lifecycle
callbacks may retry typed, definitely uncommitted failures such as SQLite FULL,
BUSY, LOCKED, or failure before a write begins. Generic I/O and unknown commit
outcomes hold additive accounting without replay. Ambiguity is published while
the progress semaphore is held, and queued writers recheck it under that same
semaphore.

Explicit native goal changes first drain retained progress, using the original
expected goal ID and recomputing shared descendant/time deltas after each
acknowledged baseline. Failed preparation prevents an ALAB replacement from
discarding the retained state. Only an acknowledged successful mutation clears
the old hold.

Terminal tool and native policy paths drain older failed turns before changing
status, leaving the ending turn to its original native accounting/policy path.
Retained usage for the same original stopped or complete goal uses the existing
native accounting mode for that status. A refused/unchanged update never
authorizes dropping a positive snapshot. A terminal lifecycle guard retires only
the matching ended turn's liveness marker, separately from retained usage, so
successful explicit recovery of an idle turn can permit a later checkpoint.

Physical pool recovery is automatic. A failed goal hold has no disk-restored
event and does not automatically resume after free space returns. It requires an
acknowledged explicit native goal mutation through the existing UI/API or goal
tool; healthy reads, ordinary successful accounting, and idle callbacks do not
clear it. The UI/API mutation can run without starting a new model turn.

An explicit Plan turn first drains old usage under the native goal permit,
before taking the turn-start lock. Once that usage is acknowledged, native Plan
mode clears its clock normally, so Plan time is excluded. If this preparation
still fails, the original observed boundary is quarantined before that clear.
This conservative boundary quarantine has no normal-mutation release path, even
when the failed SQL operation is known not to have committed; its restriction is
attribution after a destructive clock boundary, not a claim of an unknown commit.
The manual Plan turn can run, while automatic work and checkpoint remain held.

A raw other writer can replace a goal without the supported mutation ordering.
Before binding a later manual turn to that replacement, ALAB quarantines the old
goal association and its observed elapsed/descendant boundary, and publishes an
ambiguous hold. Core accepts the manual turn without rebinding its goal accounting.
The original turn records and first boundary observations remain retained; later
manual usage cannot authorize additive replay or erase this quarantine, even if a
raw writer restores the original ID. This is fail-closed evidence, not automatic
reconciliation of arbitrary external database mutations. Supported native
mutations drain before changing the goal and do not enter this quarantine.

Terminal policy counters are evaluated once by their original owned stop; storage
failure never schedules an automatic replay of stop policy. A stop queued behind
a native mutation checks mutation generation after obtaining the native goal
permit. Goal ID and millisecond timestamps do not substitute for this generation:
a successful resume can retain both. Mutation intent and its acknowledgement or
cancellation fence stops queued before and during that mutation. Healthy pre-write validation refusals release their recording guard without
clearing an earlier failure. A failed or
cancelled status write retains an admission hold, including when no usage delta
preceded it. Tool guard fields publish that fence before releasing the native
permit.

The hold and unflushed snapshots are live process state. Native handoff refuses
while they remain, rather than carrying an incomplete checkpoint into a new
process. This does not claim preservation through an external forced process kill.

## Passive runtime status

A settled persistence hold must not advertise work solely because its saved goal
remains Active. Native accounting publishes an atomic admission/writer snapshot
through a weak runtime watcher and the existing extension event sink. Passive
metadata uses a bounded coalescing worker and normal authenticated app-server
output, independently of a database-owning thread listener. Durable goal events
keep their existing route. Neither transport nor UI metadata authorizes admission,
changes the saved goal, clears a hold, or weakens handoff guards.

The host assigns an explicit stream generation and publication sequence under
the native current-runtime publication guard. Selected and inline snapshots keep
enqueue custody through cancellation; an acknowledged newer version prevents an
older cancelled sender from restoring stale bytes. Permanent output closure
retires the passive worker. Distinct-thread pressure is capped at the native
channel capacity plus the worker's selected record. Overflow retains one reset
for the old generation. Acknowledged reset rearms a new generation on the same
authenticated host, and a single abortable background native goal read refreshes
the displayed thread before any database read. This is event-driven metadata
recovery, not goal recovery or another model turn.

The client orders generations across every thread of that backend. A delayed old
packet cannot replace a newer generation, including on a previously unseen thread;
an old reset invalidates only its own generation. Host replacement requires an
observed native transport-loss boundary. Backend switches, thread switches, loss,
close and superseded refreshes retain their native ownership boundaries. Unknown
or unavailable observations preserve the native conservative status. Actual turns,
pending input, live writers, MCP startup, modal blockers and background terminals
still report their real activity. The saved Active badge and objective remain.

Older peers that do not understand the optional ALAB notification keep native
behavior. This status integration and its new regression controls are prepared
source at this point; the earlier runtime results below do not qualify this change.

## Validation

The production `codex-state` pool tests use a tiny database page limit, never
actual volume exhaustion. They assert the unprotected failure, physical connection
identity, retained objective and 11 tokens, followed by exactly 18 after a recovered
7-token commit. The ordinary rollback test ensures healthy connection reuse.

ALAB persistence tests exercise observer cancellation, sticky stop holds,
ambiguous outcomes, queued writer rechecks, and mutation-generation fencing.
Native Autopilot tests exercise failed writes with healthy active-goal reads,
zero-usage external stop failure and recovery, and a delayed old stop racing a
same-goal reactivation. Primitive tests do not replace native integration or the
normal full certified application gate.
