<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Review account recovery

## Current qualification

Runtime source `4358eef0b8f32dc04519c0dfe08ff7508afea0d4` passes all seventeen
native scenarios in `.aterm/.ubd/codex-review-native-4358eef0b-v1/summary.json`.
The canonical private package is
`0.1.0-dev.14779+codex.0.166.0.g4358eef0b`, tested with native host
`4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`. Package version, compiled defaults,
strict signatures, system-library dependencies and layout checks pass. It was
built with `--no-promote`; the selected installation remains unchanged.

The final full gate passes at `f1b509008b8c123344497f188c5a0f2ec2cf1002`:
Core 3,059 passed/one ignored, app-server 469 passed, Exec 71 passed, TUI 6,186
passed/five ignored, every other required suite passing, and no ALab-owned Tippy
findings (125 upstream diagnostics set aside). The only changes between the
packaged runtime and this gate revision are `alab/unit_results.py` and its tests.
They repair attribution when a terminal redraw directly precedes a test result;
seventeen parser tests pass, including exact-artifact and duplicate-result guards.
The retained gate result contains no failed unit-test copies.

The native matrix covers durable and ephemeral review control/quota recovery,
ephemeral root/active-child/idle-child recovery, chained rotations, active-child
continuity, token accounting, lost consume acknowledgement, human output,
configuration/worktree continuity, exhausted pools, owner cancellation, forty
parallel logins and eight processes sharing one pool. Forty concurrent requests
use forty distinct logins without modifying their credential files. The shared
pool preserves eight distinct processes and turns across two rotations, consumes
nine receipts and retires four unexecuted reservations.

Both review quota cases retain the original process and parent/delegate turns,
keep the actual pre-quota command output, and finish after three provider
requests. Durable review publishes its structured result once. Ephemeral review
renders its result once without creating a rollout; separate in-memory tests
verify its complete typed result and original lifecycle.

Retained logs under `.aterm/.ubd` are `exec-review-final-f1b509-full-gate.log`,
`exec-review-package-4358ee.log` and `exec-review-native-matrix-4358ee.log`.
The unit-result provenance is retained in `exec-review-final-f1b509-unit-v1`.

These checks use isolated fixture accounts and providers. Live OpenAI email-code
enrollment remains unqualified after the recorded Cloudflare refusal, and live
fleet placement remains unqualified under the installation/promotion hold.
Mailbox access, authentication, account lists and machine identity continue to
belong to aterm's shared services; Codex supplies its recovery adapter.

## Shared authentication qualification

Aterm main `d598ac53d8647d11811aaeb824b4f57664698105` includes the shared
Proton transport owner. Both `aterm email` and the native authentication-code
reader use `mail_browser::shared_mailbox_until`; independent CLI processes
share one browser owner while retaining native Proton decryption. Codex uses
aterm's identity-bound account selection and renewal protocols and adds no
Proton connection or email-code client.

The aterm change passes `targo --unverified verify -p aterm-agent`, including
all library and integration tests, lint, source checks and the selected
cross-platform cells. A private build from
`b9c8202206a1cb672c396a2c722edbe3e3535a52` passes real concurrent Proton
sessions and continued folder reads by both open sessions after their owner
is killed and replaced. An earlier run also verified idle exit and socket
removal. The aterm measurement document is
`docs/measured/proton-cross-process-readers-2026-11-04.md`; the private evidence
is `proton-owner-final-{build-record,live-concurrent,live-retained}.json` under
`.aterm/.ubd`. These probes read no messages and send no mail.

The 2026-11-04 local inventory still names machine `m158`, thirteen shared
Codex rows, eleven assigned elsewhere, one not enrolled and one locally
usable. The local rows' authentication state is `unseen`; a usable roster row
does not prove a current provider login. The installed Codex is managed package
`2026110301`, not the qualified private package above. No installation or
promotion was performed. Live email-code enrollment, a second usable local
login and provider-backed fleet placement therefore remain unqualified;
fixture rotation and concurrent mailbox access do not establish them.

