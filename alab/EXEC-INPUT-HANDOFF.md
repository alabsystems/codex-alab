<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Accepted input across account replacement

The pending-input branch begins with a private snapshot format. It preserves
ordered internal input variants, client acceptance order and UUID, private
response-envelope metadata, tool-result success, and shared message-delivery
identity. A restored duplicate delivery shares its consumption state while the
original process's delivery remains independent. Snapshot data grants no
execution authority.

The ordinary TurnInput serializer rejects annotated response envelopes and the
ordinary PendingAgentMessage serializer omits publication state. Using either
would discard accepted state. The snapshot therefore reuses the lossless private
history item representation and records a publication table explicitly.

A pending publication with lifecycle observers is refused: those observers can
hold arbitrary extension data and require a restoration contract. Refusal must
happen before suspension can relinquish the original runtime. It must never be
converted to historical mail or a newly published message. Consumed publications
need no observer callback and preserve their consumed state.

Four codec tests pass: annotated input and order, shared consumption without
changing the source, invalid reference rejection, and version rejection. The
separate observer test passes and verifies that refusal neither consumes nor
republishes the delivery. These tests do not prove runtime preservation.

The runtime integration retains the removed active turn's queue before
`clear_pending` would destroy it. It captures the root session mailbox and its
start options after producer shutdown, preserves notification and delivery
phase, and binds the data to the original thread and turn. The private memory
capsule and bootstrap require this snapshot. Bootstrap decodes it without
enqueueing; native admission must succeed before installation and sampling.
A one-use startup marker prevents ordinary new-task mailbox draining from
bypassing the restored delivery phase.

The snapshot-state test passes for mailbox order, phase, and start metadata.
Three recovery tests pass for persisted, compacted, and ephemeral roots: two
accepted messages are first shown absent from recorded history, then preserved
once and in order after recovery. Denied and replayed admission checks remain.
The client admission test and all 70 exec library tests also pass. A direct
startup test verifies that deferred mail remains deferred until its original
delivery phase permits consumption.

The real-process pending-mail fixture now handles Responses Lite tool definitions
carried in `additional_tools` input items as well as ordinary tool lists. Against
the pre-fix private package `cc6e5e0637`, the real child successfully submitted
one message during the native preparation pause; the replacement request had
zero copies of that message. Its acceptance and loss artifacts are in
`$HOME/.aterm/.ubd/codex-pending-mail-negative-v6`. This is a reproduced
failure in the baseline. The private package built from `71d149396b` passes the
same real-process probe: one accepted child message reaches the recovered root
request exactly once, with the original process and turn. A second run passes
with human-readable output, a dropped native-consumption acknowledgment, and a
model/configuration change during the quota wait; the original model and
developer instructions remain in effect and the final answer appears once.
Results are under `$HOME/.aterm/.ubd/codex-pending-mail-positive-71d149`
and `$HOME/.aterm/.ubd/codex-pending-mail-human-71d149`.

The full gate passed on `4329f1a5d1`: CLI build/run, all 1,542 ALab tests,
owned-code lint, and every required hooked upstream unit suite, including 2,958
Core tests and 6,185 TUI tests. Earlier attempts exposed warnings in older
ALab-owned code under the current Trust compiler; separate toolchain commits
resolved them. The successful log is
`$HOME/.aterm/.ubd/exec-input-full-gate-final.log`.

The same `71d149396b` private package also passed eight concurrent sessions,
with two shared-pool rotations, nine consumed receipts, and five retired
unexecuted reservations while one preparation was delayed. The result is
`$HOME/.aterm/.ubd/codex-input-shared-eight-71d149/result.json`.
These fixtures do not qualify live provider email authentication, descendant
restoration or every runtime state. No package was installed or promoted.

Unresolved approvals and other interactive waiters, extension publication
state, unloaded descendants' controller mailboxes, and remaining session state
need their own explicit treatment. This root-queue change does not establish
complete runtime preservation or enable automatic ephemeral/review rotation.
Further probes must cover accepted steering, deferred mailbox delivery,
repeated replacement, and failure after the original runtime has stopped.
