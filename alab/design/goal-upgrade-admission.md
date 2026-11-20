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

The signed 0.11 client uses target protocol 3, exec custody 2 and runtime writer 2.
Later targets report the runtime formats they actually read in `runtime_readers`.
Keep the default capability response readable by the released caller when changing
a checkpoint format. The exact target image must have native signed-build
admission. Native source-exec Go binds its original
operation, fresh constructor command, primary and retained frame digest/size.
The destination loads through that constructor's runtime host, restores its
Core/goal forest behind admission, then activates after primary UI attachment
and event replay. Settlement retains its original connection until native
cleanup finishes. No decoded checkpoint grants model work by itself.

Validation covers hidden loaded goals, failed native row reads, queued mutations,
revocation before writes, final accounting, cancellation, concurrent commit/writer
admission, and destination accounting before native continuation. These component
fixtures do not substitute for a signed end-to-end live handoff and recovery run.

## Signed watcher observations on November 17, 2026

| Running image | Selected image | Observed result |
| --- | --- | --- |
| 0.10.0 build 2026111601 | 0.11.0 build 2026111701 | The watcher detected the update. Its strict capability decoder rejected `full_runtime`. The original process answered the next request on the same thread. |
| 0.11.0 build 2026111701 | A second physical copy of the same signed build | The watcher detected the inode change. The host reported `preparation=1 execution=unavailable input-fence=1`. The original process answered the next request on the same thread. |

Both observations used the real released executable in a private terminal and a
loopback model. The second observation ended at host discovery, before candidate
admission. Source reservation also requires distinct published build numbers.
The shipping host returned the same execution status when queried independently.
The recorded watcher logs have SHA-256 digests
`1ec08c97e5d849a68b646c844da5a69109abae8d1b69a0cfc2a23e725b60f1f1`
and `06e9d8bfc1f593601ec04d53da0ab2a69f1313b6344b2135409391b403e4e379`,
respectively.

## Signed package selection with a conversation open, November 18

Signed index 158 selected 0.10.0 (build 2026111601). Index 159 advanced Codex to
0.11.0 (build 2026111701), which index 164 still selected. In a private store,
the installed atpkg rolled back to 0.10 and reported that exact executable as
managed and ready. The test moved the inactive 0.11 package directory out of
the store, leaving the verified package archive available in the download cache.

1. The 0.10 app-server started a conversation and completed a request to a
   loopback provider. The kernel identified the PID's executable as the 0.10 image.
2. While that process remained open, `atpkg update --no-init --defer-busy-flip
   --default-selected codex` installed and selected build 2026111701. The
   package lookup reported index 164 and tree root
   `bd3e861163f3464def452eca98179b94994edf2a13e5058eaeae223e99e12792`.
   The unpacked executable matched the installed signed 0.11 executable byte for byte.
3. The same 0.10 PID answered the second turn. After it exited, a fresh 0.11
   process resumed the conversation and answered a third turn. The provider
   observed three distinct turn IDs on the original thread, with the preceding
   prompts and replies in the follow-ups.

The installed window also updated Codex autonomously on November 17. Its log
records a publication wake for index 157. The resulting atpkg pass
(PID 45856, 21:44:41–21:45:36 UTC) resolved index 160 and recorded Codex
advancing from build 2026111601 to 2026111701. The same pass recorded
`lane=window`, `verb=update` and exit 0. The next warm launch exited 0.
The notification and transaction can name different indices when the channel
advances between discovery and resolution.

A separate November 18 exercise used the managed `exec/codex` launcher in a
private signed store. A real 0.10 turn started one `exec_command` and polled
it twice. While its child waited, the unattended package command selected
0.11. The kernel still identified the original app-server and the same live
child. Releasing the child produced one output on the original turn. The next
managed launch ran 0.11, resumed the original thread and sent that output in
its first provider request. An exclusive-create check would fail if the shell
command ran twice. The result record has SHA-256
`719ed96930018ad97b41dc80189fc156ac8c9d9d7d42644c27cf18fe5e990099`.

