<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native destination handoff

This is the agreed interface for the destination side of account rotation. It
does not enable SourceV1 or install ProcessHost. Activation still requires the
complete reader/resource census, retained source seal, authenticated grant
lineage, and a real two-subscription continuation test.

## Authority and ordering

An assignment operation identifies work; its identifier does not authorize a
credential read. Native code issues an opaque destination context only from the
actual retained Installed entry, with its source seal, exact authenticated caller,
native instance, chosen destination and assignment generation. The SDK context
is redacted and has no Clone implementation. It is distinct from an old source
Borrow or UseReceipt.

An exact context request replay returns the same context while that role remains
valid. It cannot select another destination, create a new grant, or reacquire a
privileged role after Applied. No context response invents a credential path or
the SDK's four manager generations.

The SDK retains each constructor/preload attempt before its first write. Native
code counts destination allocation and pending preload before worker dispatch
or credential I/O. Lost replies reconcile the exact retained attempts. A target
preload reopens the current authenticated destination owner and checks the
actual Book/Slot/policy role; generic source/bootstrap fallback is forbidden.

Destination Apps, model and MCP initialization can perform authenticated HTTP
before Applied. A native target member therefore admits Begin while AwaitingAck
only through its actual retained Installed role and current Ready registration.
Those destination uses are counted separately from the frozen old source drain.
Contextual destination refresh/preload derives permission from an actual
destination SourceUse. An old source capability or generic root activity cannot
grant it. PolicyUnavailable does not constitute Ready or acknowledgment.

Applied consumes the special initialization role and transitions the actual
destination members to ordinary destination use. Existing destination body,
WebSocket and other resource leases remain valid across that transition.

## Private wire contract

All requests use the existing authenticated `codex-auth` transport, schema 1,
request prefix/counter and session correlation. Identifiers below are 32 lower
hex digits except context, target_ref and digest, which are 64. Reject duplicate
or unknown fields and frames over 16 KiB. Existing waiting replies contain only
their correlated common fields; they confer no authority.

| Action | Action-specific request fields | Successful result |
| --- | --- | --- |
| `member-target-context` | `operation=ASSIGNMENT` | common fields with member null, context, positive assignment_generation, target_ref |
| `member-open-target` | `operation=OPEN assignment=ASSIGNMENT context=CONTEXT` | common fields with the actual member |
| `member-preload-target` | `operation=PRELOAD member=MEMBER assignment=ASSIGNMENT context=CONTEXT` | common fields, registration, complete private Source for PRELOAD/session |
| `member-ready-target` | `operation=PRELOAD member=MEMBER assignment=ASSIGNMENT context=CONTEXT registration=REG credential_generation=C digest=D account=A user=U auth_generation=G owner_generation=O source_generation=S login_generation=L` | common fields and the same registration |
| `member-reader-ready-target` | the Ready fields above, omitting the four SDK manager generations | common fields and the same registration |
| `member-unavailable-target` | the exact pending target/preload correlation | unavailable result with registration; never Ready |

The ordinary Begin/End/Close syntax stays unchanged. The native-issued member
purpose supplies the additional target-role check. Ready verifies the actual
captured Source, digest, credential generation and, for managers, their actual
current four generations. Reader Ready has a distinct role and fabricates none.

## Cleanup and unresolved edge

Release-only End, Close and retirement remain valid independently of destination
context expiry. Cleanup grants no new credential or work authority.

An accepted OPEN cancelled before any preload cannot invent a PRELOAD identifier.
The proposed additional release-only `member-retire-open-owned` action carries
`operation=CLEANUP member=MEMBER|none open_operation=OPEN`. Native proof must come
from the exact retained open attempt and caller. `none` is lost-reply correlation,
not evidence that no allocation exists. A missing record or a high-water counter
alone cannot return Closed. This action remains pending backend record and
terminal replay design; it is not yet part of the implemented protocol.

## Migration admission

Ordinary turn suspension waits only its own use family and actual native End
acknowledgments. Process migration instead owns a retained census of every
admitted reader/manager through effects, source drain, destination initialization
and exact acknowledgment. A preflight check that immediately drops its census
does not provide this authority. Lost owners stay Unknown until explicit
authenticated retirement; Weak loss and age cannot compact them away.