## Earlier native baseline

Source `d8170491b1f1b81652f9f0e20ef8755aa2a60dd1` passes the full repository
gate and all fifteen native scenarios in
`.aterm/.ubd/codex-review-native-d8170491b-v1/summary.json`. The canonical package
is `0.1.0-dev.14762+codex.0.166.0.gd8170491b`; the native host is
`4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`. No package was installed or promoted.

The real review quota case preserves the original process, parent review turn,
delegate thread and delegate turn. Three provider requests cross the quota
boundary, retain the actual command output exactly once, and emit one structured
review result. Its no-quota control also passes. The other thirteen scenarios
cover ephemeral root/active-child/idle-child recovery, repeated rotations,
accepted input, lost consume acknowledgement, token accounting, human output,
managed-worktree/configuration continuity, exhausted accounts, owner Stop,
forty isolated parallel logins, and eight sessions sharing two account rotations.
The latter consumed nine receipts and retired five unexecuted reservations.

The gate reports Core 3,050 passed/one ignored, Exec 71 passed, TUI 6,186
passed/five ignored, all remaining required suites passing, and no ALab-owned
Tippy findings (125 upstream diagnostics set aside). Retained logs are
`exec-review-inventory-full-gate.log`, `exec-review-package.log` and
`exec-review-native-matrix-d817.log` under `.aterm/.ubd`.

This earlier baseline preceded the completed-delegate, queue-drainage,
cancellation and ephemeral-review follow-ups now covered by the current
qualification above. The sections below retain their failure and
implementation-stage records.

## Completed-delegate follow-up

A deterministic test found that a delegate completing while its parent was
frozen made capture fail with `review snapshot changed its delegate identity or
turn`. The follow-up accepts an idle completed delegate only when canonical
history names its original turn and exactly one queued completion carries the
same result. Another turn, an abort, a repeated completion, or a restart after
completion is rejected. The parent review must remain unfinished.

Destination preparation uses normal idle admission for a captured completed
delegate. If an active restored delegate completes before parent attachment,
the parent accepts its verified original completion and consumes its existing
IO; it does not start another reviewer. A finishing task is allowed to publish
its terminal status before that check.

Twenty-two focused review tests, forty-two capture/recovery tests, and seven
tree recovery tests pass. The full-tree completed-source case produces the
original review result with no destination model request. Core/app-server
all-target Tippy reports no ALab-owned diagnostics (58 upstream set aside).
Logs are `exec-review-completed-source-before.log`,
`exec-review-completed-source-after.log`, `exec-review-completion-tests.log`,
`exec-review-completion-capture-regression.log`, and
`exec-review-completion-tree-regression.log` under `.aterm/.ubd`.

This follow-up is newer than the native-qualified package above. It still
requires final full-gate/package/native validation along with the queue and
cancellation follow-up below.

## Queue and cancellation follow-up

`8ed846a7e4` fixes a reproduced deadlock where freezing the parent stopped it
reading the bounded delegate queue, preventing the delegate from shutting down.
The frozen consumer retains events without publishing them and shares one drain
lock with snapshot collection, preserving event order.

`2f6b046178` distinguishes an acknowledged freeze from a committed handoff.
Preflight can inspect a frozen consumer; actual suspension requires its commit.
Ordinary cancellation finishes the consumer and cancels its bridge. After commit,
the tree owner stops the delegate while its independent bridge drains final
events. The bridge watcher holds no session or consumer alive. Tests exercise
ordinary cancellation, late-commit refusal, real forwarding after parent
cancellation, bounded queues, and watcher cleanup. Twenty review tests,
forty-two Core recovery tests, and seven app-server tree tests pass; Core/app-server
Tippy reports no owned diagnostics. These commits are newer than the packaged
native baseline and remain subject to the final combined qualification.