Same-PID terminal replacement still needs a qualified native handoff.

For release qualification, start the original process directly from its admitted
signed store image. Select a distinct signed successor through the normal updater,
observe the same PID mapping the replacement executable and an increased exec
generation, and verify the next provider response completes on the original
thread. Capture the source and target build identities and the host's activation
and settlement outcomes alongside the thread observation.

## Recovery contract in aterm 0.206.0

Read-only discovery on installed aterm build `1794890684` returned
`preparation=1 execution=unavailable input-fence=1`. The runtime-data
endpoint reported `authority=data-only`. The signed Codex 0.11 client requires
`execution=available` in global discovery and `execution=unavailable` in
ordinary preparation replies. Before re-exec it also requires a separate `state=go` reply bound to its
exact operation, retained checkpoint and granted target constructor. [Released client checks](https://github.com/alabsystems/codex-alab/blob/70d63912591840da2a154f057dc0cade90a17d79/codex-rs/alab-tui/src/upgrade_runtime_data.rs#L20-L234)

Source inspection of aterm tag `v0.206.0` at `7d1365f2c5d4` explains the
rollback limit. Checkpoint access requires the original PID and kernel birth
identity. Successor admission requires the signed target image and entry
through its granted constructor. On process death the host closes the operation
and drops its checkpoint pin and executable leases. A relaunched rollback
image therefore lacks an owner authorized to restore the retained state.
Qualifying rollback requires transferring that ownership and observing the
same thread answer after a post-commit failure.
[Checkpoint ownership](https://github.com/alabsystems/aterm/blob/7d1365f2c5d488a8c59479d8a819124d3f40b4b1/crates/aterm-gui/src/native_runtime_data.rs#L121-L151) ·
[Operation closure](https://github.com/alabsystems/aterm/blob/7d1365f2c5d488a8c59479d8a819124d3f40b4b1/crates/aterm-gui/src/native_source_exec.rs#L313-L356) ·
[Successor admission](https://github.com/alabsystems/aterm/blob/7d1365f2c5d488a8c59479d8a819124d3f40b4b1/crates/aterm-gui/src/native_source_exec.rs#L549-L587)

## Host development after 0.206.0

On November 18, aterm main at `dcdea960c371` retained the signed images and
checkpoint when the original process died after the constructor grant. The
checkpoint must match the size and digest recorded for that grant. The host
releases the old input and supervisor claims, allows a new process to create
its own checkpoint, and keeps the original data until the transaction closes.
[Retention implementation](https://github.com/alabsystems/aterm/blob/dcdea960c3719127fcc37480afd21bdc222635ec/crates/aterm-gui/src/native_source_exec.rs#L316-L380) ·
[Source-death test](https://github.com/alabsystems/aterm/blob/dcdea960c3719127fcc37480afd21bdc222635ec/crates/aterm-gui/src/native_source_exec_tests.rs#L887-L988)

The retained data has a passive owner. Restoring a conversation after process
death still needs an authorized rollback reader and a successful same-thread
response from the restored process. Discovery at that commit still returns
`execution=unavailable`. The signed host release at this observation was
0.206.0, and the installed host returned the same execution status.
[Checkpoint ownership](https://github.com/alabsystems/aterm/blob/dcdea960c3719127fcc37480afd21bdc222635ec/crates/aterm-gui/src/native_runtime_data.rs#L149-L169) ·
[Development discovery](https://github.com/alabsystems/aterm/blob/dcdea960c3719127fcc37480afd21bdc222635ec/crates/aterm-gui/src/native_source_exec.rs#L575-L582)

## Recovery after a lost settlement acknowledgment

aterm added a read-only `settled source-exec` query for a replacement process
whose SDK connection closed while settling. In development commit `8900603540`,
codex-alab authenticates a second connection to the original listener. It
accepts settlement only when the host confirms the operation, primary thread and
build recorded at activation. A waiting response gives the host time to finish
processing the original connection's EOF. The supervisor then obtains its own
adoption acknowledgment before taking ownership. [Host implementation](https://github.com/alabsystems/aterm/commit/084277911919b467a78f4cb147193a24a2939175)

Six native tests exercised the SDK and supervisor over authenticated Unix
sockets with a host fixture, including a dropped reply, waiting, refusal and
changed identities. The complete SDK library run passed 355 tests with four
subprocess helpers ignored. The installed host still reported
`execution=unavailable`. Qualifying a signed live update still requires a
running process to adopt the selected signed build and continue its conversation.

## Development image compatibility with the 0.11 reader

On November 18, a compiled comparison reader launched the signed 0.11 image
and two 0.12 development images. Its executable came from `2061c8cece`.
The target-response parser, admission probe, exec-custody protocol and runtime
state source files all have the same Git blobs as signed 0.11 at `70d6391259`.
The probe checked the child PID, physical executable, device, inode, version
and advertised runtime compatibility.

| Image | Result from the comparison reader |
| --- | --- |
| Signed 0.11, build 2026111701 | Accepted |
| Signed 0.10, build 2026111601 | Rejected because its response omits `full_runtime` |
| 0.12.0-dev.20360 at `17f6dd5f2` | Accepted |
| 0.12.0-dev.20430 at `e317f1333` | Accepted |

The development executables had SHA-256
`d2f85467fa42a6a92ec51c21df77f4cdec43f12975ebaa9a9becefb93961e5f1` and
`484dd82a613273a91b03458e5cd1043b9d2d523cedfff1b503d1a4059b15b5a6`.
A second run built the reader from the exact signed 0.11 source,
`70d63912591840da2a154f057dc0cade90a17d79`. The reader executable had SHA-256
`3563f762523c4d63844177eececbb65ce062cb532cb25d230ac57ce93398a14b`.
Its native retained-image test accepted signed 0.11 and each development
image in the table. Signed 0.10 failed at the missing `full_runtime` field.
The reader checked the physical candidate process using the release code's
original admission probe.

Release qualification still requires a signed successor and a same-conversation
update through the host. The installed aterm 0.207.0 reported
`preparation=1 execution=unavailable input-fence=1`. Signed index 164
selected 0.11.

The host also checks whether the executable supports its per-session credential
constructor. Both signed Codex executables (0.10 and 0.11) and the ordinary
0.12 development executable `e8355ffcb2` contain the two machine-slot ABI
markers. All three lack `aterm session credential source v1`. The 0.207
registry returns `Unsupported` when this check fails. An accepted
`debug upgrade-target` response alone cannot qualify that constructor.
The [isolated source qualification](native-source-debug-proof.md) exercises
the credential implementation before the ordinary image advertises support.
[0.207 credential check](https://github.com/alabsystems/aterm/blob/ef072bbe1cfd9b8f030985b6ff8824a45834684a/crates/aterm-agent/src/harness/codex_alab.rs#L1202-L1218) ·
[0.207 constructor admission](https://github.com/alabsystems/aterm/blob/ef072bbe1cfd9b8f030985b6ff8824a45834684a/crates/aterm-agent/src/harness/codex_source_registry.rs#L1383-L1395)

### Signed 0.11 reader and the 0.170-based development target

On November 19 UTC the target reader compiled from the signed 0.11 source
`70d6391259` accepted development CLI `248ff99eca`, version
`0.12.0-dev.20770+codex.0.170.0.g248ff99ec`. Separate ordinary CLI tests
resumed a conversation written by signed 0.11 and sent its next request on
the original thread. After another Work client discovered Sol 6.2, that
request used Sol 6.2 and retained the earlier conversation. All twelve model
wire checks passed on this executable.

At the time of the check, the installed aterm command reported 0.208.0.
The running session answered `preparation=1 execution=unavailable input-fence=1`
to `aterm ctl @self native-act info source-exec`. Signed index 165 selected
codex-alab 0.11.0, build 2026111701, and the installed files matched its signed
tree. Shipping an automatic handoff still requires a certified successor, a
host that grants execution, and a successful turn from the same running
conversation after image replacement. [aterm 0.208.0](https://github.com/alabsystems/aterm/releases/tag/v0.208.0)
