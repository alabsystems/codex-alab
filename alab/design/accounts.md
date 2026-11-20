<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Codex account pool interface

Status: implementation design; runtime support must be qualified separately.

codex-alab presents the account pool. aterm owns enrollment, credential slots,
refresh custody, quota observations and account assignment. Native OAuth logins
remain the subscription authentication path; workspace personal access tokens
are a separate credential kind, never interpreted as OAuth refresh tokens.

## User interface

`/accounts` opens an asynchronously loaded account view. Each row shows its label,
current-session assignment, verified login readiness, quota windows and reset
times, observation age, and a concrete reason when unavailable. No credential,
credential path, token digest or mailbox content enters conversation history.

`/accounts add` starts aterm's staged enrollment workflow. Different accounts may
authenticate concurrently; a shared credential lineage has one refresh owner.
Existing active credentials remain usable until a new login is proved and bound.

`/accounts use <label>` moves this session to that signed-in account, and
`/accounts auto` moves it to the row a new launch would start on, by asking aterm
to restart the session there (`restart account=<label>` or `account-auto`). The
relaunch resumes the same conversation in a new process under the other login.
The move is this session's only: it changes no pool pin, active row or rotation
count, and it is not a rotation. It is refused, with nothing sent, while the
session record does not stand at its idle point (a turn runs, input waits, a
background terminal runs, a box waits for its answer, an active goal works between
its turns or the session is still starting), and aterm refuses a row that is not
signed in, and a session it does not find at that idle point when it comes to
restart it (`account-busy`). A background terminal (a dev server, a watch build)
must be stopped before the session can move: nothing ends it for the person. A
refusal the move meets after its pane was closed is said once in history. The decision,
the wire and what a restart loses are in
[`accounts-assignment.md`](accounts-assignment.md). The in-process assignment
request grammar this section used to point at is withdrawn.

Here a session is the running Codex process in its aterm session, including its
background threads and subagents that share an auth manager. A move ends all of
them with the process; only the conversation continues.

## Native boundary

Use the existing authenticated aterm connection and native session binding.
Negotiate an explicit account-pool protocol version. A missing capability gives
a local actionable status; it never starts an arbitrary shell command.

The read-only snapshot includes schema version, snapshot revision, session
assignment generation, selection mode, and bounded account rows. Labels are
display names; opaque stable account references identify operations. Quota
observations carry timestamps and unknown values remain unknown.

Mutations carry an operation ID, expected snapshot/assignment generation and
account reference. The owner validates session ownership and current policy.
Repeated requests retain one durable operation; polling reports waiting,
authentication required, ready, applied, cancelled or refused. Disconnect and
timeout mean uncertain progress, not permission to start a duplicate login.
No response contains credentials. Credentials reach Codex only through the
existing verified native credential-slot materialization and reload boundary.

### Interaction and recovery states

The account view keeps its selected row and scroll position while observations
refresh. Show the current session account separately from the highlighted row;
moving the highlight never selects an account. Account labels must be unique
within the pool for command lookup; ambiguous labels return matching display
names without submitting an operation. Internally, selection always uses the
opaque account reference from the displayed snapshot.

| Interaction | Visible result |
| --- | --- |
| Open or refresh | Keep the last snapshot visible with its observation time while loading; distinguish an empty pool from an unavailable host. |
| Add account | Open one enrollment operation with its own progress and cancellation; an email or OTP is entered in aterm's private enrollment UI. |
| Select an account | Show the requested label while checking that the session is idle. Report the reason if the move is refused, including in history if the pane has closed. Show “Restarting this conversation” once aterm accepts. After resume, `/status` shows the active login. |
| Return to automatic selection | The same, toward the row a new launch would start on; nothing restarts when that is the session's own row. |
| No answer from aterm | Say the session resumes this conversation if it restarts, and is otherwise unchanged; nothing is retried. |
| Enrollment needs interaction | Show “Complete sign-in in aterm”; opening the account view must not start another login. |
| Account or policy changed | Show the native reason and refresh the choices; a different destination requires a new explicit selection. |

Closing a view dismisses presentation and does not cancel an enrollment. An
explicit Cancel requests cancellation of the existing enrollment; show
“Cancelling” until native custody proves its outcome. A move has nothing to
cancel: once aterm accepted it, the process stops.

Account metadata belongs in the account view, not model messages or transcript
events. Navigation, refresh and dismissal remain available during network waits.
An unavailable quota is displayed as unknown, never as zero remaining. Speed
selection (Fast or Ultrafast) remains a separate model setting and does not imply
that a subscription account supports that service tier.

