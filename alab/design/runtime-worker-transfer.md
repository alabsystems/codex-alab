<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve running work while the frontend updates

The goal includes adopting a new executable during a response or tool invocation
without cancelling, restarting, duplicating, or losing that work. Waiting for an
empty composer and an idle Core is not the finished behavior.

## Current boundaries

`app-server-client::InProcessAppServerClient` owns a Tokio worker and an embedded
app-server in the frontend process. Core's model transports, tool futures, and
native credential-source lifetimes therefore disappear on frontend exec.
`exec_upgrade_forest.rs` captures idle inputs, and `exec_bootstrap_forest.rs`
explicitly rejects an active restored root. The current format-2 pipeline does
not carry an in-flight HTTP/TLS connection or a running tool future.

The existing app-server can keep a loaded thread running while a separate client
reconnects. `handle_pending_thread_resume_request` snapshots an active turn's
history, but that reducer omits unfinished assistant text. The ALab listener now
retains canonical unfinished assistant items and replays them to the resumed
connection as ordered `item/started` notifications after its reply. The existing
client handler seeds a live stream from their text. Completed history stays in
its existing format; completion retires the extra live copy. This is a useful existing extension point, but ordinary
TUI reconnect is insufficient: `finish_reconnect` may discard input, quarantines
uncertain submissions, and resets asynchronous UI owners. A live-update handoff
must preserve those owners or reconcile their effects explicitly.

Native SDK membership is process-bound. In aterm, `NativeRestart::admits` checks
PID, birth, parent, process group and session, then binds an exec generation.
Derived opens also remain attached to their actual actor. In codex-alab,
`native_source_launch::allocate_derived_owned` retains a real parent use and
member incarnation; its data is not a cross-process child constructor. Inheriting
a token, copying a Source, or serializing a parent's receipt cannot admit a new
backend process.

## Ownership direction

Use private backend ownership per root tree, started before its Core owners are
created. Each backend owns that tree's live model connections, tools, descendant
threads, goals, and real SDK memberships. Route the frontend's thread operations
to their actual retained backend. This avoids requiring unrelated running roots
to become idle together just to create a new root on the new version. The frontend owns input,
editor/display state and rendering. Replacing the frontend leaves the active
backend invocation alive, analogous to aterm retaining its child program while
aterm updates. This must not turn on shared daemon discovery or reuse another
session's backend.

Frontend control now has its own native allocation and authenticated connection,
separate from credential members. It is not counted as a credential reader and
needs no provider Ready receipt. Credential retirement or reconnection must not
retire this control tenure. Native admission still checks the actual process,
original connection and operation; backend requests additionally require the
actual foreground PTY. A control close acknowledges only withdrawal of that
control allocation, never completion of backend or tool cancellation. The real
SDK qualification now checks admission before credential readiness, survival
after explicit credential retirement, and subsequent controller withdrawal
while the same SDK process remains alive. The joined SDK/native protocol passed in a SIP-enabled isolated macOS guest:
all credential owners obtained native Closed receipts, followed by explicit
controller withdrawal while the original SDK process was alive. This used a
synthetic issuer and a real SDK test executable. It does not prove production
SourceV1 activation, positive foreground attachment, backend continuity or an
automatic update. The qualified native support landed on remote main as
`be2b2778490edb748cebd09872dbc3c9bb09d3b8` after the full nine-check gate.

After a frontend update, replace each backend at a qualified Core boundary before
starting subsequent root turns with the new backend version. A new independent
root can start on a new backend while an existing invocation finishes on its
retained owner. Existing idle-forest
capture can serve that separate transition; it cannot be used to restart an
already running invocation and call that preservation. Keep the admitted old
image and its owners while the invocation still needs them.

This direction is not wired into production. Before adding a launcher:

1. Native code must issue and own an independent backend constructor for the
   actual signed child, with explicit supervision and lifetime rules. Frontend
   readiness, a matching PID, a data descriptor or a public socket path must not
   substitute for that grant. Frontend config readers need their own valid
   membership or must become backend RPCs.
2. The backend must survive only an authenticated frontend handoff; normal exit,
   holds and failed startup still have defined cancellation and cleanup. A lost
   frontend socket alone cannot invent a new SDK actor or resume work. Updating
   the frontend must also work during manual chat and with Autopilot off; it must
   not enable autonomous work or override an explicit hold.
3. Preserve request receipts and events across attachment. Confirmed mutations
   must not be resubmitted; uncertain mutations require reconciliation against
   their actual retained owner. Resume/history hydration alone is not proof of
   exactly-once request effects.
4. Carry complete frontend state and input, using the existing editor/terminal
   ownership work. Drain the handoff's input frontier, including queued stop
   commands and their acknowledged backend effects, before automatic next-goal
   production resumes. Do not block the ongoing response just to hold its next
   automatic turn.
5. Recovery must reattach to the advancing backend, rather than reinstall an old
   Core snapshot over work that ran after frontend activation. Native custody
   must retain the correct image, event/request frontier and terminal/input
   owners until settlement.
