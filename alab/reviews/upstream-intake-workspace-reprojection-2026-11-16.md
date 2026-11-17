<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Cloud status, stable workspace and deferred MCP reprojection

The maintained `alab/github.py` fetch, using managed Python 3.14 and Agh,
confirms upstream main at `98e1584a082234ae716a3cc69d4c6f2d3631cf2d`.
Before advancing the isolated intake worktree, all six prior review/report files
were byte-identical to main and the working tree was clean. The working base is
ALab main `afa1d02db7e62b003d746fd39761b96dc0f599f9`.

Extend the exact reviewed prefix from 66 to 76 commits through
`4413deac572eb5d4cdb49295794cbe30f8622935`, retaining every earlier decision
unchanged. There are 179 fetched upstream commits beyond the integrated
`ce3031018ef130e653ed4d9c5bb971652de97161` frontier, of which 103 remain beyond
this review. No upstream code is integrated here. Qualification of the separate
61-commit sync candidate does not qualify these ten additional decisions.

## Accepted Cloud model status: adapt `f8580db472`

An accepted Cloud selection now supplies the status line, terminal title and
`/status` model and reasoning display. A staged selection does not update those
surfaces before native admission; rejected or unknown admission does not become
an accepted selection. The session's serving model, effort and speed remain
separate. Reasoning-only `/model` commands resolve against the accepted selection.
The common session-only confirmation does not persist a new default or change
native source/destination admission.

ALab needs a status-specific adaptation. Its hook in
`chatwidget/status_controls.rs` supplies `alab_model_with_service_tier()` to the
status card. That helper in `alab-tui/glue/interface_status.rs` still combines
`model_display_name()` with `processing_tier_display_name()`. After this upstream
change those can describe different models: the first is serving, while the
second uses the accepted selection. Use upstream's
`status_model_display_name()` for status formatting while retaining ALab's
supported Fast/Ultrafast/custom labels and independent feature gates. Preserve
the saved model/reasoning pairing of `alab_session_model_with_service_tier()`
and deliberately assess the cleared-session header, another caller of the
existing helper; do not globally replace serving metadata with a future choice.

The optional interface's suppression of the word `default` also constructs its
prefix from `model_display_name()`. Update that status-only comparison to the
accepted status name, so a different accepted model with default effort does not
leave a spurious `default` suffix. This remains presentation data, never model,
permission, credential or service-tier execution authority.

Require upstream `cloud_model_selection_waits_for_native_admission` with the
ALab layer active, including started/steered acceptance, local refusal, unknown
outcome, unchanged serving metadata and session-metadata reset. Add actual
status-card, native status-line/title and optional-interface assertions with
different serving/accepted display names, default and explicit effort, and
supported/unsupported tiers. Run with the optional interface both on and off,
and retain `supported_speed_tiers_reach_model_status_title_and_header_surfaces`
and `startup_header_keeps_saved_model_and_reasoning_when_plan_model_differs`.

## Canonical proxy policy selectors: adopt `a60197d6a0`

The public allow/deny/review comparison keys share the matcher's existing syntax
validation. `?*.example.com` and `*.example.com` have the same subdomain-only
comparison identity; `**.example.com` still includes the apex. Deny rules still
reject global wildcards. Unscoped IPv6 restrictions compare canonical addresses,
whereas grants retain their spelling. Scoped IPv6 identity stays scoped.

A wildcard stripped from a local allow rule's port is retained in its comparison
key, so `[::1]:*` cannot become an explicit local grant merely through key
normalization. The runtime's extracted local-literal predicate preserves the
existing localhost/non-public-IP classification without resolving DNS. The
matcher continues byte-oriented wildcard semantics without IDNA conversion.
No ALab caller of the new canonicalization functions was found. Comparison
normalization grants no new destination authority or credential access.

Require `comparison_keys_preserve_differences_between_grants_and_restrictions`,
syntax refusal and bracket/scope normalization tests, and the independent
dynamic-program matcher comparison. Include global-wildcard deny refusal,
explicit versus wildcard-port local grants, IPv4-mapped restrictions and
private-address DNS rebinding refusals under the ordinary proxy runtime tests.
Preserve ALab's provider-source and native destination admission around proxy use.