Before another model or account-scoped tool request, retire source-account
realtime/WebSocket connections, account model caches, Apps MCP discovery,
connections, credentials and tool catalogs. Re-establish those resources under
the acknowledged destination generation. Local project MCP configuration may
persist, but destination Apps permissions must be established independently.
Pending approvals and results retain their original account and turn binding.
Preserve the local conversation and workspace intentionally; do not merge the
destination account's remote history or account-scoped resources into them.

### Destination graph construction

The selected native handoff uses authenticated cold restart of the same
conversation. Normal destination startup constructs fresh configuration, auth,
model client/provider, model catalog and MCP/Apps resources under the admitted
destination source. It must preserve the original accepted input and wait for
actual destination readiness and native acknowledgment before continuation.
Source processes and captured resources retain their original ownership until
actual retirement.

Generic in-process assignment remains unavailable, and its request path was
removed on 2026-11-12: a manual move restarts the session instead
([`accounts-assignment.md`](accounts-assignment.md)). A future in-process
implementation must prepare and atomically publish the complete destination
resource graph across all actual participants. Configuration preparation and a standalone model-client
reconstruction cannot establish that publication proof. Configuration cohort and
resource-custody APIs remain separate; capability advertisement still requires
joined qualification of the complete handoff.

### Existing resource ownership mechanisms

The current tree already distinguishes credential refresh from account ownership
in `codex-rs/login/src/auth/change_state.rs::AuthChangeState`. Native assignment
must enter through the actual auth manager so its owner generation advances;
changing a displayed label or only replacing a credential file is insufficient.
Do not introduce a parallel client ownership counter.

`model-provider/src/request_preparation.rs::prepare_provider_request` rejects
ownership changes during preparation. `core/src/client.rs::ModelClient::new_session`
and WebSocket setup reset source-owner connection state. These checks support the
barrier but do not prove that assignment occurs at a safe turn boundary.

`codex-mcp/src/runtime.rs` captures the auth owner for published runtimes, and
`core/src/session/mcp.rs` passes auth changes into runtime projection. The
assignment acceptance test must verify source Apps connections and catalogs are
retired before any destination tool dispatch; detecting dirty state alone is not
that proof. `models-manager/src/manager.rs::refresh_after_auth_change` provides
catalog refresh, whose timeout must not authorize use of a source-account catalog.

`core/src/session/async_compaction.rs` and compacted-history installation compare
the captured auth owner before publishing a result. Preserve that fence when
waiting for compaction. `core/src/realtime_conversation.rs` has an awaited
`shutdown` that cancels and joins conversation tasks; account retirement
(`retire_session` in `alab-core/glue/assignment_lifecycle.rs`) shuts it down
explicitly, since cached WebSocket reset does not shut down this separate
conversation manager.

The snapshot revision must describe authoritative roster, binding and policy
state. Observation timestamps and time-derived quota presentation do not belong
in that revision. Mutations still revalidate the resolved account and credential
lineage immediately before publication.

### Session credential source

Use a native session-owned credential materialization and a typed per-manager
source binding. Never implement assignment by changing process environment
variables or overwriting a shared account's credential file. Preparing a target
does not install it; installation follows the process-wide resource barrier and
the destination acknowledgment precedes another model or account tool request.

The private source handle binds caller, operation, slot reference, exact document
digest, credential generation and assignment generation. Its bounded file handle
is consumed internally by the auth storage adapter, has redacted diagnostics,
and never enters the public snapshot, UI or conversation history. Reloading the
startup path alone cannot rebind a manager whose source was fixed at launch.

Native grant ownership and refresh follow the existing managed-auth contract in
`alab/CONTRACT.md`: one encrypted owner per proved refresh lineage, with separate
session materializations. Copies and local IDs do not create independent grants.
Prove ownership and reject unsupported or ambiguous live consumers before
publishing a marker; an unqualified source stays unmarked. Preserve ordinary
authentication and active credentials while a new source is staged and verified.

### Private source protocol

The persistent member lifecycle is specified in
[`accounts-source-members.md`](accounts-source-members.md). Its integration is
required before advertising process-wide source support.

The Codex auth adapter obtains its materialized source on a dedicated authenticated
connection with `@<sid> codex-auth source operation=<32 lowercase hex>`. This is an
internal credential channel, independent of enhanced UI and supervision. The
native endpoint must prove the connection's kernel peer, foreground Codex image,
PID and process birth using the same session-claim boundary as account discovery;
an SID, claimed PID, old slot reference or persisted resume record alone grants
no access. Recheck that proof before delivering a completed background read.

