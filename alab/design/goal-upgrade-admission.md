<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native goal admission during an update

An active goal crosses through its actual runtime checkpoint, joined with the
Core forest and terminal/input owners at one final synchronous source decision.
The updater inventories every loaded root, including hidden roots, before asking
native source-exec preparation to become ready. Active status itself no longer
blocks preparation. An unreadable goal, missing native runtime, queued mutation,
or invalidated checkpoint still refuses it.

This is a development path, not a qualified live-update claim. Native execution
discovery remains unavailable until the complete signed transition and recovery
are qualified. Other restrictions, including idle turns and an empty composer,
remain and do not satisfy the goal of immediate seamless updates.

The checkpoint obtains the existing goal mutation semaphore, requires an idle
enabled runtime with no recovery or persisted continuation deferral, and reads
the entire saved goal after flushing idle progress through native accounting.
It never replaces a saved goal from a checkpoint snapshot. Objective, identity,
budget and accumulated accounting remain owned by the native goal service.

Holding the semaphore through asynchronous preparation would strand a queued
continuation. Preparation instead returns an owned, revocable transfer. Every
native goal mutation registers before waiting for the semaphore. A registered
writer prevents preparation; a later writer revokes it before mutation. A
configuration change also revokes it. Core or goal revocation cancels the
updater's native readiness wait; abandoning that wait cancels its unused native
reservation before a detached worker can later seal input.

The final decision checks runtime ownership/readiness and serializes the UI
and native host decision with writer admission. Refusal leaves rollback available.
Dropping an uncommitted transfer reopens its own generation only. Commitment
seals the old runtimes, waits for Core producers and actual SDK reader retirement,
then flushes final goal accounting and retains the full frame on the native host.
A failed final capture is fatal cleanup, not permission to restart old producers.

The target protocol is 3, with exec custody 2 and full-runtime format 1. Capability
data supplies no package or constructor authority. The exact target image must
have native signed-build admission; native source-exec Go binds its original
operation, fresh constructor command, primary and retained frame digest/size.
The destination loads through that constructor's runtime host, restores its
Core/goal forest behind admission, then activates after primary UI attachment
and event replay. Settlement retains its original connection until native
cleanup finishes. No decoded checkpoint grants model work by itself.

Validation covers hidden loaded goals, failed native row reads, queued mutations,
revocation before writes, final accounting, cancellation, concurrent commit/writer
admission, and destination accounting before native continuation. These component
fixtures do not substitute for a signed end-to-end live handoff and recovery run.