## Workspace default and view guards: adopt `4ccf87254a`, `2bad26e03f`

Accept the upstream stable `tui_workspace = true` default rather than add an
ALab-only legacy default. Neither ALab defaults file currently overrides it.
This is an actual default change: `LocalSettings::workspace_enabled()` requires
the owned transcript, and `App::side_retention_enabled()` also requires an owned
screen, so native-scrollback and alternate-screen restrictions still take
precedence. Explicit feature disable must continue to work. Existing side-chat
retention, sidebar focus and workspace keymap paths become the ordinary owned
screen behavior; enabling the flag alone does not start a new model conversation.

Upstream deliberately disables the feature in the legacy inline-HUD test and
updates the empty quick-dial snapshot. Do not flatten both screen modes into one
fixture expectation. Before integration require default/explicit-disable and
alternate-screen controls, `global_chord_keeps_hints_and_completes_before_deadline`,
existing workspace focus/printable-remap/offline routing tests, and side-chat
retention with closed/archived children. Exercise ALab's focused goal/account
rows, constrained draft cursor and account click target, prompt stash, selected
identity, parked backend turn ownership, progress, pet overlay cleanup and resize
in both workspace states. Retained conversations must not introduce duplicate
pollers or lose the native shutdown path when their pane closes.

`is_read_only_view()` currently delegates exactly to `is_external_writer_view()`;
the second commit replaces guards in input, paste, external editor, transcript,
tips, navigation and session restoration without adding a permission rule. Keep
ALab's surrounding interface, goal interrupt, FIFO, session and pet hooks.
Require `external_writer_snapshot_freezes_active_command_and_mcp_rows`,
`external_writer_view_preserves_draft_from_keys_and_paste` and external-writer
quit/navigation/reconnect coverage. A cached read-only view must not become a
local editable session or revive a paused backend.

## Guardian write efficiency: adopt `60fb26f6ba`

Existing destinations may omit their descriptions, and an identical returned
description no longer writes the channel again. New destinations still require
descriptions, and every description update must name a routed destination. The
complete routing validation precedes writes. Stored descriptions remain untrusted
discovery text; original evidence, owner binding, bounded request construction,
cancellation/deadline checks and acknowledgement after all writes remain intact.

Require all `routes_captured_originals_and_updates_descriptions` cases: successful
and failed writes, missing new descriptions and unrelated description updates.
Its later batches check omitted and repeated descriptions through actual board
call counts (catalog read plus evidence append) and verify retained descriptions
and message counts. Invalid model output must roll back the reviewer session,
leave the cursor unchanged and perform no board write. A failed pass still stops
its instance; this optimization does not add replay or execution authority.

## Removed Bazel-only upgrade: supersede `76a8ca3914`

All 17 changed paths are already covered by `alab/removed-paths`: Bazel modules,
lock, rule/build files and patches. The change updates rules_rs to 0.2.3 and
platforms to 1.1.0, changes crate labels and removes obsolete Bazel-only patches
and extra dependencies. It changes no Cargo manifest, Cargo lock or application
source. Keep those whole paths absent after merge. Do not reactivate a Bazel,
GCS, compiler or release lane, and do not run removed Bazel tests as ALab
qualification. Revisit only if the committed whole-path removal policy changes.

## Deferred MCP projection: adopt `ae12a9d645`

The new `CodexThread` API sets one atomic next-turn request. Idle reconciliation
and prewarm do not claim it. A regular task claims the request after publishing
`TurnStarted`, then requests ordinary reprojection before tools are bound.
Repeated requests coalesce; a request racing after the atomic claim remains for
a later turn. Reprojection reuses connections and does not invalidate resource
caches. Failed or cancelled unpublished refreshes retain their existing dirty
causes through the invalidation guard.

Preserve ALab's `alab_assignment::prewarm` hook, which admits old-source work
without waiting and cancels it before retirement joins. Neither a deferred flag
nor a dirty cause can restore old account authority. Keep account retirement,
router replacement and fresh source/destination publication checks intact. No
existing ALab caller or literal needs the new `McpRefresh` field. Native handoff
must still deliver a current destination-owned MCP projection: explicitly test a
deferred request across capture/restore and establish that fresh reconstruction
satisfies it before tool binding, or carry the scheduling demand as data if that
path can otherwise skip it. Do not transplant connections or authorization.