While reading, return `OK codex-auth source-waiting <JSON>` with exactly
`schema`, `operation` and `session`. The ready reply is
`OK codex-auth source <JSON>` with exactly those fields plus `slot`, `path`,
`account`, `user`, `credential_generation`, `digest`, `assignment_generation`
and `native_intent`. Schema is 1. Session contains `sid`, `process_pid`,
`process_birth` and the native host `epoch`; operation and caller must match
the request. Unknown, duplicate, missing and malformed fields fail closed.
Replies are bounded to 16 KiB. Errors must not echo the body, path or credentials.

`slot` is a canonical lowercase hexadecimal encoding of the private slot key,
bounded to 256 characters. `account` and `user` are 64-character lowercase
hexadecimal identity digests; `credential_generation` is positive and `digest`
is the exact document's 64-character lowercase hexadecimal digest. The absolute
UTF-8 `path` is bounded to 4096 bytes and excludes controls and dot components.
The auth adapter must independently open and verify the actual canonical,
owner-private, single-link regular file under its refresh lock, bound to this
receipt; the wire parser's path validation is insufficient.

Assignment generation zero requires an explicit JSON-null `native_intent` and
does not authorize a manual account rebind. A positive assignment generation
requires its admitted 32-character lowercase hexadecimal native intent. A
receipt proves a source, not a safe switching boundary: installing it still
requires the exact auth manager's held process-wide boundary permit and current
native assignment acknowledgment. Preparing or polling a source cannot publish
an assignment. No source receipt belongs in a public account snapshot, model
event, history entry or user-facing diagnostic.

Waiting retries retain the same operation and connection, bounded to sixty
seconds. A timeout leaves progress uncertain. Keep the existing six-field
`codex-auth refresh` protocol separate; source discovery must not silently
reinterpret a refresh request or transfer its ownership authority.

## Recovery order

On quota exhaustion, attempt an eligible earned reset first, then choose another
eligible account if needed. Any ordinary wall whose reset is still ahead is
eligible: the twelve-hour refusal was removed on 2026-11-06 (`76eacbc29b`), and
only the preflight keeps a five-minute headroom.

The second step needs the walled turn to end, because both the Autopilot's
`follow_renewal` and aterm's rotation act on an ended usage wall. An
interactive root turn therefore ends on its original usage error when all of
these hold: the fresh usage read shows a wall the host would leave (below), no
earned credit can be redeemed (a credit list read as empty, or two polls in a
row whose list could not be read or whose credit was refused), no redemption
is in custody, and a host that can rotate accounts is acting for that turn and
shows another account to move to (`codex_alab_core::quota::WallHandoff` in the
turn's store, installed by the Autopilot and answered from the same policy
gate rotation later needs, and from the host's `codex-accounts` snapshot: two
rows a session could move to, or one besides the row the host names as the
session's own), and no background terminal runs in the session (added
2026-11-14, and amended after its review: core's own count over every thread
the thread manager lists, internal threads aside, not the TUI's footer count,
which covers only the thread on screen). While one runs the turn keeps polling
for an earned reset, because the restart would end the terminal and aterm's
rest fence sees only the shown thread's; another root thread's walled turn
does not end for the move either, and the move is not asked for. Where the
host's pool shows another account to move to (read for the word on every
path, amended 2026-11-15 after the re-review), the person is told once per
wall why the move waits, in the history of every root thread, so the root
thread on screen shows it (a sub-agent's thread is not told); with none, the
hold stands and nothing is said. The first look after the terminals end ends
the turn for the move.

A wall the host would leave is one of two, and the host is told which
(`codex_alab_core::quota::SpentWall`):

- The weekly window spent with its reset ahead.
- The five-hour window spent on a week that reads as usable, with its reset
  further away than the bound the host advertises plus one minute, measured
  when the host's policy has been read (the wall carries the moment it
  resets, not a time left). The host
  advertises the bound as `codex_rotation_five_hour=<seconds>` in its
  `harness policy` reply; a host without the word leaves no five-hour wall.
  `limit_wait` must be on. The installed aterm measured on 2026-11-12
  (0.198.0) does not print the word, so under it only a spent week is left;
  aterm main prints `codex_rotation_five_hour=900` since 2365b983b6c, so a
  release after 0.198.0 leaves both.

In every other case the original request stays and checks for a credit every
30 seconds until the provider's reset: no such host, a standby, a thread that
is not the primary one, a pool with no other able account (one enrolled
account included), a host that does not show its pool, a five-hour wall the
host would not leave, a usage outage, or an uncertain or propagating
redemption. Exec keeps its request across the wall and never consults the
handoff. The pool evidence is the host's cached record;
`alab/continuous-earned-resets.md` lists the refusals it cannot show.