## Ephemeral review qualification

The native probe accepts both review scenarios with `--ephemeral`:

```
python3 alab/smoke/exec_accounts.py --host HOST --package PACKAGE --output FRESH_DIR --review control --ephemeral
python3 alab/smoke/exec_accounts.py --host HOST --package PACKAGE --output FRESH_DIR --review --ephemeral
```

Both pass against the `d8170491b` package and the native host above. Retained
results are `.aterm/.ubd/codex-d817-ephemeral-review-control-v1/result.json` and
`.aterm/.ubd/codex-d817-ephemeral-review-quota-v1/result.json`. The quota case
binds the original process and parent turn through the consumed native receipt,
preserves the delegate identity across all three provider requests, retains the
real command output once, renders one final review result, and creates no rollout.
The control uses two source-account requests and the same no-rollout checks.

Exec JSON exposes the rendered review result, not typed review lifecycle items.
The ephemeral probe records `renderedResult` and its evidence source rather than
claiming a persisted `structuredResult`. It rejects duplicate/missing/raw-JSON
answers, changed root identity, replayed/failed lifecycle events, missing native
turn binding, lost command output, and any session rollout. The durable probe
continues to require the original typed entry, exit, and completion in history.

The app-server ephemeral tests separately round-trip the captured memory tree,
restore both original identities, require independent child admission, compare
the complete typed review result, and count exactly one original parent entry,
exit and completion in memory. Neither source nor replacement has a rollout or
stored thread history. A refused child admission produces no destination model
request. These tests pass on the newer source; that source has not yet received
the final native-package qualification.

## Reproduced gap, 2026-11-04

The regular-turn native matrix is green, but `codex exec review` does not
currently rotate when its reviewer hits a weekly quota. The new review probe
uses the real review entry point, a real command tool, and a structured review
result. It supports two cases with the same package, native host and isolated
fixture accounts:

```
python3 alab/smoke/exec_accounts.py --host HOST --package PACKAGE --output FRESH_DIR --review control
python3 alab/smoke/exec_accounts.py --host HOST --package PACKAGE --output FRESH_DIR --review
```

The control performs review work and completes on the source account without a
quota error. The quota case returns 429 only after the review's actual command
output is present in the next model request. It then requires native recovery,
the selected account, the original delegate thread and turn, the original parent
turn, and exactly one structured review result. A new review after abandoning
the old one is a failure.

With package `0.1.0-dev.14752+codex.0.166.0.g9ab689081`, source
`9ab68908135c5e5306ee1fd612a0da5c78426458`, and native host
`4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`:

- The no-quota control passes. Its two provider requests preserve delegate
  identity; the parent publishes the expected structured result exactly once.
- The quota case fails its 200-second completion deadline. The provider records
  one successful response and one 429, and the native host creates no account
  handoff receipt. The CLI logs the controller as disabled; the delegate's quota
  observation reports `turn_exec=false` and `thread_exec=false`.
- An ordinary native account handoff still passes with the extended fixture.
  Eight review-oracle tests, fifteen existing child-oracle tests and eighteen
  shared fixture tests pass.

Retained results under `.aterm/.ubd` are
`codex-native-review-control-9ab-v4`, `codex-native-review-quota-9ab-v1`, and
`codex-native-review-harness-regression-9ab-v1`. The failed control drafts v1/v2
exposed an oracle assumption, not a review failure: current history records
`ItemCompleted(ExitedReviewMode)`, and the apparent parent `TaskStarted` is a
forwarded delegate event. The corrected control checks the parent review entry,
exit and completion IDs separately from the delegate's turn.

## State that must survive

