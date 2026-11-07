<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native parent source use

This private protocol extends [source members](accounts-source-members.md).
It is an implementation contract, not an enabled capability. Do not add the
SourceV1 image mark until the complete consumer and native controller qualify.

A native use spans an admitted root turn and its tools, approvals, compaction,
and inherited resource work. Finishing an HTTP body does not end its parent
use. Cached managers and factories own no activity use. The last local use
starts an owned, idempotent end operation; quiescence waits for its actual native
acknowledgement. EOF is unknown, never an end acknowledgement.

## Requests

All verbs use the authenticated private `codex-auth` channel. Common fields are
`schema=1 request=<rpc32> operation=<semantic32>`. Keep the existing per-connection
RPC prefix/counter sequence and exact Waiting request replay rules.

| Verb | Additional fields | Semantic operation |
| --- | --- | --- |
| `member-use-begin` | `member=<32> registration=<32>` | Use nonce |
| `member-use-end` | `member=<32> capability=<64>` | Same use nonce |
| `member-use-close` | `member=<32> registration=<original32>` | Same use nonce |
| `member-open-derived` | `use_member=<32> use_operation=<32> capability=<64>` | New open nonce; **no common member field** |
| `member-preload-use` | Existing target member/preload fields plus `use_member=<32> use_operation=<32> capability=<64>` | Preload nonce |
| `member-ready-use` | Existing target member/registration/actual manager counters and source fields plus the three use fields | Matching preload nonce |
| `member-reader-ready-use` | Existing target member/registration/actual source fields, no manager counters, plus the three use fields | Matching preload nonce |
| `member-refresh-use` | Target `member=<32> slot=<slot> expected_digest=<64>` plus the three use fields | Existing refresh operation |

All hex is lowercase. A capability is 64 hex characters. Use nonces are a fixed
random 64-bit prefix plus a positive monotone 64-bit counter for the retained
logical member incarnation; they are separate from RPC request counters. Preserve
this sequence across reconnect and ordinary registration/credential refresh.
A destination member has its own sequence.

Serialize semantic Begin admission per member **through** Active, Closed, or a
definitive unaccepted result, including Waiting and uncertain retries. Assign the
use nonce only after acquiring that admission semaphore, once before its first
wire send. A cancelled actor still queued behind an unresolved Begin allocates
no nonce and sends no Close; otherwise its highwater fence could suppress that
earlier Begin. Cleanup of an already sent Begin uses its original nonce and
bypasses the queue. Serializing first writes alone is insufficient because native workers may enter the registry out of order. End and
Close bypass that Begin queue and use a dedicated authenticated cleanup connection.
They require retained caller/member/use identity, not current Ready, a read of A,
or new application admission.

Native keeps bounded active records, a highwater, and bounded ended receipts.
Never evict active or uncertain uses. Replaying an accepted Begin returns its
original capability before current registration/readiness checks, for cleanup;
it does not itself authorize new work. An absent nonce at/below highwater never
remints a use. Close atomically fences a genuinely unaccepted nonce against a
late worker; an already active nonce returns its capability for explicit End.
Capacity refusal before acceptance does not advance the highwater.

## Replies

Begin, End, and Close return `OK codex-auth member-use <JSON>`. Closed JSON fields:

```
{schema:1, request, operation, session:{sid,process_pid,process_birth,epoch},
 member, state:"active"|"closed", capability:<64hex|null>}
```

Only Active has a capability; Closed requires null. Do not invent a registration,
credential generation, or source authority for a Closed observation. Every reply
binds the actual current authenticated transport session, process birth, member,
RPC request and semantic operation, and rejects unknown or missing fields.

Transport Waiting remains the existing strict `member-waiting` common frame.
Derived open returns the normal `member-open` reply with its newly issued member.
Contextual preload and Ready return their existing exact reply shapes/prefixes;
reader Ready still contains no manager AuthChangeState counters.

Refresh returns `OK codex-auth member-refresh <JSON>`: common fields plus `slot`,
`expected_digest`, `state:"waiting"|"ready"`, positive actual `generation`, and
actual `digest` (64 hex). Waiting here requires a known native revision. Before
storage/transport has such a revision, use the existing `member-waiting` common
frame with no revision. The legacy six-field refresh ABI remains unchanged.