The wall the host is told is the walled request's own usage read's, with that
read's reset. The 429 does not decide whether it is offered: the read's wall
is offered whatever reset the 429 named (none, the window's own, or another
moment). The 429's reset bounds only the request's own wait, for a credit and
for the host's answer; a request whose 429's reset passes first ends with no
yes.

Once the turn has ended, the host is asked for the handoff only until the
wall's own reset plus the reset grace, counted in real time across host sleep,
unless a receipt is already bound. After that the session resumes on the
account it is on, as it does when the host answers `not-needed`. For a turn
the host's yes ended, the wall is the one the host was asked about, with the
reset the walled request's usage read named, a spent week's as a five-hour
window's, whatever the 429 named: the turn's handoff keeps the wall it
answered yes for, and the turn's error builds its usage wall from that wall in
place of the 429's. The ask's limit, and the resume after a lapse or a
`not-needed`, then run on that window's own reset; the journal records the
429's reset, or that it named none, beside the read's. A usage wall no yes
ended keeps the 429's reset, or none: a request whose 429's reset passed
before any yes, exec, a provider quota recovery does not serve, and a point a
restart rebuilt from history alone. An unbound intent carries its wall's
reset in the record, so the limit holds in the process after a restart too. A
usage wall that names no reset has no such limit, and neither has an intent
that was bound before a restart and retired after it
(`alab/continuous-earned-resets.md`, "The wall of an asked handoff is carried
across a restart" and "Offer a five-hour wall on its usage read alone").

Preserve the original request and completed tool results through the existing
handoff protocol. Do not replay a completed side effect. A fresh authenticated
allowance observation precedes retry. An exhausted pool waits on the earliest
relevant reset or authenticated state change without repeated prompts.

Authentication expiry asks the owning credential slot to refresh. Only a
definitive need for a new grant starts staged sign-in. Human security challenges
retain one quiet pending operation; automation does not bypass the challenge.

A login lost for good (expired, reused, invalidated or revoked) stops the
session today, even with other accounts signed in. The decided move to another
signed-in account, limited to a token's ordinary end, and the deferred early
move before a wall are in [`accounts-lost-login.md`](accounts-lost-login.md);
neither is built.

## Qualification

Require tests of stale generations, duplicate requests, concurrent sessions,
same-lineage refresh serialization, independent-account enrollment, cancellation
and uncertain replies. Verify manual selection changes only its requesting
session, automatic selection respects current eligibility, and turn handoff
preserves tool results. Verify account-resource invalidation and re-establishment
before the next model/tool call. Test OAuth and workspace-token credentials separately.
Use a real isolated aterm/Codex pair for final account assignment and recovery
acceptance; fixture success alone does not establish live login readiness.

## Supported launch and manager interfaces

An assignment covers every eligible auth manager in the Codex process, including
managers created for connectors, policy checks, analytics and background work.
Register each manager with its own storage and policy constraints. Sharing an
account does not permit replacing distinct managers with one global singleton.
Before commit, privately validate the prepared destination against every manager's
workspace, login-mode and enterprise requirements. The prepared identity preview
is internal admission evidence; its exact wire contract remains implementation
work and must not expose a credential document or path to the UI.

Use three distinct native authority types:

- `SourceWorkLease` holds the shared kernel gate during supported launch,
  refresh and source rebind work.
- `PublicationLock` holds the exclusive gate. Acquiring a lock alone grants no
  authority to activate a credential lineage.
- `PublicationLease` additionally proves the registered live sources and managers
  satisfy the publication requirements. Only that verified lease may participate
  in constructing fresh activation authority.

Acquire the source gate before the account lock. Follow with immutable grant,
assignment, session-auth and slot locks in that order. Refresh custody retains
its shared lease through response retention and durable reconciliation. Supported
launch paths retain their reservation until the actual child acknowledges source
readiness or is confirmed dead; typing a launch command is not acknowledgement.
A synchronous owned review child may retain the shared lease for its lifetime.

All supported direct Codex launches must join admission before loading auth.
Process observation supplements registration; an executable census cannot replace
source ownership proof. Missing, unresolved or unsupported known consumers refuse
activation. Do not advertise this capability until GUI, cold-launch, headless and
manual admission paths, manager registration and the validator are qualified.

Refresh completion records shared credential progress separately from session
materialization. A failed or departed caller must not discard a provider response
or cause another refresh POST. Reconcile a retained response without HTTP, then
materialize each still-authorized session independently. Slot keys and document
paths include the immutable grant binding, so revisions of separate accounts
never share a revision domain. Rotation lineage evidence must also retain spent
token digests internally: a newly received login response is not independent if
another owner has already consumed its refresh token. These digests never appear
in the account view or conversation history.
