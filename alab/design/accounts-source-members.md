<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Private source member channel

Status: integration contract, not an advertised capability. Native registration
and SDK lifecycle implementations must be qualified together. Existing one-shot
`codex-auth source` and six-field refresh replies remain unchanged.

Use one persistent authenticated native connection. Allocate its native Connection
token synchronously before dispatching background work. EOF invalidates it even
when the registry table is busy; an idle lane must retain live registrations.
An EOF is unknown source state, never evidence that credentials were retired.

## Requests

All actions use `@<sid> codex-auth`. Every request includes `schema=1`, a fresh
32-character lowercase hex `request` identifier, and a 32-character lowercase hex
`operation`. Retry the exact same request with the same identifier and body. A changed
body needs a new request nonce, including a newer Ready snapshot. Operation is
semantic correlation, not permission to borrow a native lock.

The request identifier concatenates a random 64-bit connection prefix and a
positive, monotonically increasing 64-bit counter, each encoded as sixteen hex
characters. Allocate counters serially before sending; gaps are allowed when a
request is abandoned before sending. Keep the prefix for the connection's entire
lifetime. Counter exhaustion closes the connection rather than wrapping.

The host retains bounded active requests and recent completed replies separately.
An exact retained replay returns the original outcome; changing its body is
refused. A counter at or below the accepted high-water mark without a retained
request is stale and must never execute again. Reject a different prefix on the
same connection. Capacity refusal before acceptance does not advance the mark;
an accepted operation whose reply was lost remains uncertain after its receipt
is evicted. Reconnect creates a new transport sequence and reconciles the
original semantic operation; it never assumes the old operation was cancelled.

| Action | Additional exact fields | Operation |
| --- | --- | --- |
| `member-open` | none | SDK open nonce |
| `member-preload` | `member=<native32>` | Source-read nonce |
| `member-ready` | `member=<native32> registration=<native32> credential_generation=<positive-u64> digest=<64hex> account=<64hex> user=<64hex> auth_generation=<u64> owner_generation=<u64> source_generation=<u64> login_generation=<u64>` | Matching preload nonce |
| `member-reader-ready` | `member=<native32> registration=<native32> credential_generation=<positive-u64> digest=<64hex> account=<64hex> user=<64hex>` | Matching preload nonce |
| `member-unavailable` | `member=<native32> registration=<native32>` | Matching preload nonce |
| `member-unrelated` | `member=<native32> kind=<closed-kind>` | Matching open nonce |
| `member-retire` | `member=<native32> registration=<native32-or-none>` | Retirement correlation nonce |

Numbers are canonical decimal. Unknown, duplicate, missing and extra fields are
refused. `member-unavailable` means an actual policy refusal with no cached auth;
it cannot describe a successfully cached API key. Unrelated kinds are `api_key`,
`external`, `workload`, `bedrock_api_key`, `bedrock_access_keys`, `headers`,
`agent_identity`, `personal_access_token`, and `no_credentials`. The qualified SDK
must derive the kind from its actual selection; raw ChatGPT file/keyring auth
cannot declare itself unrelated.

`member-reader-ready` describes an actual managed credential reader without an
auth manager. It validates the same preloaded source and actual document/policy,
and supplies no manager counters. Its returned storage and all derived request,
body and WebSocket uses retain the reader lifetime. Native custody retains the
reader's shared source gate until explicit last-use retirement or confirmed
kernel death; EOF invalidates readiness and retains exclusion. Process admission
must account for these readers separately from manager readiness and unrelated
credential kinds. This role remains unadvertised until both implementations and
the lifetime/invalidation paths are qualified.

Retirement with `registration=none` covers an issued member that never obtained
a registration. Native code validates the actual lifecycle; this value never
permits discarding a live credential use. Last-use retirement includes storage,
auth snapshots and derived HTTP/body/WebSocket uses, not just the auth manager.

## Replies

