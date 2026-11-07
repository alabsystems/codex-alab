<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native assignment custody

This is the implementation contract for the missing native seal and target
re-admission path. It describes required behavior, not a qualified capability.
SourceV1 remains unadvertised until the actual producer, controller, consumers
and successor-process path pass their joined qualification.

## Ownership and phases

Keep four authorities separate:

| Authority | Permitted effect |
| --- | --- |
| Prepared target | Reopen and authenticate the original immutable target intent |
| Reservation census | Inspect and close admission for the exact source cohort |
| Source seal | Retain actual kernel exclusion after verified source quiescence |
| Installed assignment | Rebind destination consumers and reconcile its exact ACK |

None is constructible from wire counters, account labels, an operation string,
an SDK assertion, a saved PID, or a generic application `Drained` value. Each
native authority is non-cloneable and non-deserializable; diagnostics redact
private identities. Persisted intent can locate an operation but cannot recreate
live custody after a process restart.

The controller advances the original operation through reservation, admission
closure, drain, seal, target re-admission, slot installation, destination binding
and acknowledgement. A retry resumes its original phase and immutable body.
It never fabricates a fresh operation to escape an unresolved earlier phase.

## Reservation census

Use a phase-aware reservation authority rather than retaining a stale Ready
proof after explicit member retirement. Bind it to the actual App lifetime,
registry incarnation, kernel caller PID/birth/image, native session instance,
original connection, full immutable source authority and prepared target.

Closing admission prevents new unrelated members and uses from joining that
source. Work already accepted retains its counted scope: tool continuations,
approvals, compaction and child work must still complete under their original
source. Closing the reservation must not cancel those scopes by pausing the
whole App lifetime.

Before publishing a Waiting response, Begin must already have a counted native
Pending intent. Accepted opens and preloads also remain accounted for while
their result or acknowledgement is unknown. EOF alone proves no retirement.

A current census must establish all of the following:

- Every accepted open is resolved, explicitly retired, or proven disjoint.
- Every Pending or Active use has settled under its original body and the
  correlated native End or Close acknowledgement.
- Each manager has its actual current loaded credential snapshot and four
  ownership counters; public readiness and greater counters are insufficient.
- Each direct reader has its actual last-use retirement proof.
- No retained unknown worker, preload, refresh response, or copied credential
  resource can still use the retiring source outside the accounted cohort.

Validate the settled loaded Slot/SameGrant snapshot; do not eagerly materialize
the owner's latest credential revision after source reads have been sealed.
A same-grant refresh does not change membership, and exact explicit retirement
can satisfy a member's obligation without reopening its credential source.

Snapshot state under the registry mutex, release it before filesystem, process
or kernel-lock probes, then recheck the exact reservation, source, member,
registration, connection and capability identities before accepting the result.
New work or changed identities invalidate the census; an earlier empty count
does not authorize a later seal.

## Source seal

The seal engine consumes current reservation/census authority and retains the
actual source namespace and actor-family kernel exclusion. It must not borrow
an old Ready proof whose member was retired to achieve that same census.
The phase-specific constructor remains private to the registry/controller;
there is no generic lock-to-seal conversion.

An unavailable or replaced namespace, changed caller, outstanding custody or
failed physical identity recheck refuses sealing. Partial acquisitions are
released before waiting. Retry uses the original cancellation scope and
absolute deadline, with no account-book, owner or authentication lock held.

The seal is held through installation and acknowledgement. Failure before an
installation has effects may release it without publishing a destination.
First settle or reopen reservation admission through the exact typed terminal
or reconciliation transition. Dropping a guard or cell handle alone must not
release a retained reservation.
Once installation may have had effects, retain reconciliation custody and
inspect the original operation; cancellation does not establish rollback.

## Target re-admission and destination members

After drain, reopen the actual prepared assignment book and authenticated owner
to validate the exact original target, source authority, pool revision, policy,
realm, grant binding and credential floor. A cached target record cannot replace
this admission. Release those account claims before any asynchronous wait.

Destination preload and Ready require a new native member and SDK Lifetime
under the admitted prepared-target continuation. They may borrow only the
actual held global/family guards through a typed destination role. An A use
capability, its operation string, or an old member cannot authorize B.

Publish the private slot using the admitted target and held seal. Rebind every
registered manager and resource owner, including tools, approvals, compaction,
HTTP bodies, WebSockets, realtime, catalog and Apps resources. Cached factories
carry no activity and cannot revive the retired source. Require actual binding
and resource readiness before accepting the destination ACK and reopening
admission. Preserve the historical ACK separately from a newer same-grant
credential revision.

## Owed work and restart

Try an eligible earned weekly reset before changing subscription. An automatic
switch during an owed turn must use the existing typed turn-suspension path:
stop and await its root task, keep independent native cleanup actors alive,
await actual use retirement, then persist and transfer the frozen accepted
input. Do not wait for a turn to drain while retaining that turn's own root use.
Do not manufacture a healthy quota observation or a manual selection intent.

A new process needs signed native succession bound to its actual PID/birth and
admitted image, plus destination assignment custody. Saved environment, an old
ACK and the predecessor's boundary do not authorize the successor. Unknown
predecessor custody remains unresolved until native orphan cleanup proves it
settled. Explicit human holds remain effective throughout recovery.

## Joined qualification

Exercise the actual control parser, accepted-job queue, registry, physical gate,
owner, canonical writer, private slot and SDK resource binding together. Include
queued Begin before pause, lost Close/retirement ACK, delayed old worker,
same-source Ready loss, child progress with a withheld parent reply, replaced
gate identity, target revision changes, cancellation before and after effects,
lost assignment ACK and successor PID transfer.

Synthetic provider fixtures can prove these local transitions but cannot prove
independent provider refresh-token lineage or two real subscriptions. Final
qualification additionally requires two real subscription accounts, correctly
routed independent email challenges, both durable publications, an automatic
quota-triggered switch, and continuation of the original owed work without
credential mixing or an unsolicited human prompt.
