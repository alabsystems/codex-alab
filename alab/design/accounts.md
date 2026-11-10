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

`/accounts use <label>` requests an assignment for this session. An active turn
retains its account through completion; the operation reports pending until a
safe handoff completes. Selection must not silently change a global pool pin.
`/accounts auto` removes this session's explicit selection and returns it to the
pool's eligible-account policy. Neither operation claims a switch before the
native handoff acknowledges the destination generation.

The safe boundary includes pending tools, approvals and compaction, not just the
TUI busy flag. Manual selection carries a separate typed assignment intent,
bound to its operation, thread, source generation and destination account. It
does not reuse the owed-turn quota recovery request or fabricate a weekly wall.
The agreed request grammar and interrupted-handoff rules are in
[`accounts-assignment.md`](accounts-assignment.md); status grammar is specified there, while runtime
qualification remains implementation work.

Here a session is the running Codex process in its aterm session, including its
background threads and subagents that share an auth manager. The originating
conversation ID fences the intent and its UI; it does not make another thread's
active work safe to switch. Block new turn/thread admission and drain every
request, tool, approval and compaction using that manager before retiring its
account resources. Keep the submission dispatcher able to finish existing work.

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
| Select an account | Show the destination and “Waiting for current work”; keep the current account marked until the handoff is acknowledged. |
| Return to automatic selection | Show “Automatic selection pending”, then the acknowledged current account and automatic mode. |
| Connection lost during a change | Show “Checking account change”; reconnect and poll the same operation. |
| Enrollment needs interaction | Show “Complete sign-in in aterm”; opening the account view must not start another login. |
| Account or policy changed | Show the native reason and refresh the choices; a different destination requires a new explicit selection. |

Closing a view dismisses presentation and does not cancel an enrollment or
assignment. An explicit Cancel requests cancellation of the existing operation;
show “Cancelling” until native custody proves its outcome. If publication has
already begun, reconcile that operation and display its actual result. A
successful response means the session can use the acknowledged destination,
not merely that its credentials were saved.

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

Generic in-process assignment remains unavailable. A future implementation must
prepare and atomically publish the complete destination resource graph across
all actual participants. Configuration preparation and a standalone model-client
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
`shutdown` that cancels and joins conversation tasks; the assignment coordinator
must explicitly retire realtime work, since cached WebSocket reset does not
shut down this separate conversation manager.

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

On quota exhaustion, attempt an eligible earned reset under the existing
inclusive twelve-hour policy, then choose another eligible account if needed.
Preserve the original request and completed tool results through the existing
handoff protocol. Do not replay a completed side effect. A fresh authenticated
allowance observation precedes retry. An exhausted pool waits on the earliest
relevant reset or authenticated state change without repeated prompts.

Authentication expiry asks the owning credential slot to refresh. Only a
definitive need for a new grant starts staged sign-in. Human security challenges
retain one quiet pending operation; automation does not bypass the challenge.

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