6. Define and qualify the new runtime schema before advertising it. Native
   negotiation probes a binary's writer; one binary must not silently choose an
   incompatible writer by process mode. Reader declarations need real decoders
   and migrations. The existing format-2 package is not a private-backend target.

Native `Registry::reserve_owned_worker` now provides a separate construction
boundary for a native-owned child. It retains the independently admitted
starter, scope and source gate; the consumer cannot claim the reservation until
`OwnedLaunchReservation::bind` checks an actual unreaped native child handle,
kernel parent/session/dedicated group, image and constructor environment. An
abandoned unbound reservation is revoked without declaring its child dead.
Binding does not bypass SDK image/session admission, create a member or Ready
receipt, or inherit a parent's Source. A bound child retains its actual SDK
lifetime when the launcher reservation is dropped. This API is not the private
launcher or supervisor; authenticated launch/attachment, child lifecycle and
root routing still need implementation and qualification.

`OwnedLaunchReservation::spawn` now joins this admission with an actual native
birth attempt and dedicated cohort. It rechecks scope, image and constructor
environment, supplies both SDK session variables from the reservation, and
binds the returned real child. Pre-birth refusal retains an actual NotSpawned
receipt when available; a postbirth observation/binding failure retains the
actual Spawned receipt and revokes the unused constructor. `OwnedWorker` keeps
startup custody through cohort cleanup or explicit receipt transfer to the
running supervisor. None of these receipts proves SDK readiness or closure of
tools in separate process groups.

The next implementation milestone is a native-owned launcher, not another
transport-only fixture. It must launch a retained admitted image, bind the
independently reserved constructor to its observed child before credential
preload, and retain/reap the actual child under explicit frontend handoff and
exit rules. The frontend's valid native membership authorizes the request even
in manual chat; an autopilot task claim cannot be a prerequisite for updating.
The private RPC attachment must also authenticate its admitted frontend: a
public socket path or a matching user ID alone cannot authorize arbitrary turn
mutations. Use native custody of a private descriptor or attachment credential,
independent of SDK member/Source permission, and carry it only through the
authenticated handoff. Upstream's managed daemon is shared across CLI
invocations and does not supply this per-root lifetime contract.

Use upstream's existing authenticated WebSocket listener for attachment:
`app-server --listen ws://127.0.0.1:0 --ws-auth capability-token
--ws-token-sha256 <digest>`. Native code keeps the unpredictable token and reads
the actual ephemeral address from the child's retained output. Only the digest
goes in argv; no token file or credential-bearing child environment is needed.
Deliver the token only to the authenticated parent/target controller through
native custody. The existing remote app-server client accepts this bearer.
The native supervisor must validate child liveness and SDK construction before
exposing the endpoint and invalidate the attachment on child death. The banner,
HTTP readiness, matching user ID or bearer alone cannot establish SDK readiness,
model permission, or continuity of a retained invocation.

The SDK-readiness check must use the actual child's bound process incarnation
and constructor in the native registry. Managed construction requires its live,
known manager registration after completed preload/Ready; a reader-only or
policy-unavailable record is insufficient. API-key, external and no-credentials
providers instead finish `complete_unrelated`, which records a validated
registration without creating a Source or Slot grant. That distinct successful
construction path must work too. Do not infer readiness from a member count,
copy a parent's registration, or accept a caller-reported Ready bit.

## Feasibility qualification

The ALab-owned `runtime_worker_reconnect_tests.rs` uses the existing private
app-server fixture and a gated, no-credentials Responses provider. A distinct
frontend child consumes a real partial response, execs itself, and rejoins the
same live turn in the unchanged backend. The test requires a fresh frontend
image with the same PID, the original session/turn and partial text, one provider
request, and completion containing all response segments. An independent
subscriber observes additional deltas while the new frontend is deliberately
detached, before it may rejoin. A FIFO response barrier also checks that targeted
prefix replay does not reach that existing subscriber. A paired negative
control kills and reaps the backend while its stream is held; the frontend must
observe actual closure and cannot report completion or launch another request.

The companion `runtime_tool_reconnect_tests.rs` starts an actual Unified Exec
shell that writes its PID through a private FIFO before frontend exec, then
blocks on a second FIFO. The new frontend rejoins the original in-progress turn
and receives its canonical pending command start. The parent checks the same
tool PID remains alive before releasing it, then requires one start and one
finish side effect and exactly the expected tool-call/final-answer model
requests. This exposed the same unfinished-item gap for commands: the listener
now retains command starts and output, retiring them on canonical completion,
and replays them only to the new attachment after its resume reply.

These are tests of existing Core/process/RPC behavior, not production child SDK
admission, a new-version TUI, native Go, or complete UI/input recovery. Its test
results must be recorded separately. The tool case covers an invocation still
inside its active turn, not a yielded background process crossing later turns
or native supervision after backend death. Next qualification must include
real native child admission and supervision, pending RPC effects,
queued input/control keys, replacement failure and recovery, then the signed
end-to-end transition and required gates.
