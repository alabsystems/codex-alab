<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Headless account recovery

## Current qualification, 2026-11-05

The [current native and fleet record](AUTH-FLEET-2026-11-05.md) records source
`8b00fe6315` passing all 17 native recovery scenarios, including review and
ephemeral review, 40 isolated concurrent logins and eight sessions sharing quota
rotations. Two live fleet machines agree on their shared account catalog. Live
enrollment of the additional account remains blocked by Cloudflare, so live
provider rotation and the full parallel-login goal are not declared complete.

## Previous qualification, 2026-11-04

The combined source `9ab68908135c5e5306ee1fd612a0da5c78426458` passes the full
gate and thirteen native scenarios, including forty isolated concurrent logins,
eight sessions sharing account rotations, and active/idle ephemeral children.
See [the current child-continuity record](EXEC-CHILD-CONTINUITY.md#current-native-verification-2026-11-04)
for the exact package, host, results and limitations. No package was installed.

The source now captures the stopped tree, accepted input and renderer in one
anonymous memory capsule, binds its digest and ordered membership to the native
operation, and reconstructs fresh runtimes under the replacement root. Each
member still needs native admission; the launch barrier releases model work
only after the admitted original turns have registered. Account lists, machine
placement, email and sign-in belong to aterm's shared services. No Codex mailbox
reader or provider-specific account selector was added.

Live OpenAI email enrollment and fleet placement remain unqualified. The
successful live mailbox API read is not proof of an end-to-end sign-in; the
native enrollment attempt stopped at Cloudflare. Non-regular task modes and
external resources beyond the captured/restored bindings need further work.
The [review recovery probe](EXEC-REVIEW-RECOVERY.md) reproduces the disabled
review path with a passing no-quota control and a failing quota case.

## Earlier qualification and implementation history

Status: original-turn recovery, repeated moves, lost-consume-ACK recovery,
JSON item continuity, human final-output continuity and owner cancellation pass
real-process tests. Four and eight concurrent sessions also pass against one
shared account pool, rotating from the same exhausted source to the same target.
No installed runtime
has been updated, and the full parallel-login goal is not complete.

Managed-worktree recovery is qualified with Codex `7a6cae40a6` and native
`e449bca767`: JSON and human-output probes pass two account moves, a lost
consume acknowledgement and changed model configuration while retaining the
same managed checkout, its original thread owner, dirty tracked/untracked
files, process, turn and completed tool output. Recovery loads the native
checkpoint before checkout allocation and validates the existing checkout
through the shared worktree manager. The two bootstrap unit tests pass; the
new feature still requires its full gate.

Contract 3 moves shared-pool reservation ahead of suspension. Native
`7e4b30be9b` and Codex package `16f3fdb8cb` pass the four- and eight-session
shared-pool tests and the four-session slow-canary regression
(`exec_shared_pool.py --canary-delay 130`). In the latter, three waiting
reservations expire without executing; all four original processes and turns
then complete, with one shared rotation and the target proven. Repeated moves,
lost consume acknowledgement, changed model config, JSON and human output,
exhausted-pool cancellation and post-consume owner Stop also pass. Client,
native quota/cancellation, transaction and exec completion unit tests pass,
as does strict native lint. Full gates still need to run on the settled change.

The first probe stalled because native exec presented the config home as a
second login override. Aterm `45e8083889` attributes only the explicit auth path,
as its TUI adapter does. With Codex `e530ab09a5`, the initial move and a dropped
consume acknowledgement both preserve PID, original turn, completed tool output
and successful exit. The repeated-move case exposed a history reader stopping at
the latest recovery start before reaching the original input; this branch retains
the whole same-turn start sequence without discarding terminal or compaction
evidence. Codex `842d4699c0` passes two moves through three accounts without
changing PID or the original turn. The stronger smoke now also requires item IDs
to retain one type, one completion and closure of started items. It exposed a
reset item counter and lost todo list. The new checkpoint restores all renderer
data before new events and moves replacement onto the output owner's task.
The private package built from `9c19645050`, with native host `45e8083889`,
passes the strict JSON case with two moves and a lost consume acknowledgement:
one PID and original turn, four model requests, completed tool output retained
exactly once, stable item types and IDs, and every started item completed.
The corresponding human-output case passes with one final answer. Separate
all-accounts-exhausted and post-consume owner-Stop cases pass with no extra model
request and exit 143. These fixtures use isolated accounts and a local provider;
they do not qualify live provider email authentication or arbitrary shared-pool
concurrency. Opt-in
`codex_alab_exec_handoff=debug` traces report stage and validity checks without
credential values.

Email-assisted sign-in remains aterm's shared service:
`vendor_mailbox_sign_in` -> `VendorApprover::with_mailbox` ->
`login_drive::sign_in_with`, using the common native mailbox path, including
Proton. The exec controller calls `alab-aterm::account_recovery`; it has no mailbox
reader, credentials store or provider-specific account selector.

Account handoff cancels an in-flight agent wait cooperatively. The wait emits
one interrupted terminal item before suspension finishes, and its retained tool
result identifies the account handoff. The Multi-Agent V2 namespace wrapper
preserves the handler's cancellation contract. JSON output closes the original
item as failed, since its public status vocabulary has no interrupted value.
The real-dispatch suspension test checks that the original turn remains open,
the canonical rollout retains the handoff result, and no additional provider
request is made. The native child-quota probe also rejects user-abort attribution;
the previous private package still fails that probe and a rebuilt package needs
to pass before this behavior is considered qualified end to end.

## Earlier original-turn handoff

1. Core's actual sampler publishes a process-local quota-wait observation. It
   identifies the attempted account and retains the original turn settings. A
   configured login alone is not an attempt; a changed owner, generation or
   bearer revokes the observation.
2. The controller keeps one native claim connection while aterm verifies fresh
   source/destination quota through the shared pool. Selection itself grants no
   execution. The native host captures the source process/image, exec generation,
   full argv, directory and auth path from the kernel.
3. Native `arm` binds the original checkpoint and selected account, creates the
   shared-pool receipt and reserves input/update custody. Contract 3 requires
   native `reserve` to admit the shared-pool selection while the original Core
   turn remains alive. An expired reservation is cancelled and prepared again;
   it cannot end the quota-waiting turn. Only after reservation does Core's local
   `suspend_exec_turn` stops the original turn and all descendant writers. A
   changed checkpoint or uncertain shutdown cannot authorize replacement.
4. The output owner drains queued events, checkpoints its renderer and flushes
   the streams. Native `execute` revalidates the reserved pool receipt and
   acknowledges the original monotonic deadline. The output owner then calls
   `execve`, preserving PID, parent wait, argv and standard streams. The selected
   auth path, native ticket and renderer reference are the only environment changes.
5. The replacement loads the native ticket before reading a prompt or stdin. It
   resumes the exact thread and calls `recover_exec_turn`, rather than starting
   another turn. Before emitting anything, it restores the renderer from a
   private file bound to its process, native operation, original turn and exact
   digest. Its matching first startup notifications are suppressed.
6. Core reserves the original unfinished turn and checks canonical history before
   calling native `consume`. The native host verifies the new exec generation,
   selected account, policy, original claim and cancellation state, durably
   consumes the receipt, transfers the claim connection and releases custody.
   Only then may Core issue a model request. Lost replies reconcile the same
   ticket and exact process generation.
7. A matching successful original-turn completion reports back through the same
   native operation. The host rechecks the consumed successor and shared account
   before proving the canary, allowing other prepared sessions to follow it.

The renderer checkpoint retains the JSON item counter, open item mapping, todo
list, token usage, error/final-message state and the corresponding human-output
state. It is written before native execution, without putting its contents into
environment variables or native control messages. The new runtime removes it
after successful recovery admission. Loading refuses symlinks, shared permissions,
another owner, stale operation/process references and changed bytes. The main
output task owns snapshotting and replacement; a worker cannot interrupt a JSON
write with a process-wide exec.

The native claim survives the original socket's close only while this operation
owns custody. Owner Stop and changed policy/authority revoke pending admission.
A missing process read is not proof of death. Native monitoring retains custody
until the applicable operation or process boundary is positively settled.

## Durable recovery

`SuspendedExec` captures `ExecRecoveryOptions` from the live regular turn:
original thread/turn, output schema, service tier and cyber setting. The current
source also captures the live step's collaboration/model/reasoning choices,
approval policy/reviewer and disabled plugins, and submits these through Core's
normal thread-settings constraint checks during recovery. This extension is
verified for the captured choices: the `--change-model` smoke against package
`9c19645050` reproduces a model change after editing the config during the
original quota wait. Package `ee199333a5` retains the original model across two
account moves and a lost consume acknowledgement, with both JSON and human
output. The source recovery tests also preserve the original reasoning effort
and summary despite different replacement defaults, across ordinary and
compacted history. Post-consume cancellation and all-accounts-exhausted probes
still stop without an extra model request. This does not yet capture every
configuration input. Deserialization
is not admission. `recover_exec_turn` flushes queued persistence, reads canonical
latest-turn evidence, and rechecks it under Core's idle reservation before the
native callback. Successful request clones can reconcile a lost reply; unrelated
or previously attempted requests cannot borrow admission.

`load_latest_turn_history` reads backward through paginated history, across
compaction and bounded inherited segments. Model-context compaction cannot erase
the original start/input/terminal boundary. The display index is not evidence,
because it can lag the durable log. Changing or malformed canonical history
refuses recovery.

## Shared-pool validation and remaining work

`alab/smoke/exec_shared_pool.py` runs several native hosts against one roster,
account store and pair of logins, with an HTTP barrier that holds every session
at its original quota request before releasing any response. With native
`444dd088d1` and Codex package `ee199333a5`, the four-session probe fails:
one session completes and three exit at native commit. The account store still
marks the selected account as an unproven canary after the successful turn.
The repaired pool accepts freshly recomputed move flags after that same target
is proven, while retaining destination, identity, generation and current-policy
checks. Native waiting and successful completion reporting resolve the observed
failure; independent-store probes alone do not establish shared-pool safety.

The initial repair used native exec contract 2. An execute
reply can explicitly wait under the original deadline; the output owner retains
its checkpoint and remains interruptible while retrying that same operation.
After a matching successful original-turn completion, the admitted successor
reports completion to the native host. The host checks the consumed process,
claim, receipt, credentials and current pool policy before recording the shared
account proof. A consumed receipt alone does not prove the canary.

Native `7c4a437951` with package `052113f69f` passes both four- and eight-session
shared-pool probes. All original processes and turns remain distinct, every
session retains its completed tool output exactly once, all exit successfully,
every successor reports native completion, and the shared target ends proven
with no canary. The same build also passes repeated moves with a dropped consume
acknowledgement and a changed model config, JSON and human output, exhausted-pool
cancellation and post-consume owner Stop. The source tests pass (88 client tests,
the successful-completion gate and nine native quota tests); Codex lint has only
upstream warnings and native strict lint passes. These are local-provider probes,
not live email authentication or a test of concurrently changing destinations.

Contract 3 reserves before suspending Core, so waiting for another session's
canary cannot consume a grant after the original task has already stopped.
The slow-canary probe above verifies expiry and a fresh reservation while the
original quota-waiting turn stays alive. These results are retained in
`$HOME/.aterm/.ubd/codex-shared-pool-v3-{four,eight,slow130}` and
`$HOME/.aterm/.ubd/codex-exec-v3-{chain,human,stop,exhausted}`.

The shared driver also supports `--chain --completion-timeout 60`: the first
replacement exhausts before its canary is proven, forcing the waiting sessions
to select a third account. Native `7e4b30be9b` retained the turns but made three
followers wait roughly 137 seconds. Rejecting the stale reservation alone left
the old selection cached. Native `6bc9615429` also invalidates that exact cached
selection, and all four processes complete around 15 seconds after startup,
with five consumed receipts, two shared rotations, and the original turns and
completed tool output preserved. The result is retained in
`$HOME/.aterm/.ubd/codex-shared-pool-cache-chain/result.json`.

The `--late-prepare` variant holds the selected-account replies until all
sessions have prepared, then delays one reply until the first target is known
full. Native `975a0a0654` also invalidates a selection rejected at `arm`, before
an operation exists. The eight-session late-prepare and ordinary chain cases
pass with Codex `7a6cae40a6`, nine consumed receipts, two shared rotations and
all original processes and turns retained. The late case requires an observed
rejected arm followed by successful recovery; merely losing the delayed reply
does not satisfy it. Results are in
`$HOME/.aterm/.ubd/codex-shared-pool-arm-{race,normal}-eight`.

At this historical stage the controller excluded ephemeral and review execution.
The ephemeral branch now captures canonical history without a persistence
writer, preserves provider-omitted item fields, and restores the original
model context before a recovered turn starts. The context snapshot retains
review evidence, token-accounting boundaries, and context-update baselines;
process-local review authorization revisions are renewed. Two context tests
and all three original-turn recovery tests pass, including fresh ephemeral
runtime recovery without a rollout, restored model/schema/tier/effort,
refusal before sampling, and rejection of consumed-admission replay.
The native memory capsule is connected to a private embedded startup API;
the CLI's 70 library tests pass. This is not yet end-to-end qualification of
automatic ephemeral rotation: external configuration resources, queued input,
and failure recovery after suspension remain outstanding.
The ephemeral handoff branch now has a nonpersistent transport primitive in
`alab-aterm/src/exec_capsule.rs`: an immediately unlinked POSIX shared-memory
object, held by an owned descriptor that becomes inheritable only at exec.
The replacement checks the original process, native ticket, thread, turn and
payload digest before consuming it. The macOS real-exec test restores a
1.28 MB fixture and verifies that consumption closes the inherited descriptor;
wrong-turn and corrupted-reference checks leave the descriptor available.
All 90 client library tests and strict all-target lint pass. This validates
the transport only; the private ephemeral capture/restore path described above
is connected, while Linux transport execution remains unqualified.
The development branch also captures the original configuration layers and every
CLI runtime override in the private memory capsule, for persistent and ephemeral
threads alike. Replacement startup rebuilds those inputs with the current managed
requirements and current runtime network/authentication bindings. The capture
records the final loader invocation, including the headless approval-policy retry.
The configuration snapshot has two passing regression tests. Persistent startup
uses the private embedded bootstrap with the existing store-owned resume reader,
so a normal thread/resume config reload cannot discard the restored inputs.
The `cc6e5e0637` package passes a real-process two-switch chain with a lost
consume acknowledgement and changed model/developer instructions, retaining
the original settings on all four provider requests. A human-output managed
worktree chain retains the same checkout and edits. Eight concurrent processes
pass the delayed-selection shared-pool chain with nine consumed receipts, three
retired reservations, and two shared rotations. These runs use native fixture
`975a0a0654`; artifacts are under
`$HOME/.aterm/.ubd/codex-config-handoff-{chain,worktree,shared-eight}`.
The corresponding `stop` and `exhausted` cases both pass with exit 143 and
only the two original model requests; neither cancellation emits a new request.
The private package is not installed. The full repository gate passed on
`9f8aea6934`: CLI build/run, all ALab suites, owned-code lint, and hooked upstream
unit suites (including 2,951 Core tests and 6,185 TUI tests). The initial gate
found one `expect()` lint; the validated revision replaces it with pattern
matching. The five process probes above used the preceding `cc6e5e0637` package;
the lint correction does not change successful handoff behavior.
Root accepted input is now retained across suspension and restored only after
native admission. The `71d149396b` private package preserves accepted child mail
exactly once in JSON and human-output probes, including a lost acknowledgment
and changed model configuration. Eight concurrent sessions pass a delayed
preparation chain with nine consumed receipts, five retired reservations and
two shared rotations. The input-preservation gate passed on `4329f1a5d1`,
including 2,958 Core and 6,185 TUI tests; see [the input handoff record](EXEC-INPUT-HANDOFF.md).
No package was installed.

The current native matrix closes the active/idle descendant and ephemeral
recovery gaps described in the earlier stages above. Non-regular task modes,
external resource contents beyond captured bindings, interactive waiters and
failure recovery beyond the tested admission/cancellation boundaries remain
unqualified. These limits must not redefine the overall goal.

Run the real exec entry point with generated account credentials, the native host
and an isolated provider. `alab/smoke/exec_accounts.py` covers an initial native
account move and lost consume acknowledgement (`--chain` adds a second move,
`--human` checks human output). Check JSON and human output,
original turn identity, completed tools exactly once, parent wait/exit status,
repeated exhausted accounts, all accounts exhausted, cancellation and receipt
boundary failures. Run independent concurrent invocations against shared history
and distinct auth paths. Earlier 40-process credential-isolation checks and TUI
rotation checks do not prove this new headless handoff.
