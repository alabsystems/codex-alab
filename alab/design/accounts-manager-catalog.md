<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Local manager catalog and source retirement

A Codex process can share one AuthManager across several foreground and background
threads. Closing one thread's input does not establish a process boundary. The local
catalog therefore admits actual manager construction before credential reads and
retains every manager Domain and session resource mailbox associated with it.

This catalog is local exclusion and lifetime evidence. It is not a native SourceSeal,
a prepared target, a provider grant, or permission to install another account. The
production native ProcessHost and SourceV1 marker remain disabled until the native
continuations and all reader roles are joined and qualified.

## Ownership

A Domain retains its catalog record; the record's Domain reference is weak. The
registry retains stable records and Unknown tombstones. Records own only the
admission gate, which has no census backedge. A record retains type-erased resource
mailboxes and an actual auth-state watch, never a recursive Session object. Losing Weak<AuthManager>
does not retire the Domain, a response body, an outstanding native use, or a mailbox.

Construction is Loading, Deferred, Ready, or Unknown. A sealed catalog defers new
constructors before their readers run; cached getters do not expose their cache.
Async initialization must obtain admission before settling a deferred constructor.
Cancellation during an admitted load leaves Unknown while its owned Domain remains
live. None is not a NoNativeReader attestation.

A seal closes new root activity atomically with constructor admission. Existing
admitted work can derive child work while it drains. Retirement closes further
construction/work admission after actual old work/start counts reach zero. The
registry retains closure when a seal or caller is dropped. Only validated pre-effect
cancellation reopens it.

## Resource boundary

HTTP permits compose the native source use with a local manager Resource activity.
Actual requests, bodies and WebSockets retain that activity through their last use;
cached factories retain no activity. Source watches fence old factories on owner or
login changes. Unproven sources additionally require the same source counter. A real
managed Native NetworkScope permits ordinary same-grant credential refresh without
retiring the entire account scope.

Every Session sharing a manager registers a separate mailbox. Resource retirement
uses sealed evidence minted from the actual Domain/catalog epoch and auth-state
watch. Runtime shutdown marks a participant stopped only after producer, realtime,
MCP and model transport cleanup. EOF or a vanished Session reference is insufficient.
The final old-domain predicate also waits for owned native End acknowledgements.

Ordinary typed turn suspension observes only the exact removed turn's native-use
family. Its cleanup observer owns no activity/use lease, covers children and late
admitted uses in that same family, and awaits actual End acknowledgements. It does
not close process factories or join unrelated managers/sessions. The unfinished turn
and native cold-succession authority remain separate.

A process/account migration must obtain its distinct actual source admission and
retain the exact preflight census in an owned guard across every task/resource
effect. The current mark_effects check alone does not retain that census and is
not sufficient for migration admission. A vanished Domain
or manager is Unknown and refuses pre-effect admission; it is never omitted or
classified as NoNativeReader. The caller can explicitly cancel a failed pre-effect
seal. Native process/account suspension remains unavailable without its actual
reservation, purpose and owned lifecycle continuation. Unknown tombstones also
need explicit authenticated final-use retirement/compaction; weak loss never
permits eviction.

## Required joins

Native healthy assignment still needs the real prepared/reserved census, kernel
SourceSeal, independently re-admitted installed target and explicit B factory role.
The B role cannot be derived from an A Activity, an A use capability, an empty count,
or an AssignmentIntent. B initialization must use a separate source epoch so live B
resources cannot become part of the old-A drain.

Standalone credential readers need their own ReaderLoaded role, snapshot/storage
and derived-use lifetimes, and owned retirement acknowledgements. They cannot use
fake manager counters. External credential operations and cap-free same-member
repair require their actual owned admission/cleanup paths. These gaps are activation
blockers, not permission to classify missing state as unrelated or enable SourceV1.

## Registered direct readers

Registered direct readers have their own catalog role and Domain. Their source
observation is the actual reader-ready member incarnation, registration, preload
operation and credential revision; it contains no manager counters. Native API
selection remains the separately acknowledged unrelated-source kind. An owned
loader retains factory admission and an admitted parent activity after a caller
stops waiting.

Retirement observations retain the exact cleanup body and native member proof,
not storage, Domain or activity. A reader record becomes Retired only after the
same incarnation's native Closed acknowledgment. A lost Domain stays Unknown
until that acknowledgment; only explicit Retired records can be compacted. After
all old reader body/use leases and End acknowledgments drain, catalog retirement
revokes future extraction and cached-factory admission before awaiting Closed.

Credential snapshots retain their immutable source Epoch and policy with the
actual cache document. Account replacement closes the old Epoch; ordinary verified
same-grant refresh preserves it. Source selection precedes credential material and
conjoins the caller's concrete HTTP/WebSocket destination restrictions. Body and
socket ownership retains the actual source use. Known native material needs a
concrete destination owner for header export; an owned refresh action is a separate
operation and grants no raw-header permission. Only an explicitly captured,
nonactivated legacy snapshot retains legacy introspection behavior.

These reader roles do not install ProcessHost or advertise SourceV1. Remaining
joins include direct-reader credential-policy snapshots without manager counters,
non-native/unmanaged low-level reader coverage, extending and retaining the actual
migration census, target-authorized B factory/source-epoch propagation and all
Session mailboxes, and full provider/realm policy previews. Native retirement,
source seal, target admission and resource ACK remain independent real proofs.