Destination tree preparation now binds each saved review consumer to the exact
fresh parent and delegate after loading all members. The original-turn request
carries those bindings through normal native admission. The launch barrier
checks each member against its captured task kind and releases review execution
only after every original turn is registered. A real `Op::Review` test passes:
the original delegate resumes after its delayed admission, only one destination
model request occurs, and the original parent emits the structured result.
The matching refusal test makes no destination model request and stops every
member. All six tree recovery tests pass, including the existing regular-tree,
deadline and embedded-bootstrap cases, in
`.aterm/.ubd/exec-review-destination-regression.log`.

The scoped Core run passed 300 checks and found one old hand-written fixture
missing the required task-kind field. With that fixture explicitly marked
regular, its focused rerun passes in
`.aterm/.ubd/exec-review-destination-accounting.log`; the matching exec checkpoint
fixture also passes. Core and app-server all-target Tippy report no ALab-owned,
spanning, failed-target or build-script diagnostics, with 58 upstream diagnostics
set aside. The full gate has not run for this component.

These tests use explicit admission callbacks, not the native account host.
The CLI review path is now enabled in the candidate, pending the full gate
and a rebuilt package passing the real quota probe.

Source capture now freezes every review consumer before stopping tree members,
retains the original delegate and pending output, and records the review task
kind in recovery options. The private tree snapshot is version 2 and rejects a
review without its consumer, delegate, matching parent, or original delegate
turn. Suspension accepts a review only while its consumer has acknowledged the
freeze. Ordinary review cancellation remains unchanged.

The real-session capture test retains both original turns and queued review
work without a completion or abort. It and all 40 recovery-options/tree-capture
tests pass in `.aterm/.ubd/exec-review-capture-regression.log`. Destination tree
startup now has the component coverage above; quota observation and native
qualification remain unfinished.
Core's all-target Tippy check reports no ALab-owned, spanning, failed-target or
build-script diagnostics for this capture component, with 47 upstream diagnostics
set aside. The full gate and native quota probe remain outstanding.

The history component now recognizes the review parent's typed entry and exit
items. A forwarded delegate start no longer replaces the parent as the open
review turn. Exit, completion and parent abort revoke that open state. The
paginated history reader retains the parent entry through a repeated parent
start while excluding an older review from a later regular turn. Seven store
tests and eleven memory-history tests pass; results are retained in
`.aterm/.ubd/exec-review-history-store-final.log` and
`.aterm/.ubd/exec-review-history-memory.log`. This is history validation only;
the enabled native path still requires its end-to-end quota qualification.
The all-target Tippy check for Core and the thread store reports no ALab-owned,
spanning, failed-target or build-script diagnostics, with 48 upstream diagnostics
set aside by the ownership filter.

In the qualified `9ab6890813` baseline, simply enabling the controller would not restore a review:

1. `exec/src/lib.rs` excludes `InitialOperation::Review` from the controller.
2. `ReviewTask` creates its delegate through `run_codex_thread_one_shot`, whose
   direct `Session::spawn` path does not put a `CodexThread` into the manager's
   loaded tree. Existing extension hooks already supply `ExecQuotaMember` and
   publish quota observations; the debug fields `turn_exec`/`thread_exec` report
   the separate `HeadlessExec` marker, not the absence of all quota provenance.
3. The parent task consumes an event channel and retains the last assistant
   message until the delegate completes. That consumer state is not part of the
   regular-turn checkpoint. Cancellation currently means an interrupted review.
4. The parent's review entry and exit carry its own turn ID. The forwarded
   delegate start carries a different ID. The regular history reducer's last
   open turn is therefore not sufficient evidence of a parent review turn.
5. `ExecRecoveryOptions::capture` and child suspension intentionally admit only
   regular tasks, and regular recovery starts a regular sampler.

The next implementation needs typed review continuation state rather than
relabeling the review as a regular task. Delegate registration and quota
observation must use the same tree owner and startup fence. Source capture must
retain the exact parent/delegate relationship and the review consumer's pending
message. Destination startup must restore the delegate under the selected auth
manager, reattach the parent consumer, and obtain native admission for every
resumed turn before releasing the launch barrier. A source cancellation must
not publish an interrupted review as if the person stopped it.

