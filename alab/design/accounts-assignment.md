<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Process-wide account assignment

Status: agreed request grammar for client/native integration, not a shipped capability.
The native owner, process barrier, source adapter and restart custody must be
qualified together before `/accounts use` or `/accounts auto` becomes available.

## Operation

Use one durable 32-character lowercase hexadecimal operation ID for the entire
assignment. Its native intent is that same ID. A source-read nonce is separate:
reading a private source neither submits nor acknowledges an assignment.

`codex-accounts-change` actions:

| Action | Exact inputs after the action |
| --- | --- |
| `assignment-prepare` | `schema=1 operation=<id> pool_revision=<pool64> expected_assignment=<u64> selection=<auto-or-account64>` |
| `assignment-commit` | `schema=1 operation=<id> boundary=<32hex> source_credential_generation=<u64> auth_generation=<u64> owner_generation=<u64> source_generation=<u64> login_generation=<u64>` |
| `assignment-poll` | `schema=1 operation=<id>` |
| `assignment-ack` | `schema=1 operation=<id> boundary=<32hex> assignment_generation=<positive-u64> credential_generation=<positive-u64> digest=<64hex> account=<64hex> user=<64hex> auth_generation=<u64> owner_generation=<u64> source_generation=<u64> login_generation=<u64>` |
| `assignment-cancel` | `schema=1 operation=<id>` |

All numbers are canonical decimal. Every request is bound to the authenticated
kernel peer, native session, process birth and admitted executable image. Retargeting
an operation is refused permanently, including after a source revision reverts.
The pool revision covers current authority, not observation timestamps.
Generation zero means no prior native assignment and requires actual bootstrap
source proof; it never authorizes guessing a default credential path.

Commit carries the actual source `AuthChangeState`; acknowledge carries the
actual destination state. `auth_generation` is that type's `generation` field;
the other three names match its fields. These local counters are staleness and
reconciliation evidence, never authority to obtain a grant. Principal hashes
use the existing native renewal identity domains and differ from pool account
references. A source credential generation of zero is allowed only for the
explicitly unknown bootstrap case; it cannot manufacture a known revision.

The status response is `OK codex-accounts-change assignment <JSON>`, bounded to
16 KiB, with exactly these fields:

| Field | Meaning |
| --- | --- |
| `schema` | `1` |
| `operation`, `native_intent` | The same immutable operation ID |
| `session` | Authenticated `sid`, `process_pid`, `process_birth`, host `epoch` |
| `phase` | `waiting`, `prepared`, `installing`, `awaiting_ack`, `applied`, `cancelled`, `refused`, or `reconciling` |
| `boundary` | Exact boundary ID when known; otherwise JSON null |
| `assignment_generation`, `credential_generation` | Actual positive generations when known; otherwise JSON null |
| `target_ref` | Opaque account reference when known; otherwise JSON null |
| `selection_mode` | `manual`, `auto`, or JSON null |
| `reason` | Closed static reason or JSON null |

Prepared status has a target and selection mode, but no boundary or allocated
generations. Awaiting-ack and applied status require the actual boundary and
positive generations. An unknown operation can return cancelled or refused only
with a durable opaque tombstone; its target, mode, boundary and generations are
null. Without that evidence, polling an unknown operation is uncertain.

Reasons are `busy`, `unavailable`, `denied`, `stale_revision`, `source_changed`,
`target_unavailable`, `policy`, `cancelled`, `superseded`, `needs_reconciliation`,
`unsupported_consumer`, or `authentication_required`. No response contains
credentials, credential paths, grant references or a model-visible copy of the
private source receipt. Unknown fields and malformed combinations are refused.
A definitive terminal response requires durable evidence; timeout and disconnect
are uncertain. This grammar is agreed; end-to-end runtime qualification remains
required before advertising assignment support.

## Handoff

1. Prepare validates current source, destination, policy and live consumer
   capabilities. It records the immutable intent but does not change the active
   source, global pool pin or session selection mode.
2. Codex closes new admission for its shared auth manager, lets existing work
   finish, and retires source realtime, WebSocket, catalog and Apps resources.
   Its submission dispatcher remains able to finish tools and approvals.
3. Commit requires the exact fully drained manager boundary. Native code
   revalidates authority, allocates the actual assignment generation, and
   installs a verified session materialization using its typed assignment proof.
   It retains per-process assignment custody while awaiting acknowledgement.
4. Codex reads the private source with a new read nonce, checks native intent
   and assignment generation, and binds it through the actual auth manager.
   It then establishes destination resources under the resulting auth state.
5. Acknowledge binds the installed assignment generation and exact credential
   digest. Only a durable acknowledgement plus actual source binding and resource
   readiness allows Codex to reopen admission. Neither a prepared target nor a
   successful file write proves the switch complete.

Native enrollment generation, owner credential revision, session assignment
generation and Codex auth ownership state are separate domains. Tests must use
different values for them. A same-account selection may change selection mode
without falsely incrementing Codex's account ownership state.

## Interrupted work

Cancel is definitive only before publication starts. Once commit can have had
effects, reconcile the original operation and installed destination; never claim
the previous account was restored merely because a request was cancelled.
An exact repeated installation can repair partial publication or a lost ACK,
without allocating another assignment. Different intent, binding or caller at
the same generation is refused. Credential revisions cannot roll back.

Authenticate slot records and compare their generation against the canonical
assignment journal. A signed old record is not proof of the current assignment.
Source lookup while awaiting ACK reuses the operation's held native lease rather
than deadlocking on a second acquisition of its own file lock.

Persist the pending operation, boundary, complete source auth stamp and opaque
native locator before sending commit. Record the acknowledged destination stamp
after reconciliation against the signed native journal. Never interpret a
counter from an old manager or process as a current auth stamp.
Kernel launch environment remains the original source
after an in-process switch. Restart and successor-process construction therefore
require sealed native assignment custody for the destination, followed by
authorized PID/birth transfer. A slot reference, saved environment string or old
PID record cannot grant that transfer. Existing kernel-environment validation
remains in force for ordinary launches.