Replies are single status lines bounded to 16 KiB. Success prefixes are
`OK codex-auth member-open`, `member-preload`, `member-ready`,
`member-reader-ready`, `member-unavailable`, `member-unrelated`, or
`member-retire`, followed by JSON.
The common exact fields are `schema`, `request`, `operation`, `session`, and
`member`. Schema is 1 and correlation fields must match the request. Session is
the existing private source session binding: SID, actual process PID, canonical
process birth and native host epoch.

Open adds no fields. Preload adds `registration` and `source`; `source` is the
complete existing private Source JSON object, with its operation equal to this
preload nonce and the same session. Ready and unavailable add `registration`,
echoing the validated registration; reader-ready has that same exact reply shape.
Unrelated adds the actual native-issued
`registration`. Retire adds no fields. No response contains credential bytes.

`OK codex-auth member-waiting` has only the common fields; member is JSON null
until open has allocated it. Waiting proves neither allocation nor readiness.
Static errors use the existing `ERR codex-auth` namespace and never echo a body,
path or secret. Timeout and disconnect are uncertain, not successful retirement.

## Authority and lifetime

Member IDs are native-issued epoch/ordinal identifiers. Exact peer, connection,
registration and current source must validate before Ready. Ordinary credential
refresh may advance read snapshots while retaining the immutable source scope;
account/grant reassignment retires the old scope. Native Ready snapshots are
single-member evidence, not a frozen-process or publication capability.

Foreground admission uses the actual PTY and kernel image. When the actual kernel
launch environment contains `ATERM_CODEX_SOURCE_LAUNCH`, admission must use its
reserved constructor path and never fall back to foreground. The SDK must validate
that constructor before any credential load/use, including API environment and
ephemeral modes. Revoking an unclaimed reservation denies every later bind;
it does not assert that an unseen process died.

Unrelated completion releases constructor custody without minting Source,
GrantBinding, assignment or independent-grant authority. In particular, an opaque
external provider is not proof that no other actor can refresh the same family.

This first backend supports existing managed Slots and unrelated constructor
completion. Unmanaged-file admission, owned canonical Source0, native-operation
lock borrowing, complete process freeze, restart succession and publication
coverage remain separate required interfaces. They must not be simulated using
zero credential generations, a guessed path, caller-supplied operation IDs or
policy-unavailable readiness. A Source capability is not advertised until the
required SDK and native paths work together.

## Reserved actor-family freeze and native lock borrowing

Status: agreed design for the next native/SDK slice, not an advertised capability.
`ReadySourceProof` and `ReaderSourceProof` remain observations. Neither an operation
ID nor a ready-member snapshot can substitute for the following owned protocol.

A healthy assignment selects its source family from the native prepared assignment
and the initiating member's authenticated slot/grant association. It covers every
manager and direct reader in that caller using that family, including distinct
workspace/network policies and transient readers. A policy/configuration difference
must not partition two managers reading the same slot. Genuinely independent grant
and owned-storage authorities may be disjoint; a differing thread UUID, account
label, environment string or caller-provided family hash does not prove that.

The proposed private actions use the existing serialized persistent connection and
request sequence rules. All begin with `schema=1 request=<sequence32>`:

| Action | Remaining exact fields | Result |
| --- | --- | --- |
| `member-freeze-reserve` | `operation=<assignment32> member=<origin32> registration=<registration32>` | Native-issued `freeze` capability and `census` revision, bound to the prepared operation, actual caller, immutable source family and full member set. |
| `member-freeze-manager` | `operation=<assignment32> freeze=<cap64> census=<revision64> member=<member32> registration=<registration32> credential_generation=<positive-u64> digest=<64hex> auth_generation=<u64> owner_generation=<u64> source_generation=<u64> login_generation=<u64>` | Actual manager snapshot and quiescence receipt after all request/body/WebSocket/derived uses retire. |
| `member-freeze-reader` | `operation=<assignment32> freeze=<cap64> census=<revision64> member=<member32> registration=<registration32> credential_generation=<positive-u64> digest=<64hex>` | Actual direct-reader snapshot and last-use retirement, without fabricated manager counters. |
| `member-freeze-seal` | `operation=<assignment32> freeze=<cap64> census=<revision64>` | A native-issued `borrow` capability only after every affected member is quiescent and the native actor-family exclusive gate is actually held. |
| `member-freeze-release` | `operation=<assignment32> freeze=<cap64> borrow=<cap64>` | Terminal release only against the operation's actual successful handoff or authoritative unchanged-source cancellation, including all required member readiness receipts. |

