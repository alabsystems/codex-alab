<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native goal admission during an update

An active goal can cross a process boundary only after its native runtime has
checkpointed the persisted goal and committed a handoff with the final native UI
decision. This document describes the admission primitive; the updater does not
yet use it to relax its active-goal hold.

The checkpoint obtains the existing goal mutation semaphore, requires an idle
enabled runtime with no recovery or persisted continuation deferral, and reads
the entire active goal row after flushing idle progress through native accounting.
It never replaces a saved goal from a checkpoint snapshot. Objective, identity,
budget and accumulated accounting remain owned by the native goal service.

Holding the semaphore through asynchronous UI teardown would strand a queued
continuation. Instead, preparation returns an owned, revocable transfer. Every
native goal mutation registers before waiting for the semaphore. A registered
writer prevents preparation; a later writer revokes preparation before it can
mutate. Cancellation cannot make that revoked attempt valid again. Configuration
changes also revoke preparation.

The final synchronous decision checks that the token belongs to this runtime,
rechecks runtime readiness, and serializes the UI decision with writer admission.
A refusal leaves rollback available. Dropping an uncommitted transfer reopens its
own generation only. A successful decision seals the old runtime: new mutations
refuse, and idle continuations skip instead of waiting through teardown. This
primitive supplies no execution authority and does not authenticate an update.

The updater must still retain and validate the exact target executable, preserve
the native session and all carried work, exclude active hidden roots, and commit
the real UI frame. Removing the blanket active-goal guard before those pieces
exist would violate the handoff contract.

The target-side `debug upgrade-capabilities [--hold]` entry emits one bounded
JSON frame identifying protocol 1, its process, canonical executable, file
identity and compiled product version. With `--hold`, it flushes the frame before
waiting for EOF on standard input. Unexpected input refuses. This handshake
does not authenticate a package, authorize a restart, or advertise active-goal
restore support. The caller must authenticate the exact build through atpkg's
signed named-build lease, compare the returned identity with its retained target,
and own the helper until rollback or exec closes its pipe. The caller and final
exec integration remain to be implemented.

Validation covers cancelled and queued writers, stale rollback, exhausted
generations, conditional refusal and concurrent writer/commit linearization.
The native runtime integration test exercises persisted accounting, continuation
deferral, a real pause mutation, foreign-token refusal, committed admission and
idle shutdown. Existing hidden-root and failed-goal-read regressions remain in
place.