Require `next_turn_reprojection_is_not_claimed_by_prewarm`,
`deferred_mcp_reprojection_waits_for_turn_start_and_binds_refreshed_tools`,
`turn_start_refreshes_dirty_mcp_reuses_clean_runtime_and_cancels_discovery` and
refresh invalidation/cancellation tests. Extend native source-retirement controls
with pending deferred requests; demonstrate no idle discovery for that demand,
one coalesced turn projection, first-request refreshed tools, cancellation
completion and rejection of retired-source work.

## Profile MCP removal help: adopt `3f2603f2a2`

`mcp get` reports the active profile file and the global-only removal scope when
that enabled user profile actually defines the named server. Global/inherited
servers retain the ordinary removal hint. Configuration and removal behavior do
not change. Require `get_reports_removal_scope_for_profile_servers`, including
profile-only, shadowed and inherited servers, and retain credential/header-helper
redaction tests. No secret or additional configuration file is read by this hint.

## Structured process-start errors: adopt `dd8cee9822`

Optional JSON-RPC `data.kind` distinguishes missing/denied/other spawn failures,
unavailable sandbox runtimes and sandbox preparation failure. Existing error
codes/messages and process-map cleanup are retained. No retry or weaker sandbox
fallback is introduced. Response timing records only the code and parsed bounded
enum for `process/start`; other RPCs retain their previous receipt fields, and
unknown/malformed data gives no parsed kind. Raw server error text is not added
to this trace-safe event. No ALab consumer or exhaustive match needs adaptation.

The commit adds no native regression tests. Before integration require real
missing-executable and permission-denied launches, ordinary spawn failure,
unavailable runtime, sandbox transform/preparation failure and success controls.
Assert unchanged refusal codes, no launched process or leaked process-map entry
on failure, protocol serde spelling, optional/unknown data compatibility and
method-scoped receipt telemetry without command/environment/error-body leakage.
Retain native destination permissions and ALab source custody in both local and
remote execution paths.

## Queued attachment cwd: adopt `4413deac57`

Queue add/update resolve relative local image and audio paths against the loaded
thread's current config snapshot cwd, or the unloaded stored thread cwd, before
the existing queue service snapshots attachment bytes. Absolute paths remain
unchanged. A still-relative joined result is refused before file I/O or queue
mutation. This is lexical path resolution, with no canonicalization, existence
probe or extra file grant in the conversion helper. Client IDs, attribution and
queue ordering retain their existing paths.

Archived/ephemeral and direct-input restrictions are still checked; starting a
queued item still requires a loaded thread and current provider checks. Native
permission and source admission remain required at actual execution. Preserve
ALab's queue/input ownership and private handoff behavior rather than resolve
saved paths later against a destination process cwd.

Require `queue_attachments_resolve_relative_paths_against_thread_cwd` for actual
add and update snapshots with different server/thread fixture images. Extend
coverage to local audio, unloaded stored cwd, unchanged absolute paths and an
invalid relative cwd with unchanged queue/no attachment read. Retain attachment
snapshot/error, client-identity, attributed-update, queue ordering and
subagent/archive/ephemeral refusal tests.

## Review admission and validation

The new artifact must preserve all 66 prior decision objects and provide exact
76-commit coverage with one active declaration per commit. Historical overlapping
artifacts point directly to the new active replacement. The next upstream commit,
`6d99161bced769f68a60e3c37e5dd4fc6fbad44c`, remains unreviewed by this report and
must fail complete coverage. Named native regressions and the Cloud status
adaptation remain integration requirements; none is asserted to have run here.

Observed validation: the maintained intake validator accepts the exact 76-commit
record. Independent source/record checks find one active decision per covered
commit, all 66 original decision objects unchanged, and all 14 historical
decision bodies unchanged apart from direct supersession metadata. The 77th
commit is rejected as incomplete. A private Git index over both sides of the
Bazel commit confirms all 17 paths match the existing removal policy. All 58
upstream workflow Python tests passed in 34.947 seconds with managed Python 3.14;
`git diff --check` passed. No Rust command, application build, integration or
native runtime reproduction was run for this intake extension.
