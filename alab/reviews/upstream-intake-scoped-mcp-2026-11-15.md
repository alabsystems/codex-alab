<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Scoped MCP, realtime defaults and transport intake

A successful maintained Agh fetch observed upstream main at
`0184dfa6a48f0110dd9c6835522492908e5abcf3`. It is 178 commits beyond integrated
`ce3031018ef130e653ed4d9c5bb971652de97161`. Extend the existing reviewed prefix
from 46 to 55 commits through `69231e119bfca0b14453515e97d99688ce1c47c9`;
123 commits remain after that prefix. Preserve the existing 46 decision objects
unchanged and supersede their batch. This is source review, not integration or
qualification.

## Scoped MCP registrations

Adopt `6fcfa75a9f` with ALab fixture adaptations at integration. Status,
capabilities, tool catalogs, errors, resources and auth status use `McpServerKey`
rather than collapsing same-named registrations. An explicit environment selects
that key; omission retains existing default-registration resolution. The runtime
captures one publication before resolving the key and using its connections.
Unknown tool/resource targets fail, and tool filters run before startup and again
before execution. Selected status preserves its expected-registration check.
Existing name-only startup reporting still selects the default registration;
the new environment field does not make every registration an eager startup.

Threadless status rejects an environment selector without server and thread.
Connector targets and origin-call reads reject scoped `codex_apps`; they retain
their existing unscoped attribution. Elicitation and tool-history records carry
registration scope, which is not execution-environment or account authority.
OAuth resolves the selected effective registration. The TUI keys current logins
by login ID plus requested name/environment, so retrying one target leaves a
peer environment's login alive. Startup state and diagnostics use the same
name/environment pair. The ALab turn reporter only checks whether startup state
is present and therefore does not consume the changed key type.

Required ALab fixture changes, only once the upstream fields exist:

- Add `environment_id: None` to `ListMcpServerStatusParams` in
  `alab-core/glue/mcp_status_owner_tests.rs`.
- Add it to the three `McpServerStatusUpdatedNotification` literals in
  `alab-tui/glue/turns_session_tests.rs` and
  `alab-tui/glue/goal_runtime_status_widget_tests.rs`.

Preserve the selected-auth-manager, explicit-auth-scope, source transport,
thread/turn status, parked backend and account hooks in the touched upstream
files. Require `mcp_server_status_keeps_same_named_environment_inventory_separate`,
the status auth-owner fixture, unknown-scope/tool-filter controls, scoped
elicitation/history coverage, OAuth retry/peer-login tests, and ALab startup-turn
and goal-status tests. Retain `apps = false` and the SourceV1 hold.

`454bf339f0` supplies the new selector field as `None` in statusline inventory
and resource requests and updates exact wire assertions. Adopt it with the scoped
MCP change; no environment is guessed from a statusline display label.

`c899e21cda` makes runtime MCP projection accept an explicit contribution context.
Adopt its originator, selected-environment and disabled-plugin restrictions and
the host-only constructor. Host projection has thread initialization identity
without a live extension store or discovery capability. Existing threadless
callers explicitly pass global context. No ALab-owned MCP runtime-config caller
was found. Require `start_thread_seeds_extension_data_for_mcp_and_lifecycle_contributors`
and the current auth-owner and account-isolation controls.

## Realtime defaults

Keep `b05266d61b` behind explicit realtime use. The V3 default model becomes
`gpt-live-1.1-codex`. Unset voice remains optional until the wire adapter selects
the provider default: API V3 uses Marin and ChatGPT uses Cove. Explicit voices
remain authoritative. New V3 call startup reloads effective voice configuration;
`Some(None)` clears a retained thread preference instead of inheriting it.
Existing-call attachment does not reload or initialize the session. The picker
uses the V3 catalog; saving or starting no longer requires a catalog request.

Require the API/ChatGPT default-and-override tests, `webrtc_v3_uses_current_voice_setting`
including cleared preference, existing-call noninitialization, remote picker and
catalog-failure cases. Retain source/destination admission, local AuthManager
custody, cancellation, SDP redaction and the matching-attempt TUI checks from the
earlier realtime intake. This model spelling is not evidence of entitlement.

## HTTP compression and session windows

Adopt `16280e19b1`'s gzip/zstd reqwest features and its exact upstream dependency
resolution. Keep Bazel paths removed and never hand-edit upstream lock entries.
The new code enables response decoding; it does not introduce a request source,
credential lookup, telemetry endpoint or retry owner. ALab's rejection capture
and explicit response limits count chunks after reqwest decoding. Before
integration, require real gzip and zstd success/rejection tests, decoded expansion
past both limits, malformed/truncated entities, cancellation and policy revocation,
with original status, correlation IDs and header-receipt advice retained. Existing
plain and WebSocket upgrade fixtures remain required. This review does not claim
these compression-specific regressions already exist or passed.

Adopt `72be450854`: acquire session state, read window IDs through the existing
state helper, and release the lock before assembling response metadata. The fork
feature predicate remains equivalent. Async-compaction fixtures now own their
explicit test environment. Preserve ALab checkpoint and input hooks; require
fork-window/metadata and async-compaction snapshot, pending-work and exec-catalog
tests under the native test environment.

## Filesystem helper and marketplace settings

Adopt `5045589550`'s filesystem-helper-specific sandbox transform. It removes the
redundant direct-spawn API, materializes explicit workspace grants, and refuses
Windows token helper sandboxes on Unix with `SandboxUnavailable`. Windows wrapper
preparation remains a helper operation and preserves proxy settings. Permission
entry telemetry is sampled before adding helper runtime read access. No ALab
caller of the removed API was found. Require nonnative-cwd and Unix refusal tests,
Windows helper executable-only reads and explicit/empty write roots, and real
helper read/write isolation. A macOS run cannot establish Windows execution.

Keep `69231e119b`'s marketplace auto-update setting opt-in. The default is false;
this change declares and preserves the choice, without adding a new refresh
caller. Re-adding the same source retains the setting; replacing its type or
source drops it. Local paths compare after normalization. Converting an inline
table retains other marketplace entries. Require same-source/replacement,
equivalent-local-path and sibling-entry edit tests, and retain the existing
plugin catalog/background-network defaults. No ALab `MarketplaceConfig` literal
was found that needs the added field.

## Admission and delivery limits

Validate exact 55-commit coverage, unchanged prior decisions, a single active
record per covered commit, and rejection of the next unreviewed target. Application
integration still requires feature evidence, hook retention, the complete native
gate and a certified application build. The retained unaudited in-process
procedural-macro refusal is unchanged; do not weaken it or substitute unverified
test execution. No upstream source is merged and no installed artifact is changed
by this review.

Observed validation: the maintained intake validator accepts all 55 decisions.
An independent inventory of active and superseded records finds exactly one
active declaration per covered commit, with historical pointers going directly
to the active replacement. The previous 46 decision objects compare equal to
their committed originals. Extending the requested range by the next upstream
commit, `c79dfb1d9cc531e0b091b802edd44665edee9b81`, is rejected as incomplete.
All 58 upstream workflow Python tests passed in 39.652 seconds with the managed
Python 3.14 interpreter; `git diff --check` passed. No Rust test or application
build was run for these review artifacts.
