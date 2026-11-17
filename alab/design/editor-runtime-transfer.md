<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Editor state across a live update

The complete native draft must cross the exec. Plain text recovery loses atomic
attachment IDs, pending paste payloads, mention bindings, Vim command recording,
Replace recovery, undo/redo, registers, selection and cursor state. The displayed
draft and every conversation's stashed draft belong to this requirement.

The native draft/stash checkpoint is now carried in joined runtime format 2.
The real source capture supplies it with input/terminal data; the receiver stages
and installs it at primary attachment before activation. Startup keeps input in
the shared broker and preserves the retained screen. The restored editor must
have a native layout before the real SDK activation runs; activation success
releases input and wakes the TUI, then settlement runs under an owned retry task.
This wiring is still unqualified for signed/native execution. Execution discovery
stays unavailable and the empty-composer restriction remains. Complete busy UI,
worker transfer and recovery are not yet implemented or proved.

Each projection lives in ALab's glue, compiled inside the native type's module
through a marked hook. Serde remote derives describe the exact native fields
without adding derives or moving state machines in upstream files. The source
and destination continue to use their real editing logic. These bytes confer no
Core, SDK, terminal, clipboard or child-process ownership.

Register generations require graph preservation as well as value codecs. The
composer keys rich register payloads by Weak pointer identity, while live and
recorded yank actions retain the corresponding Arc element generations. Ordinary
Serde Arc encoding duplicates values and loses that relationship. The enclosing
editor checkpoint must assign stable graph IDs, restore shared generations once,
and reconnect weak payload keys to those same objects. Equal element values from
distinct generations must stay distinct. Native register leaf codecs now use the
scoped generation adapter. Standalone leaf encoding still has no enclosing graph;
a whole composer must use scoped encoding and include every rich payload key and
strong root, or rich payloads can be lost despite a visible text round trip.

Remaining capture and restore work must satisfy these boundaries:

- Capture the full textarea, composer draft, rich attachments, undo/redo, pending
  slash history and all per-thread stash slots together at the actual TUI decision.
  Include accepted but unflushed input and preserve its paste classification.
- Carry monotonic editing deadlines through a clock conversion, rather than
  serializing an Instant's private representation or restarting elapsed timers.
  Recompute derived layout caches against the destination viewport and install
  its freshly resolved keymaps before accepting the next key.
- Preserve native editor and clipboard workers through actual ownership transfer
  or completed source work. A serialized PID, Arc or pending callback is not an
  owner. Never replay a completed external clipboard effect as new work.
- Validate cursor/range boundaries, element identities and references, history
  budgets, thread routing and all schemas before replacing destination state.
  Restore all slots atomically before input/event replay can edit them. A stash
  belonging to another thread cannot become the displayed conversation's draft.
- Join editor capture with the existing terminal/input, Core and goal commitment.
  A refusal leaves the source unchanged; a committed capture never restarts old
  producers. Retain recovery ownership until the native destination settles.
- Bump the joined full-runtime format when connecting this new required state.
  The target capability probe and native bridge must agree with that contract;
  an older target's capability is not permission to discard editor state.

Component tests compare real native editing behavior after restoring pending
find/operators and Replace recovery/dot replay. Completion additionally requires
full composer/stash round trips with rich elements and an actual CLI transition
while a draft is present, including refusal and recovery cases. Passing a codec
test alone is not evidence that the live-update path preserves a draft.

The owned `upgrade_editor_graph` table now preserves register generation identity
independently of field ordering. Source capture pins each strong generation once;
weak payload keys resolve only after all roots are collected. A live weak key
without a captured root refuses capture rather than silently losing rich content.
Destination references share one allocation per table entry; equal-valued entries
remain distinct. Unknown IDs refuse restoration. The enclosing checkpoint still
must supply node/byte budgets and walk all native strong roots and weak payload
keys. The scoped decoder releases its temporary table after validating that every
generation has a strong root in the restored body.
This component underlies the joined draft checkpoint; other UI owners still need capture.

Native register elements, copy/paste events, recorded yank snapshots and kill
buffer snapshots now use scoped generation adapters. The checkpoint encoder
performs two immutable passes so weak payload fields can precede their roots.
Standalone leaf serialization remains an explicit inline representation; graph
restoration rejects inline values, unknown IDs and references outside their scope.
The table is decoded before the body, with byte and generation limits. Scoped
capture is synchronous and rejects nesting; error cleanup releases its pinned
roots. This does not yet capture any whole draft or stash, nor any worker owner.

The native textarea projection carries text, atomic element IDs/ranges and the
next ID, cursor/preferred column, mouse selection/click sequence, kill buffer,
register events and generations, Vim mode/pending/recorded transactions/search,
and the prefix insertion revision. It rejects invalid UTF-8 cursor/element ranges,
duplicate or overlapping element identities, and cursors inside atomic elements.
Layout rectangles and wrapping/hyperlink caches start empty. Fresh default keymaps
are temporary derived state: the destination must apply its resolved configuration
to both the draft and any active search query before input resumes. Native scroll
state has its own projection and must be checked against the reconstructed layout.

Paste-burst fields preserve the held first character, accepted buffer, detection
counter, active flag and all three timing fields. Editor Instant conversion uses
Rust's actual native monotonic domain (UPTIME_RAW on Apple, MONOTONIC otherwise),
confirmed against the selected Trust standard library source. A public-API clock
sample is bracketed to at most 100 microseconds; at most three attempts are made
before refusing capture. No opaque Instant layout or wall time is serialized.
Source and destination each contribute at most 50 microseconds of conversion
uncertainty. Transport must separately authenticate the same host/boot. Expired
paste windows stay expired rather than acquiring fresh timeouts after exec.

