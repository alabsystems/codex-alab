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
32-character lowercase hex `request` nonce, and a 32-character lowercase hex
`operation`. Retry the exact same request with the same nonce and body. A changed
body needs a new request nonce, including a newer Ready snapshot. Operation is
semantic correlation, not permission to borrow a native lock.

| Action | Additional exact fields | Operation |
| --- | --- | --- |
| `member-open` | none | SDK open nonce |
| `member-preload` | `member=<native32>` | Source-read nonce |
| `member-ready` | `member=<native32> registration=<native32> credential_generation=<positive-u64> digest=<64hex> account=<64hex> user=<64hex> auth_generation=<u64> owner_generation=<u64> source_generation=<u64> login_generation=<u64>` | Matching preload nonce |
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

Retirement with `registration=none` covers an issued member that never obtained
a registration. Native code validates the actual lifecycle; this value never
permits discarding a live credential use. Last-use retirement includes storage,
auth snapshots and derived HTTP/body/WebSocket uses, not just the auth manager.

## Replies

Replies are single status lines bounded to 16 KiB. Success prefixes are
`OK codex-auth member-open`, `member-preload`, `member-ready`,
`member-unavailable`, `member-unrelated`, or `member-retire`, followed by JSON.
The common exact fields are `schema`, `request`, `operation`, `session`, and
`member`. Schema is 1 and correlation fields must match the request. Session is
the existing private source session binding: SID, actual process PID, canonical
process birth and native host epoch.

Open adds no fields. Preload adds `registration` and `source`; `source` is the
complete existing private Source JSON object, with its operation equal to this
preload nonce and the same session. Ready and unavailable add `registration`,
echoing the validated registration. Unrelated adds the actual native-issued
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
