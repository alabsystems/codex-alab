<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Managed proxy startup, durable names and request lifecycle intake

Extend the reviewed prefix from 55 to 61 commits through
`d17887a8d1a7d3e800fb9fb4cb9dd419c147910b`, preserving the prior 55 decisions
unchanged. At the already fetched `0184dfa6a48f0110dd9c6835522492908e5abcf3`,
117 commits remain beyond this prefix. No new upstream source is integrated.

## Managed proxy startup: adopt `c79dfb1d9c`

`NetworkProxy::start_managed` owns the shared build/run sequence. Its explicit
`managed_by_codex = true` matches the existing builder default. Routing, resolved
policy state, credential brokers, launch attribution and native executor context
remain the callers' inputs. Disabled proxy policy still produces an inactive
handle; the existing handle lifetime governs active listeners.

Core retains approval-policy selection: disabled approvals or a final managed
allowlist install no active decider; a managed sandbox without a supplied decider
uses the existing ask fallback. The original supplied decider stays retained in
`StartedNetworkProxy` for later profile changes. The blocked-request observer is
passed independently. Exec-server keeps its existing policy-audit observer,
broker state, proxy-private-IP routing and registration metadata before using the
same startup helper. Startup failures retain their stage and immediate cause.

Require all three `managed_network_proxy_respects_approval_policy_after_profile_change`
cases: approvals enabled, approvals disabled and final managed allowlist. Each
must retain its expected decider count and blocked-request notification. Also
require disabled-policy listener behavior, executor routing/lifetime and current
native network-policy refusal controls. No ALab direct builder/startup caller was
found that needs changing; the touched session fixture hook stays in place.

## Host-confirmed trace edges: adopt `9d12d007b2`

Code Mode records a wait target only after the host returns a live-cell outcome.
Trace replay stops parsing model request arguments to infer that relationship.
The reducer checks the originating thread/turn and rejects duplicate confirmations;
it retains a confirmed runtime handle even when no cell source is available.
Pending cell starts retain their wait links until source arrives. Resume clears
runtime bindings and records its sequence, so a late observation from an earlier
dispatch cannot attach to a recycled runtime handle.

Require `wait_handler_records_only_host_confirmed_targets`, pending-source,
malformed/missing-target, same-handle/different-thread and post-resume tests. Empty
wait lists in old traces mean no recorded host confirmation, not proof of no wait.
The disabled recorder remains inert. This is trace attribution, not execution or
caller-delivery authority; ALab's own request diagnostics stay independent.

## Session model aliases: adopt `a4b69a0ae9`

The new store scopes aliases by session, kind and stable JSON key. SQLite first
checks an existing binding, rechecks under an immediate write transaction,
allocates one ordinal and commits before returning. Primary/unique/check
constraints protect key/ordinal mapping and the first generator state. Snapshot
keeps namespace and ordinal order. The memory store shares a locked map across
clones. Thread deletion removes its alias namespace with the surrounding state
transaction; the memory store removes its corresponding session map.

This commit exposes the store and environment-ID types without adding production
alias allocation calls. Keep model-visible aliases distinct from registered
executor identity and native source authority. ALab's existing item-ID generator
checkpoint is not replaced by this new namespace. Its `FirstInputStore` fixture
inherits the trait's unsupported default; forwarding alias capability becomes
necessary if a later integrated caller relies on it, not merely to compile this
commit. Existing ALab migration numbering ends at 62, so upstream migration 63
does not collide at the reviewed source.

Require `concurrent_assignments_share_a_durable_alias`, cross-session and
cross-kind isolation, reopen persistence, exact snapshot/initialization roundtrip,
delete behavior and memory/SQLite initialization-precondition checks. Fork and
private execution handoff must preserve any activated namespace before issuing
new aliases. Do not infer that store exposure alone implements that lifecycle.

## Prompt image sanitizer: keep `7844815bd7` opt-in

`safe_image_decoder` is under development and false by default. Its opt-in path
decodes supported PNG/GIF/JPEG/WebP images, applies JPEG orientation and supported
8-bit ICC conversion, then writes fresh PNG or JPEG bytes. It bounds encoded
input/output at 79 MiB and decoded pixels at 10,240,000. These bounds do not prove
an unconditional CPU or allocator ceiling. Cache keys include the selected
decoder, preventing a legacy warm entry from skipping sanitization. The legacy
path retains its existing byte/resize behavior.

History preparation now carries complete envelopes. Sanitizer replay can use
the saved original inline payload so repeated JPEG encoding does not drift.
Saved sources remain host history metadata, with a debug formatter exposing only
length. Replay uses the inline store, leaves file references unchanged, disables
resize notices and does not upload or rewrite saved history. ALab's private
`MemoryItem` already clones and serializes `CodexHarnessMetadata`; its literals
use defaults, and no ALab caller of the changed image preparation helpers was
found. Require an explicit nonempty replay-source private-handoff roundtrip and
provider-wire exclusion test before integration.

Also require `replay_regenerates_the_recorded_jpeg_from_its_original`,
`legacy_cache_entry_cannot_bypass_sanitization`, metadata/orientation/color cases,
input/output/pixel boundaries, malformed images and MCP/`view_image` validation.
Keep image analytics and source/account custody under their existing defaults.
Use upstream's pinned image/png/moxcms dependencies and retain removed Bazel paths.

## Cancelled inventory and saved side titles

Adopt `61c106ed66`. The Cloud worker skips a queued `ThreadList` with a closed
receiver and drops an in-flight list future when its receiver closes. This check
is limited to stateless listing, whose handler reads its bound client and adjusts
the returned history representation without mutating loaded conversation state. Other operations retain
their existing completion/commit rules. Require
`dropping_an_in_flight_list_unblocks_the_native_worker`, queued cancellation and
source-retirement/network-policy controls. Cancellation must continue to drop the
original admitted transport future; no fresh credential or connection is created
by this optimization.

Adopt `d17887a8d1`. A successfully admitted first direct user message in a newly
saved side chat may replace its inherited name. Text extraction uses only that
direct input. Image-only input consumes the first-input eligibility without
generating from inherited text. Failed submission retains eligibility. The
automatic writer checks the originating thread's expected name, while manual
renames retain their semaphore/cancellation precedence. Server-owned Cloud title
settings still skip TUI generation. Temporary side chats are not added to the
saved-fork map.

Require `saved_side_chat_title_uses_only_its_first_direct_input`, image-only,
failed submission, thread switch, inherited-name replacement, parent-name
preservation, manual rename and late cancelled title controls. Preserve ALab's
native account routing, queue, checkpoint, title defaults and parked-backend hooks
in the touched TUI files. No ALab caller of the changed title helper or event was
found that needs a new argument.

## Validation boundary

Require exact 61-commit intake validation, unchanged prior decision objects,
direct active supersession and no duplicate coverage. The next commit,
`c33002259e`, remains unreviewed and must not pass this record's coverage. Runtime
tests named above are integration requirements, not claimed executions. The
certified application gate and existing procedural-macro boundary remain
separate; no compiler rule, installed artifact or feature default is changed.

Observed validation: the maintained intake validator accepts the exact 61-commit
record. The prior 55 decision objects compare equal to their committed originals;
all covered commits have one active declaration and direct, valid historical
supersession. Extending the requested range to
`c33002259e28db9e87807cedebfb25299d95a2c6` is rejected as incomplete.
`git diff --check` passed. No workflow implementation changed since the preceding
58-test Python run; this extension ran intake/source checks only, with no Rust
test, application build or runtime qualification claim.
