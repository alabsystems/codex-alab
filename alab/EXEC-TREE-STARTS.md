<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Start barrier for an exec tree handoff

## Current status, 2026-11-04

The barrier is integrated with whole-tree suspension, fresh child restoration
and coordinated native admission. The combined source `9ab6890813` passes the
full gate and the thirteen-scenario native matrix, including active and idle
ephemeral children. [The current continuity record](EXEC-CHILD-CONTINUITY.md#current-native-verification-2026-11-04)
contains the exact evidence and remaining qualification limits. The sections
below record earlier implementation stages; their then-open descendant defects
are superseded by that result.

## Original rationale and implementation history

A snapshot cannot enumerate active children while new starts or reloads can
still register them. App-server admission alone does not fence Core's internal
child operations, and tree lifetime membership lasts until sessions shut down.
Waiting for that membership before capturing the sessions would stop them first.

The shared local runtime now has a separate barrier for short startup and input
operations. It covers child creation through initial input, thread construction
and reload, regular turn input and recovery, and automatic pending-mail wakes.
Admitted operations hold a read lease until their routing or startup work ends.
Native suspension seals this barrier before reading the expected original turn
and capturing its settings. Sealing waits for earlier leases and refuses later
ones without consuming their input. Nested startups refuse immediately while
sealing, so they cannot wait behind a writer waiting for their outer lease.

The barrier belongs to the local agent tree. Independent roots and replacement
runtimes retain their own open barriers. Cancelling a pending seal or rejecting
suspension validation reopens the original barrier. Once suspension commits to
stopping the source, the barrier stays closed even if cleanup fails.

This is a prerequisite for active-child capture, not its implementation. The
current suspension still aborts active child turns. Restoring their original
turn IDs, configuration, input, state and required capabilities under native
admission remains necessary. Direct non-regular task entrypoints also require
review when that capture is implemented; these changes do not claim a complete
runtime snapshot.

## Qualification, 2026-11-04

The full `alab/check.sh` gate passed on
`2525cc0911846cd24308b31544d2c38df1d205b2`: all 25 ALab test groups passed
(1,542 tests, four ignored), owned lint passed (118 upstream diagnostics set
aside), and all required hooked-library suites passed. These include Core
(2,971 tests, one ignored), exec (70), login (237), and TUI (6,185, five ignored).
Eleven focused checks also passed: five barrier/input checks, three suspension
checks and three original-turn recovery checks. The full log is
`exec-tree-barrier-full-gate.log` under `.aterm/.ubd`.

The private package `0.1.0-dev.14705+codex.0.166.0.g2525cc091` passed its build,
layout, signature and bundled-tool checks without promotion. With native host
`975a0a0654421f9afb387b1c4019a04155eaa8bc`, real-process probes passed for:

- accepted mail and nonzero token totals with lost acknowledgement and changed model settings;
- human output with the same lost acknowledgement and configuration change;
- a completed child's follow-up using the selected account;
- eight concurrent processes, two shared-pool rotations, nine consumed receipts
  and four retired reservations that did not execute;
- owner cancellation after native consumption, with exit 143 and no resumed model request.

Results are retained under `.aterm/.ubd` in `codex-tree-barrier-{tokens,human,child,shared8,stop}-2525`.
The matching active-child probe still fails: its parent resumes successfully,
but the child's original turn emits `turn_aborted` with reason `interrupted`.
That result is retained as `codex-tree-barrier-active-2525` and remains an open
runtime defect. These fixtures do not establish live provider email enrollment
or fleet placement.

## Child checkpoint plumbing (in progress)

The child-checkpoint work now extends the private accepted-input codec with
ordered queue groups sharing one publication table. The existing root path
still uses exactly one group; a single-group reader refuses a multi-group
snapshot rather than discarding child input. This prepares the data format for
child checkpoints and does not yet activate child resumption.
The grouped-input work passes six codec tests, four root-input tests, and the
embedded-client refusal/reuse test. It has not yet passed a full gate or been
integrated with active-child capture and restoration.

Per-thread input batches bind the shared groups to distinct thread/turn frames.
They retain each frame's queue boundaries, delivery phase, start metadata, and
accounting. Preparation validates the whole batch before returning frames;
wrong-root, wrong-turn, duplicate-thread and incomplete-batch inputs are refused.
Attaching a frame consumes only that frame, and an invalid attachment leaves the
remaining frames available. Preparation still grants no execution authority.
These APIs are checkpoint plumbing; active-child capture and activation remain
unimplemented.
The initial batch implementation passes its three identity/ownership tests and
all four existing root-input tests. It remains separate from the qualified
start-barrier build described above.

## Child task suspension (in progress)

`ExecPreparedChild` preflights a live regular child under its own drained tree
barrier. It checks portable accepted input and canonical history before any
stop. A committed barrier from that same local runtime is required to remove
the exact prepared task. A task that completed naturally is not resumed as an
unfinished turn; a replaced or cancelled task is refused.

The stop cancels and joins that task without the ordinary aborted-turn
lifecycle. It retains local input and accounting without sealing or copying the
shared controller mailbox, which remains the root's single responsibility.
It neither requests sibling shutdown nor closes a writer early. The caller
must still close all producers and writers before capturing the tree, and
replacement turns still require live native admission.

This primitive is not yet called by the native suspension path. Tree-wide
configuration/history capture, child bootstrap and per-child admission remain
required before the active-child process regression can pass. The qualified
package above does not contain this development work.

Focused verification passes: four child-suspension tests (including canonical
turn mismatch and missing history refusals), seven existing input/batch tests,
and three shared-barrier tests. The successful child-stop test also keeps a
sibling task running in the same local runtime and confirms that the root can
still acquire controller-mail custody. Logs are
`exec-child-suspension-{checked,input-regression,gate-regression}.log` under
`.aterm/.ubd`. No full gate or new package qualification is claimed.

Child suspension now owns its stop operation independently of a caller's
future. Clones of one prepared child share one operation and retain its result;
a retry after a lost reply returns the same original-turn settings instead of
trying to stop the already-removed task again. Invalid or uncommitted barriers
are still rejected before starting the operation. If the operation itself
exits without a result, the caller gets a failure rather than success.

The cancellation test holds the checkpoint lock, waits until task cancellation
has started, drops the requesting future, then releases the lock and observes
completion without driving another suspension request. A subsequent request
and its clone recover the same original-turn result. This closes a local
partial-stop failure; it does not connect child restoration to native rotation.
The final source passes all five child-suspension tests, recorded in
`.aterm/.ubd/exec-child-cancel-safe-final.log`. This change has not been through
the full gate or included in a qualified runtime package.

## Child configuration checkpoints (in progress)

`ExecConfiguration::capture_child` carries the original invocation's exact CLI
inputs into a child's resolved configuration. It retains the child's role
layers, model and custom provider, instructions and their provenance, cwd,
workspace roots, permission profile, service tier, reasoning settings, context
limits, feature choices and configured token-budget preferences. Explicit clears
remain clears; a parent instruction, tier or context override must not reappear.
The source configuration is not modified. Prepared children expose this capture
before the tree is committed to stop.

The private configuration envelope is version 2. Root capture still records its
original layers and overrides. Child capture adds runtime preferences, applies
them as inputs to normal Core configuration loading and keeps current managed
requirements and application network/auth routing. Built-in provider definitions
remain owned by the loader. A retained profile's display identity is restored
only if the validated profile still matches; it cannot replace the validated
permissions. Captured instruction text avoids re-reading old prompt files.

This is a configuration component, not a complete child-runtime checkpoint.
Native tree capture, child startup and per-child live admission are still not
connected. Dynamic capabilities, loaded idle children, other live session state
and the active-child process regression remain open. The previously qualified
package does not contain this work.

Capture now exercises the complete JSON encode/decode path before returning.
All seven configuration tests and five child-suspension tests pass on this
source (`exec-child-configuration-preflight.log` and
`exec-child-configuration-suspension.log` under `.aterm/.ubd`). Coverage includes
custom and built-in providers, explicit clears without old file reads, inherited
executable overrides without inherited writable roots, current managed feature
and approval requirements, preserved network ownership, and refusal of a custom
provider with nonportable runtime-only fields. Core represents a cleared service
tier as `default`, which prevents the parent tier from being inherited again.
No full gate or private package qualification has been run for this component.

## Idle input frames (in progress)

Input batches now distinguish active frames from idle mailboxes (private batch
version 2). An idle frame carries only its thread identity, ordered accepted
mail and each message's start options. It has no invented unfinished turn ID or
turn accounting. All active and idle groups still use one publication table.

`validate_exec_idle_inputs` checks a loaded idle child under its drained tree
barrier before commitment. `seal_exec_idle_inputs` requires commitment and
rechecks that no active, reserved or suspended turn is present. Snapshot capture
then runs after all producers stop. Neither validation nor capture consumes the
source's accepted mail or takes the root's shared controller custody.

Batch preparation validates both sets together and refuses duplicate identities,
malformed queue boundaries, non-mail idle items or a batch with no active root.
Taking idle data is separate from attaching active-turn data; wrong-kind and
repeat requests cannot consume another frame. `PreparedExecIdleInput` holds
configuration-ready mail data only. Native activation with fresh admission is
still required, and is not implemented by this type.

The final source passes all 12 input tests plus all five child-suspension tests
(`exec-idle-input-final.log` and `exec-idle-input-child-regression.log` under
`.aterm/.ubd`). Five new checks cover idle source sealing/retention, refusal of
reserved or previously suspended turns, shared active/idle publication state,
invalid idle frames and empty idle children. No full gate or new package has
been qualified, and native tree activation is still not connected.

## Tree-wide source capture (in progress)

`prepare_exec_tree` inventories the exact local runtime by allocation identity,
including private loaded children. It rejects a replaced root, a second root,
missing/cyclic parent relationships, unsupported active tasks and unavailable
idle history before stopping any task. Dropping a plan before it starts reopens
its start barrier. The native caller must also retain its app-server admission
pause; this Core operation is not itself a new account-selection protocol.

Once started, one owned capture operation commits the barrier, stops active
children without terminal events, seals idle input, suspends the original root,
and waits for the entire runtime's producer/writer shutdown. It then captures
one active/idle input batch and reads final configuration, settings, tool
specifications and canonical history evidence from the quiescent source. A child
that completes naturally is captured as idle. Repeated callers get the same
captured result; dropping a waiter does not abandon the operation.

`ExecCapturedTree` is deliberately a Core source record, not a portable handoff
or an execution grant. Its settings still need a checked transport and fresh
capability restoration. Host extension objects are not copied into those
settings. Persistent history is latest-turn evidence, with model context still
loaded through the store; active ephemeral capture additionally retains its
existing memory/context checkpoint. Idle ephemeral history remains refused
until its own context restoration is implemented. The native capture/restore
consumer is still not connected, and the real-process active-child regression
remains open.

Three real-runtime source tests pass: active and idle children are captured
without terminalizing active turns while a foreign root keeps running; a queued
plugin-setting update is reflected in the final capture; an unsupported child
refuses before either task stops; and dropping an unstarted plan restores start
admission. The first case also reconciles a dropped waiter. Twelve input tests
and five child-suspension tests pass unchanged. Logs are
`exec-tree-capture-final.log`, `exec-tree-capture-input-regression.log` and
`exec-tree-capture-child-regression.log` under `.aterm/.ubd`. No full gate, new
package or native active-child recovery qualification is claimed.

## Portable tree data (in progress)

`ExecTreeSnapshot` is a versioned, private transport form of the source capture.
It preserves ordered parent relationships, active-versus-idle state, original
turn settings, configurations, tool definitions, selected capability-root order,
one shared input batch, and lossless latest-turn history evidence. Store revisions
are process-local and are not reused across replacement. Active ephemeral history
must agree with the separate history evidence.

Validation binds the root and turn, checks every parent and thread identity,
checks history terminal state against recovery options, and requires the input
frames to match the node list exactly. Source preflight now checks settings and
history encodability; completed capture also runs the complete transport codec.

Environment requests carry data describing the old configuration, not a live
attachment. Restoring settings requires fresh owner bindings with matching
identities, workspace paths, configuration and capability selection order.
Only the fresh binding is copied into the returned request. Changed owner policy
or unavailable bindings require revalidation; the codec does not silently weaken
old restrictions. Host extension objects are never encoded, and new owner data
comes from the caller. The returned settings still require normal Core policy
validation and live native admission.

Native capsule use and the tree restoration/activation path remain unconnected.
This format alone neither selects an account nor starts a replacement turn.

Verification passes 48 `tree_`-filtered checks, including six settings/history
transport tests and the four source-tree tests (now including conflicting
active-ephemeral history). Twelve input tests and five child-suspension tests
also pass. The native transport round trip is exercised from the real source
capture test, including graph/input tampering checks and fresh settings binding.
Logs: `.aterm/.ubd/exec-tree-wire-final.log`, `exec-tree-wire-input-regression.log`
and `exec-tree-wire-child-regression.log`. No full gate or packaged native
rotation qualification has been run for these changes.