A mouse selection with pending clipboard copy or a live primary clipboard owner
currently refuses data capture without mutating the owner. Such a refusal is not
clipboard handoff support. Complete the real work or implement its actual owner
transfer before lifting that restriction. The draft/stash checkpoint joins these codecs into runtime format 2; this does not
yet represent every visible UI owner or transferred worker.

The native composer projections retain DraftState's input flags, pending paste
payloads and burst data, skill-completion space, mention ID bindings and recent
submission bindings; AttachmentState retains local paths/placeholders, remote
URLs and keyboard selection. ComposerDraft undo/redo entries retain rich metadata
and cursor positions. History decoding enforces the native combined step/byte and
pending-entry limits instead of trimming data during restoration. SavedPrompt
includes its complete native draft, attachments, history, pending slash history
and sparkle draft state. These leaf traits feed the joined draft/stash data
checkpoint below; runtime UI integration remains unfinished.

VimRegister payload maps now resolve their native weak generation keys in the
same graph as the textarea's strong roots and recorded actions. Dead generations
are omitted without mutating source state, matching native sync's pruning rule.
A live missing root or a rich payload ID absent from that generation refuses the
checkpoint. The ordered generation-entry list is preserved, including native
first-match behavior; rich mention targets, image paths and paste blobs are data,
not re-resolved references or new clipboard/file effects. The joined UI owner
must capture all of those roots and maps together before committing transfer.

The joined draft checkpoint now captures the visible native draft data and the
entire per-thread stash map in one generation scope. Capture holds the stash mutex
through both immutable encoding passes and borrows the current pane on its UI
thread. Its thread ID comes from the real pane, not from a caller-supplied value.
That ID routes the displayed editor; it is not a native SDK/backend primary grant.
Integration must separately join displayed-thread routing to the Core forest.

Decoding stages all data before installation, rejects duplicate thread slots and
wrong displayed-thread routing, and validates every native nested decoder before
returning a prepared value. Installation refuses a different destination thread
or an already populated destination stash map before changing either owner. It
configures keymaps recursively in query editors and stashed drafts, then moves
visible data and all slots together under the stash lock without an await.
This data operation does not provide a constructor/input barrier, layout-ready
witness, clipboard owner or SDK continuation. The caller must hold those actual
owners and prevent event replay until complete UI restoration is ready.

The source and receiver now call these entry points, and their temporary dead-code
expectations have been removed. Runtime format 2 requires an editor namespace;
Core-only fixtures explicitly label synthetic editor data rather than claiming UI
qualification. Unknown runtime frame versions fail before ordinary startup.
Native execution stays unavailable and the empty-composer guard stays in place.
Complete visible history, search/popup/visual owners and worker ownership remain
necessary for busy operation.

The receiver must not install this checkpoint late in ordinary startup.
`StartupDraftPump` already consumes input while session startup runs, and its
`take_draft`/`into_draft` path produces ComposerDraftSnapshot rather than the full
native draft transaction/history/register state. Replaying a key there before
restoring a pending Vim operator can turn an editing command into literal text;
passing the restored editor through that snapshot would flatten it again. Wire
restoration before replay, retaining the native owner through the startup-to-app
handoff (or defer provisional input handling under actual constructor custody).
Only release events after complete UI state, configured keymaps and layout are
ready. A data install function is not evidence that this ordering holds.

Before publishing the first live-runtime writer, replace exact writer-version
matching in capability discovery with a stable negotiated reader contract. A new
target must be able to accept the source's previously published format, or use a
qualified migration path; merely bumping the writer and probe integers would stop
future live updates. Required state must never be silently defaulted away to make
an incompatible target pass. The current editor data format is still internal,
and no full-runtime release or compatibility qualification is claimed here.

Capability discovery now separates the target's current writer from its explicit
sorted reader set, with a stable extensions object for future optional features.
A future writer may accept a source's existing format without claiming it writes
that format itself. Reader declarations do not add a decoder or confer admission:
every published reader still needs an actual compatible decoder/migration and
cross-version qualification. The image/PID/device/inode/product and custody checks
remain independent and mandatory. No legacy frame decoder is advertised here.

The real receiver records attachment without activating the forest. The App's
completed native render checks the restored textarea/query layout, then starts
one owned activation task using its retained native request handle. Only a
successful activation opens input; failure reports fatal startup failure while
native recovery custody remains unsettled. A draft or a layout flag cannot make
an SDK handle. Settlement follows activation and remains a separate reconciliation.

Remaining integration qualification must include every startup input owner and
destructive quarantine/drain path, fresh-run interactive prompts, queued control
keys versus automatic goal resumption, and distinct SDK-primary/displayed-thread
routing. The current strict routing refuses an unsupported mismatch. Actual
signed activation/settlement and recovery at every failure boundary remain
unproven. The old local protocol-3/writer-1 candidate predates this format and is
not a current compatible target; rebuild and qualify a new candidate.

While native editor restoration is pending, protected-screen input discard now
refuses with WouldBlock before any pause/resume, broker replacement, parsed-event
clear or OS drain. The shared input facade enforces the same rule, including its
legacy-reader branch. It never certifies that a protected boundary was discarded
when constructor-custody keys are still owned by the restored editor. After actual
activation, the existing protected-screen policy applies again. This guard is no
SDK grant and does not solve the later input-frontier/automatic-producer ordering
requirement described above.

For focused TUI tests, use the same `RUST_MIN_STACK=8388608` test environment as
upstream's `justfile` and the candidate/pristine library gate in
`alab/unit-tests.sh`. Default-stack libtest aborts are not a reason to rewrite the
native startup or dispatch behavior. This setting belongs to test execution;
it supplies no evidence about native execution admission or transfer semantics.