The successful control is the minimum positive oracle for that work. The quota
case must become green without weakening its identity, retained-work, structured
result or native-receipt checks. This work does not change aterm's ownership of
email, sign-in, shared accounts or machine identity.

## Managed delegate registration

The pending destination implementation now passes nineteen focused review
tests, including restoration of the buffered message and queued
events exactly once, rejection of a changed parent or turn, and rejection of
an already-attached receiver. The Trust unverified test run is recorded in
`.aterm/.ubd/exec-review-cold-tests.log`. The child-start integration test also
checks private registration, review session identity, disabled multi-agent mode,
shared authentication and execution policy, rejection of a missing parent
binding, and removal after shutdown. The existing seven delegate tests pass
in `.aterm/.ubd/exec-review-restore-delegate-tests.log`. These checks cover the
components. Complete capture and destination preparation now have the coverage
above; native quota tests must still qualify the enabled controller.

Core's all-target Tippy check reports no ALab-owned, spanning, failed-target or
build-script diagnostics; 47 upstream diagnostics are set aside by the normal
ownership filter. The full gate and native review quota test have not run for
this destination component.

Exec-owned review delegates now register their existing `Session` with the
parent's manager under a private `InternalSessionSource::Review` registration.
Their actual session and saved source remain `SubAgentSource::Review`; the
original authentication manager, execution policy and event consumer are
retained. The normal delegate forwarder already suppresses `SessionConfigured`,
so consuming that event during manager registration does not remove a public
review event or add a competing event reader.

Registration holds the shared exec startup fence. The manager checks the live
parent atomically with insertion, refuses public access/removal and live resume
of the delegate, and removes only that exact registration after its session
loop ends. Ordinary non-exec reviews retain their existing path.

Five focused registration tests pass, covering durable and ephemeral delegates,
private access, original session identity, startup rejection after sealing,
cancellation and cleanup. All seven existing delegate tests and 322 protocol
tests also pass. Core all-target Tippy reports zero ALab diagnostics, with 47
upstream diagnostics set aside. The generated public v2 schema is unchanged.

These registration checks cover tree inventory and ownership. The controller
now includes review turns in the candidate. Complete capture and original-review
recovery under native admission must pass the quota probe before qualification.

## Quiescent parent event consumer

An exec review now retains its parent event-consumer state explicitly. It binds
one private delegate to the original parent context, buffers the last assistant
message, and preserves the existing suppression of raw final assistant output.
The unmarked review path continues to use the original consumer.

Freezing waits for any current forward to finish before acknowledging the
snapshot boundary. Dropping an unfinished freeze releases the consumer; an
acknowledged, committed freeze survives source-task cancellation without
reporting an ordinary completed review. The public freeze operation rechecks
the exact active parent context after the acknowledgement.

`ExecReviewSnapshot` binds parent and delegate thread/turn identities and keeps
both the buffered message and pending events. Snapshot collection requires the
same stopped delegate, drains its forwarding bridge, and retains each dequeued
event before awaiting again. The bridge now closes on `ShutdownComplete` even
when suspension did not emit a turn-completion event. That transport shutdown
is omitted from portable pending data; a real delegate abort is rejected.

These are source-side snapshot primitives. Destination rebinding and task
selection now have the component coverage described above. Connecting the
snapshot to complete source capture and native tree admission remains necessary
before enabling review quota recovery.

Fifteen focused consumer/registration tests pass, including concurrent snapshot
readers, cancellation during collection, completed-review refusal, exact
identity checks, output suppression and rejection of a second delegate binding.
Seven existing delegate tests pass. Core all-target Tippy reports no ALab
diagnostics (47 upstream diagnostics set aside). The receiver-drain method has
a scoped lint expectation explaining why its asynchronous lock must span the
read loop; the concurrent-reader test verifies that serialization boundary.