Capabilities are opaque native-issued values, not credentials or names from which a
client can reconstruct authority. Replies carry the exact request, operation and
existing authenticated session envelope. The member set and revisions are native
state, not caller-selected lists. The census binds membership and immutable scope,
not a Ready snapshot version that can advance on ordinary same-grant refresh.
At seal, validate each reported loaded revision against authenticated existing
Slot/SameGrant evidence. Do not materialize a newer shared-owner revision and then
demand another SDK load after reads have been sealed. Any necessary final read
occurs explicitly before that member's quiescence receipt. Reserve closes relevant new factory/load/rebind
admission before returning the census. A late or uncensused member invalidates seal;
it must join the retained operation or be proved disjoint. An explicitly retired
member already in the census satisfies that obligation; it does not force a new
census. Only additions or unresolved source changes invalidate/join the sealed set.

Before closing admission, settle/cancel already-started local open actors, or retain
a same-nonce reconciliation/retirement route under the reservation. A lost open
reply cannot strand an accepted native member behind the new admission gate.
Queued factories that have not acquired admission must exit on shutdown, rather
than being joined while waiting on that closed gate. Existing parent work that
may create a derived source use must join the reservation before final sealing.
No timeout manufactures a quiescence receipt or permits release.

The SDK `ProcessHost` must consequently have two stages: reserve the catalog/native
operation before retiring local work, then asynchronously seal after all affected
manager domains and direct-reader uses drain. Its initial validation proves a live
reservation, not a completed freeze. The retained actor holds both stages through
transient errors and reconciles the same native operation. An explicit asynchronous
finish releases the native context after genuine handoff; dropping a local guard or
losing its transport cannot masquerade as this finish. Approvals, interrupt and
shutdown dispatch stay live throughout.

The native borrow is tied to the exact held kernel guard, caller, source/census
version and immutable family. Source/commit/refresh code executes under that owned
context without acquiring a second shared lock against its own exclusive lock.
The context skips only the gate it actually borrows: an actor-family assignment
still checks global canonical-publication exclusion and all account/grant policy.
No unchecked operation-string lookup, copied file descriptor or clonable token
creates this context. EOF leaves the freeze uncertain and retained; same-operation
reconciliation or kernel-confirmed process retirement is required.

Destination preload may borrow the source context's actual held global shared
lease only after checking the same authenticated gate identity. Reacquiring that
shared lock behind a queued global writer could deadlock the handoff. This borrow
does not confer global exclusive authority. Destination B still needs its own
family gate unless that exact gate is already held, and its authority comes from
the native prepared target or installed assignment. Holding source A's family
exclusive lock never authorizes B or proves that B is excluded.

## Global publication is a different scope

The current global source gate is conservative exclusion, not proof that every
reader of every account must retire for an unrelated enrollment. First ownership
of an unmanaged canonical source needs complete coverage of that source's real
readers, including aliases and unknown legacy consumers. Registry/census closure
is global where needed to establish that coverage, but publication must then hold
the gates for the actually affected canonical source/grant families. Known private
readers of an unrelated immutable grant must not block publication forever.

Do not solve this by silently skipping the global gate. The native owner needs an
explicit coverage proof distinguishing affected, proved-disjoint and unresolved
consumers. A reader of an old private grant may remain valid while a row's default
moves to a newly enrolled grant. Current admission checks that equate an old grant's
liveness with the row's latest success generation are not sufficient for that
behavior: durable grant ownership/revocation must be separated from row routing.
Until that scope and retention model is implemented, the coarse gate remains a
fail-closed limitation and parallel unrelated publication is not qualified.