## Scope and lifecycle

Native independently resolves capabilities to the actual immutable A source
family and records a derived member's exact parent link. A parent's A capability
never grants B. A known nonquiescent A member may begin/use/refresh/preload while
its admitted work drains. New unrelated opens remain closed by the reservation;
only exact accepted earlier opens may reconcile, or an actual active parent use
may authorize a same-A derived open.

Credential C alone does not change census membership. A new loaded snapshot or
registration resets that member's quiescence receipt. Validate the previously
loaded signed Slot and SameGrant relation at seal; never force owner-latest reload
after local reads have been sealed. Destination B needs a separate, typed native
authorization and a new SDK lifetime. A use ID, member ID, or assignment operation
string is correlation, never that authority.

Truthful NoNativeReader and PolicyUnavailable observations do not manufacture a
managed grant Use. Keep their local activity/lifetime checks, and preserve the
separate native role. Cleanup acknowledgements do not establish Source or Ready.
EOF/new lifetime cannot inherit an immutable ACK merely by reusing a member ID.

## Owned member retirement

`member-retire-owned` uses the common schema/request/operation fields. Its semantic
`operation` is one retained retirement operation, distinct from the preload
operation. Additional fields are `member=<32lowerhex>`,
`preload_operation=<32lowerhex>`, and `registration=<32lowerhex|none>`.
The SDK saves the attempted preload operation before its first send and captures
its latest owned attempt at actual last-use retirement. An acknowledged
registration must belong to that exact attempt. `none` means the acknowledgement
was lost; it is never a wildcard. An older attempt cannot retire a newer accepted
preload or registration.

The native service fences delayed unaccepted preload admission and waits for
accepted source workers and their real gate custody to settle. Member retirement
also waits for actual last-use and correlated End acknowledgements. A request
uses the original kernel caller/image/native-instance release-only proof and
cannot load a source, refresh credentials, or admit new work.

A terminal `OK codex-auth member-retire-owned` frame contains only the existing
closed common JSON fields: schema, request, operation, session, and member. This
success means Closed; it adds no registration, credential generation, capability,
or synthetic Closed flag. Unsettled work returns the existing `member-waiting`
frame. Exact-body replay includes the attempted preload and registration option;
a changed body under the same semantic operation is refused. The legacy
`member-retire` verb remains separate.

Each derived member uses its own persistent authenticated work connection. Native
derived admission must independently validate the original parent's work tenure,
actual kernel caller/image/native instance, App scope, active SourceUse, and full
immutable source authority before binding the child to its new connection. A
fresh connection is not generic peer or foreground permission. Exact open-nonce
replay cannot allocate again after the parent use ends. The typed native route
must be qualified before activation; the SDK protocol tests alone do not qualify
that front door.

A Closed/never-sent proof may release the local pending Begin wait, but an unknown
request remains owned. The old work stream is neither reframed nor closed to free
other members: their independent channels continue even when that old reply is
withheld. Retirement captures the latest attempted preload before first send and
retries one immutable semantic operation independently of waiter cancellation.

## Contextual owner refresh

Registered refresh uses the existing per-slot SDK coordinator and its immutable
refresh operation. Each admitted manager supplies its own actual native SourceUse
as admission proof; member ordinal and capability are not the canonical refresh
action's identity. A legitimate same-source manager can therefore reconcile the
original operation after leader cancellation. The legacy host remains explicitly
once-installed and optional; contextual refresh does not create a legacy route.

The registered auth-refresh flow retains actual Domain resource activity before
preload, owner refresh, local file reload, and actual Ready publication. Native
RPCs precede the local materialization flock; Ready follows its release. A known
refresh Waiting frame carries an actual positive revision/digest but is not Ready.
Unknown pre-read Waiting retains its exact RPC nonce, while semantic Waiting uses
a fresh RPC receipt with the same immutable refresh action. Revisions may not go
backward, and equal revisions require equal digests. This client implementation
does not qualify the separate native foreground/owned-child front door.
