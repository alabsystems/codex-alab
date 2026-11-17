<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream source review: twelve commits after 347143f14010

This source review covers exactly `347143f140109e2736f788a2f70e6ef1216c498e..91ae3d10bf4c37ba31eb6dc99d32d8cca896dbec`, beginning `f5fb19fc4a45`. It follows committed review `633f5b23ed` through the prior 94-commit frontier; it does not replace those records or the eventual ALab integration base. The cached upper bound is `e5839cbc591df02408d5009e287887c7d57a9167`; the next commit `e87cd55ed96c6194189eadaff87a70076a948dbb` and later commits were not reviewed here.

ALab source and feature catalog were read at immutable origin/main `98ca741f4124decfd7b61ff81eb758c199c432d6`. HEAD and origin/main still named that source after declaration validation. The maintained intake validator and all 14 feature IDs were extracted from that commit. Canonical cached patches are retained individually, serialized exactly as `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA` and checked again byte-for-byte. No fetch, ref/index/shared-source mutation, Rust execution, generator, compiler admission, native run, application certification, packaging or publication occurred for this review. The staged integration is separate and remains unchecked.

The first six production/test patches and owned interactions were reviewed by `/root/aterm_audit`; `/root` independently reviewed the last six and supplied preservation requirements and test candidates. All 48 declared test candidates below were found in their named upstream target or ALab source files, and their package names were checked against Cargo manifests. This is source declaration checking, not compiled discovery, variant enumeration or execution. Every candidate is `not_run`.

## Decisions and canonical patches

| Commit | Decision | Subject | Canonical patch SHA256 |
| --- | --- | --- | --- |
| `f5fb19fc4a45` | adopt | Normalize managed network requirements before merging (#61391) | `eb71e2b22a9ecd63079592435bc4e7a4eb87edbcc6adc8e36bf797c8ef28b897` |
| `f941a5fb927b` | adopt | Add optional release IDs to cloud plugin source locations (#61393) | `0e90afbe15eb7675ac6d9332613ba20a67e29a34488aeb00476397fe0d87dc69` |
| `5158e9780e7c` | keep-opt-in | Add opt-in file-backed tool audio (#61394) | `e828535b3b6ed6670021a46e07a517553037120f794574da0fcea3a23aa73d24` |
| `f626b8fde93f` | adapt | Remove legacy sandbox-policy accessors from configuration (#61395) | `61dce8dcc9438f5dfbdf876998c62f341d7be570277837cca49cf9dea90162fe` |
| `c5ef97aa892c` | adopt | Add default model provider selection to the app server (#61397) | `44bdc6833b240a143d7f709666056294a9580107d162dfa3daa1cc72cce55d10` |
| `e8f5d2bb5555` | adopt | Restore shared connector metadata for Codex Apps tools (#61398) | `10de35f9882b053fa0df155d36ebbabb2448adaa73e4c70361e651693ea1f870` |
| `0b297ec5ee0e` | adopt | Retry incomplete cloud catalogs with exponential backoff (#61400) | `f8602c21f39c4b84794c50d0edb10050e482a531f99a92a937d522c655731c87` |
| `43ca762a59b1` | adopt | Release MCP OAuth read locks before keyring access (#61401) | `096aaa09a12b10da38aa169312f0727b60d685a87945bd288b63ec55f3cdbe61` |
| `61bedf383f66` | adopt | Update Daybreak links in TUI cybersecurity notices (#61402) | `3c5012cd31813022281c83832ef7add1dab8f3e552999d6904eeecf8dd494e99` |
| `68847438b510` | adapt | Add a structured status-line snapshot for the displayed thread (#61403) | `5c234c020b9c7313cc3a3d3acd90f054a330fe547685d468a221ecad73a7afbf` |
| `38cdc4d65561` | adapt | Show the primary executor's workspace in TUI status surfaces (#61405) | `b2562b96ab8840939a47b24a1d3d3ef6d7ddf059bb3cb440407ebea004a8c800` |
| `91ae3d10bf4c` | adopt | Expose token usage totals in statusline state (#61408) | `a63df64248e114bee9297e5eaa4b3f17c327a8729720b899054eb7ed017b2102` |

## f5fb19fc4a45 — adopt

Normalize each validated managed network source before composition, preserving exact empty allowlist replacement, source attribution, original legacy selector spelling and legacy output for older executors. Equivalent dual declarations in one source are accepted using allow/deny/review selector identities; conflicting declarations, both legacy aliases and mixed surfaces across contributing sources remain refused. Cloud errors identify sources without echoing submitted TOML. This retains managed restrictions, not a new saved-grant or native permission bypass.

Affected catalog features: `account-isolation`, `product-policy`, `provider-recovery`, `qualification-fixtures`.

Preservation and integration requirements:

- Retain current ALab credential/factory source pairing and all native human holds; managed network normalization does not supply those grants.
- Do not substitute an unmanaged or lossy legacy projection for canonical permissions. Preserve explicit empty allowlist replacement and denial precedence.
- The saved-grant loopback integration test has skip_if_no_network; actual network-enabled execution must be witnessed before claiming that scenario passed.

Owned interactions inspected:

- No ALab-owned NetworkRequirementsToml, permissions_network or exact cloud-error assertion callsite was found in the owned Rust search.
- ALab cloud contract adapters still retain actual auth/source identity; sanitized composition errors remain observational failures.

Source-declared candidates; all **not_run**:

- `codex-config --lib network_surfaces_in_one_layer_must_agree` — `codex-rs/config/src/requirements_layers/stack_tests.rs` (upstream target91ae).
- `codex-config --lib empty_network_allowlist_clears_inherited_allows` — `codex-rs/config/src/requirements_layers/stack_tests.rs` (upstream target91ae).
- `codex-config --lib shadowed_allows_agree_across_network_spellings` — `codex-rs/config/src/requirements_layers/stack_tests.rs` (upstream target91ae).
- `codex-config --lib managed_network_requirements_control_broker_project_discovery` — `codex-rs/config/src/loader/managed_project_discovery_tests.rs` (upstream target91ae).
- `codex-cloud-config --lib cloud_validation_checks_network_spellings_per_fragment` — `codex-rs/cloud-config/src/validation_tests.rs` (upstream target91ae).
- `codex-core --lib saved_grant_cannot_open_loopback_under_fixed_allowlist` — `codex-rs/core/src/config/network_proxy_spec_tests.rs` (upstream target91ae).

## f941a5fb927b — adopt

Carry an optional opaque release ID on Cloud plugin source locations. Catalog merge still distinguishes cloud membership/version from executor environment and local installation paths; no new downloader, resolver, activation or default is introduced. Existing catalog tests populate the value, and all upstream construction sites receive the new field.

Affected catalog features: `skill-discovery`, `product-policy`, `qualification-fixtures`.

Preservation and integration requirements:

- Keep release IDs opaque; they are not filesystem paths, auth identities or install authority.
- Retain ALab Apps/plugins/tool_suggest disabled defaults and actual per-account catalog retirement. No owned Cloud-location constructor was found requiring the new field.

Owned interactions inspected:

- Owned Rust search found no PluginSourceLocation::Cloud literal; existing default policy still sets apps=false, plugins=false and tool_suggest=false.

Source-declared candidates; all **not_run**:

- `codex-core-plugins --lib merged_catalog_prefers_cloud_and_matches_executor_versions` — `codex-rs/core-plugins/src/catalog_tests.rs` (upstream target91ae).
- `codex-core-plugins --lib partial_cloud_merges_known_plugins_and_preserves_unmatched_enablement` — `codex-rs/core-plugins/src/catalog_tests.rs` (upstream target91ae).

## 5158e9780e7c — keep-opt-in

Keep file_backed_tool_audio under development and false by default. When explicitly selected, fresh direct/custom tool audio is validated and budgeted before attachment upload, preserving audio charges and producer limits. Core supplies upload tags; tool producers do not supply file IDs or policy tags. Backend failures retain normalized inline audio, invalid/rejected clips use bounded text, and unavailable-file placeholders alter prompt projections rather than retained history. Duration resolution precedes continuation compaction. This adds real upload I/O and worker decoding only on the selected path; no performance measurement was made.

Affected catalog features: `exec-handoff`, `history-pagination`, `provider-recovery`, `product-policy`, `qualification-fixtures`.

Preservation and integration requirements:

- Do not enable the feature in ALab defaults or expand any held full-capsule/all-root SourceV1 receiver route to exercise it.
- Preserve original attachment-store/source identity, file IDs, measured durations, tool success and new history-budget metadata across every supported handoff container. Existing exec_model_context preserves file audio IDs/durations in both tool output forms, but that control predates the new metadata and is not complete coverage of this feature.
- producer_bounded_tool_audio is deliberately serde-skipped and transient. MemoryItem's exact encode/decode comparison must continue refusing capture if transient state cannot round-trip; never silently drop it to obtain an export.
- tool_audio_budget_applied is persisted nested metadata. Before enabled cross-build handoff, review legacy-reader behavior and require fail-closed compatibility where dropping this budgeting semantic can change retained output; current source inspection is not a reproduced rollback loss.
- Keep attribution conservative when tool file audio competes with a user upload; never blame an unrelated user recording or lose accepted history on an ambiguous provider error.

Owned interactions inspected:

- exec_model_context.rs captures file_audio_durations, rejected_audio_file_ids and complete MemoryItem envelopes; memory_context_keeps_file_audio_in_both_tool_output_forms is an existing owned control.
- exec_pending_input.rs and exec_memory_records.rs also use MemoryItem; a context-version-only compatibility change would miss those containers.
- No owned AnyToolResult::into_response or renamed replace_unresolvable_image_files callsite was found. exec_transfer only replaces the retained AnyToolResult payload and keeps original transfer attribution.
- The current staged image-replay compatibility adaptation and global ResultWrite custody requirements from earlier reviewed prefixes remain required; this review does not retire or qualify them.

**Coverage gap:** New budget-marker/transient admission, store identity, cancellation and older-reader behavior still need supported-path handoff controls before enabled ALab portability is asserted. The existing durable direct/CodeMode legacy/paginated resume test is source evidence, not that private-handoff result.

**Revisit before enabled portability:** Before enabled ALab execution portability or cross-build handoff with file_backed_tool_audio, execute supported-path budget, original store/source identity and cancellation preservation controls across all MemoryItem containers, and require fail-closed older-reader behavior for unsupported persisted budgeting semantics. The existing file-audio ID/duration fixture is incomplete coverage. Keep the default false and existing receiver holds intact while these requirements remain unproven.

Source-declared candidates; all **not_run**:

- `codex-core --lib producer_bounded_audio_keeps_the_same_parts_as_inline_history` — `codex-rs/core/src/session/file_audio_tests.rs` (upstream target91ae).
- `codex-core --lib zero_cost_invalid_audio_cannot_expand_into_unbounded_error_text` — `codex-rs/core/src/session/file_audio_tests.rs` (upstream target91ae).
- `codex-core --lib transient_upload_failure_retains_inline_audio_but_invalid_attachments_are_omitted` — `codex-rs/core/src/session/file_audio_tests.rs` (upstream target91ae).
- `codex-core --lib only_file_backed_tool_audio_competes_with_a_user_upload_for_an_unknown_file_error` — `codex-rs/core/src/context_manager/history_rejected_audio_tests.rs` (upstream target91ae).
- `codex-core --test all tool_audio_store_id_survives_rollout_and_next_request` — `codex-rs/core/tests/suite/scenarios_audio_file_tests.rs` (upstream target91ae).
- `codex-core --test all dynamic_tool_audio_exceeding_the_output_budget_is_omitted` — `codex-rs/core/tests/suite/audio_truncation.rs` (upstream target91ae).
- `codex-utils-output-truncation --lib measured_tool_output_keeps_charges_paired_with_retained_clips` — `codex-rs/utils/output-truncation/src/truncate_tests.rs` (upstream target91ae).
- `codex-core --lib memory_context_keeps_file_audio_in_both_tool_output_forms` — `codex-rs/alab-core/glue/exec_model_context.rs` (ALab source98ca).

## f626b8fde93f — adapt

Adopt removal of obsolete Config/Permissions legacy_sandbox_policy accessors while adapting five ALab-owned test assertions to canonical permission profiles or explicit filesystem/network predicates. Upstream production behavior only changes the resume-mismatch diagnostic to its active sandbox label; permission selection and enforcement remain canonical. Do not reintroduce the removed lossy compatibility accessor to make tests compile.

Affected catalog features: `product-policy`, `native-session`, `exec-handoff`, `qualification-fixtures`.

Preservation and integration requirements:

- Update alab-tui/src/tests.rs calls at source lines187,223,349 and alab-tui/glue/resume_permission_defaults_tests.rs lines114,123. Preserve exact native defaults, explicit CLI/user choices and remote-resume scope, using PermissionProfile::Disabled/read_only or the actual expected canonical profile.
- Keep native human holds, trust decisions and effective permission constraints unchanged; do not weaken expected restrictions or merely delete assertions.
- Where a genuinely legacy wire is required, use explicit fallible conversion with its actual cwd, as upstream tests now do.

Owned interactions inspected:

- No owned production accessor call was found. The five affected assertions are in aterm_defaults_reach_the_config, aterm_permission_defaults_preserve_explicit_cli_choices, the_users_config_overrides_them and native_resume_permission_child (two branches).

Source-declared candidates; all **not_run**:

- `codex-alab-tui --lib aterm_defaults_reach_the_config` — `codex-rs/alab-tui/src/tests.rs` (ALab source98ca).
- `codex-alab-tui --lib aterm_permission_defaults_preserve_explicit_cli_choices` — `codex-rs/alab-tui/src/tests.rs` (ALab source98ca).
- `codex-alab-tui --lib the_users_config_overrides_them` — `codex-rs/alab-tui/src/tests.rs` (ALab source98ca).
- `codex-tui --lib native_resume_permissions_are_scoped_and_preserve_explicit_choices` — `codex-rs/alab-tui/glue/resume_permission_defaults_tests.rs` (ALab source98ca).
- `codex-core --lib permission_profile_override_falls_back_when_disallowed_by_requirements` — `codex-rs/core/src/config/config_tests.rs` (upstream target91ae).
- `codex-core --test all forged_worktree_project_config_cannot_start_host_mcp` — `codex-rs/core/tests/suite/worktree_trust.rs` (upstream target91ae).

## c5ef97aa892c — adopt

Adopt modelProvider/default/set as an explicit config-serialized write for configured providers. It uses existing writable-user-layer, managed requirement and effective-config validation, reports CLI overrides, and invalidates catalog state only for an effective Ok write. Explicit model/list providers resolve from the current catalog configuration without replacing the active provider disk cache. Retained/running threads keep their own selected provider; a default change is not account or execution authority.

Affected catalog features: `model-discovery`, `account-isolation`, `product-policy`, `qualification-fixtures`.

Preservation and integration requirements:

- Keep ALab provider definitions, Kimi subscription auth, per-originator model caches and captured credential/factory ownership. Do not recapture a newer auth source into an old transport because a default changed.
- Preserve running and restored child/root provider IDs and explicit overrides in exec_child_configuration and exec_bootstrap_server; never replace them with the new-user default.
- Carry schema/TypeScript/Python additions through maintained generation and keep native build/proof refusal separate. This review invokes no generator or application build.

Owned interactions inspected:

- No owned exhaustive ClientRequest/ClientResponsePayload match requires the new request variant; owned typed requests use narrower variants or generic wrappers.
- exec_child_configuration.rs carries an explicit model_provider override and validates provider identity; exec_bootstrap_server.rs restores the current runtime provider. Existing model-discovery and account-isolation tests remain necessary.

Source-declared candidates; all **not_run**:

- `codex-app-server --lib model_provider_default_set_reports_a_cli_override` — `codex-rs/app-server/src/config_manager_service_tests.rs` (upstream target91ae).
- `codex-app-server --test all provider_requirements_are_effective_and_read_only` — `codex-rs/app-server/tests/suite/v2/config_model_provider_requirements_tests.rs` (upstream target91ae).
- `codex-app-server --test all model_list_selects_a_provider_without_replacing_the_startup_catalog` — `codex-rs/app-server/tests/suite/v2/model_list_requirements_tests.rs` (upstream target91ae).
- `codex-app-server --test all model_list_follows_active_provider_auth_edit` — `codex-rs/app-server/tests/suite/v2/model_list_requirements_tests.rs` (upstream target91ae).

## e8f5d2bb5555 — adopt

Restore nonempty connector names/descriptions from each Codex Apps tools/list page's shared metadata using that tool's connector_id. Regular MCP listing now stays on its standard path and still strips untrusted reserved connector metadata. The change fixes model-visible namespace/description presentation without supplying installation, account or tool-execution grants.

Affected catalog features: `product-policy`, `skill-discovery`, `account-isolation`, `qualification-fixtures`.

Preservation and integration requirements:

- Keep Apps/plugins/tool_suggest off in packaged ALab defaults. Shared display metadata does not enable a connector or provide auth.
- Preserve actual server registration/provenance, per-account transport/catalog identity and regular-server spoof stripping; connector IDs/release IDs do not replace those identities.
- Run the new real two-page Apps test with network available; skip_if_no_network must not be counted as exercised coverage.

Owned interactions inspected:

- No owned call to list_tools_with_connector_ids or removed tool_info_from_listed_tool was found. Existing optional MCP elicitation and disabled install behavior remain ALab-owned and are not replaced by this metadata conversion.

Source-declared candidates; all **not_run**:

- `codex-core --test all apps_tool_pages_expose_shared_connector_namespaces_to_the_model` — `codex-rs/core/tests/suite/mcp_tool_exposure.rs` (upstream target91ae).
- `codex-mcp --lib custom_mcp_connector_metadata_is_stripped` — `codex-rs/codex-mcp/src/rmcp_client.rs` (upstream target91ae).
- `codex-mcp --lib codex_apps_connector_metadata_is_preserved` — `codex-rs/codex-mcp/src/rmcp_client.rs` (upstream target91ae).
- `codex-core --test all elevated_apps_catalog_limit_requires_host_owned_registration` — `codex-rs/core/tests/suite/mcp_tool_exposure.rs` (upstream target91ae).

## 0b297ec5ee0e — adopt

Use a shared five-second exponential retry capped at 300 seconds for partial, warning-bearing or failed cloud catalog results. Complete warning-free results clear backoff; accepted partial results replace the prior catalog and clear skills read cache. Remove the same-turn auth latch so each step checks actual auth/resource freshness. Auth/resource replacement bypasses cooldown, and stale asynchronous publication remains refused. No performance measurement was made.

Affected catalog features: `skill-discovery`, `account-isolation`, `product-policy`, `qualification-fixtures`.

Preservation and integration requirements:

- Keep real account/source retirement and catalog invalidation; cache cooldown confers no authority to read a retired source.
- Retain Apps/plugins/tool_suggest disabled defaults, independent system-skill roots and bounded filesystem watching.
- Check actual retry, cancellation, partial replacement and cache-read identity; successful source review is not execution evidence.

Owned interactions inspected:

- Parent source comparison through observed98ca found relevant owned crosspoints unchanged apart from startup_account_read.rs (+61/-3), which is unrelated to these cloud caches.
- No required direct ALab callsite adaptation was demonstrated. Prior held handoff routes and account-source policies remain intact.

Source-declared candidates; all **not_run**:

- `codex-mcp-extension --lib partial_or_warning_cloud_catalogs_retry_with_backoff_and_recover` — `codex-rs/ext/mcp/src/cloud_plugin_tests.rs` (upstream target91ae).
- `codex-mcp-extension --lib cloud_refresh_reuses_catalogs_and_retries` — `codex-rs/ext/mcp/src/cloud_plugin_tests.rs` (upstream target91ae).
- `codex-skills-extension --lib partial_or_warning_cloud_catalogs_retry_with_backoff_and_recover` — `codex-rs/ext/skills/src/cloud_cache_tests.rs` (upstream target91ae).
- `codex-skills-extension --lib cloud_refresh_replaces_catalogs_and_read_cache_for_every_accepted_result` — `codex-rs/ext/skills/src/cloud_cache_tests.rs` (upstream target91ae).

## 43ca762a59b1 — adopt

Snapshot ciphertext under the OAuth shared file lock and release the owned read guard before keyring access. Conditional writers retain the borrowed exclusive guard across comparison and save. Cached plaintext still requires the current keyring entry; missing file, rotated key or keyring error cannot reuse old plaintext. An existing encrypted file with missing key does not create a replacement key. Updates reuse one passphrase lookup.

Affected catalog features: `account-isolation`, `qualification-fixtures`.

Preservation and integration requirements:

- Keep native secret/source custody and selected-account authority; a storage failure must not trigger unrequested credential fallback.
- Preserve compare-and-save serialization for conditional writers and original refresh ownership. Releasing a read lock before keyring I/O is not permission to release a writer's exclusive lock.

Owned interactions inspected:

- Parent found no direct owned call to the removed or private storage helpers. Existing ALab auth file-lock/source admission remains independent and required.

Source-declared candidates; all **not_run**:

- `codex-rmcp-client --lib secrets_reads_release_the_file_lock_before_keyring_access` — `codex-rs/rmcp-client/src/oauth/tests/store_lock_tests.rs` (upstream target91ae).
- `codex-rmcp-client --lib updating_an_existing_secret_requires_one_keyring_lookup` — `codex-rs/rmcp-client/src/oauth/tests/store_lock_tests.rs` (upstream target91ae).
- `codex-secrets --lib cached_reads_check_file_presence_key_rotation_and_keyring_errors` — `codex-rs/secrets/src/local_cache_tests.rs` (upstream target91ae).

## 61bedf383f66 — adopt

Update Daybreak notice link destinations and snapshot wrapping only. This changes no entitlement, policy enforcement, retry permission or ALab Daybreak default. Keep native policy-denial ownership and existing safety recovery authority checks.

Affected catalog features: `provider-recovery`, `native-ui`.

Preservation and integration requirements:

- Do not infer Daybreak entitlement or a retry grant from a new URL or visible notice. ALab security reminder settlement remains bound to its own verified current observations.

Owned interactions inspected:

- ALab defaults and safety recovery hooks remain unchanged; notice wording is not equivalent implementation of owned security-reminder identity/custody.

Source-declared candidates; all **not_run**:

- `codex-tui --lib cyber_policy_error_event_snapshot` — `codex-rs/tui/src/history_cell/tests.rs` (upstream target91ae).
- `codex-tui --lib cyber_policy_error_event_astra_snapshot` — `codex-rs/tui/src/history_cell/tests.rs` (upstream target91ae).
- `codex-tui --lib cyber_policy_error_event_narrow_snapshot` — `codex-rs/tui/src/history_cell/tests.rs` (upstream target91ae).
- `codex-tui --lib safety_recourse_handles_a_policy_block` — `codex-rs/tui/src/app/tests/safety_buffering.rs` (upstream target91ae).

## 68847438b510 — adapt

Adopt a structured statusline snapshot for the displayed thread as display data. Omit read-only/unattached snapshots and unconfirmed permission/approval/service-tier fields; inferred child state clears parent collaboration mode. Adapt the owned service-tier ThreadSessionState fixture with thread_settings_confirmed=true for its explicitly confirmed settings, preserving the earlier required read_only_history adaptation. Status snapshots are not new approval, goal or execution authority.

Affected catalog features: `native-ui`, `exec-handoff`, `qualification-fixtures`.

Preservation and integration requirements:

- Update codex-rs/alab-tui/glue/service_tier_tests.rs ThreadSessionState literal at observed line172 with truthful confirmed state, while preserving read_only_history=false required by the earlier reviewed prefix.
- Keep staged provider reads unactivated; this snapshot alone supplies no provider execution route or install authority.
- Retain ALab actual thread/backend status, goal-storage holds, title suppression and clipboard hooks. Do not project unconfirmed fields as granted permissions.

Owned interactions inspected:

- Parent found the owned service-tier literal requires the added field. Status, clipboard and title hook crosspoints must remain thin and retain their actual owner checks.

Source-declared candidates; all **not_run**:

- `codex-tui --lib statusline_state_projects_the_displayed_settings` — `codex-rs/tui/src/chatwidget/tests/statusline_state_tests.rs` (upstream target91ae).
- `codex-tui --lib inactive_thread_started_notification_initializes_replay_session` — `codex-rs/tui/src/app/tests.rs` (upstream target91ae).
- `codex-tui --lib read_only_history_hides_the_statusline_until_interactive_resume` — `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs` (upstream target91ae).

## 38cdc4d65561 — adapt

Carry primary executor cwd through initialization, restoration and settings into status/title display. A missing update retains prior state; explicit empty environments clear it and fall back appropriately. Abbreviate home only for a local daemon plus server-local executor. Adapt the same owned local service-tier fixture with primary_environment_cwd=None. Display paths never replace actual working-directory, trust or execution-admission identities.

Affected catalog features: `native-ui`, `exec-handoff`, `qualification-fixtures`.

Preservation and integration requirements:

- Add primary_environment_cwd=None to the local fixture in alab-tui/glue/service_tier_tests.rs alongside truthful confirmed/read-only fields.
- Keep ALab working-directory/trust/exec-admission custody tied to actual native roots, not status text or a remote-looking display path.
- Preserve captured thread/backend identity, prompt stash, title suppression and confirmed clipboard delivery across displayed-thread switches.

Owned interactions inspected:

- Parent found the same owned ThreadSessionState literal needs this field; no separate runtime permission adapter was demonstrated.

Source-declared candidates; all **not_run**:

- `codex-tui --lib statusline_workspace_follows_primary_executor_and_legacy_settings` — `codex-rs/tui/src/chatwidget/tests/app_server.rs` (upstream target91ae).
- `codex-tui --lib statusline_and_title_abbreviate_only_the_local_workspace` — `codex-rs/tui/src/chatwidget/tests/app_server.rs` (upstream target91ae).
- `codex-tui --lib inactive_thread_started_notification_initializes_replay_session` — `codex-rs/tui/src/app/tests.rs` (upstream target91ae).

## 91ae3d10bf4c — adopt

Expose token totals from the actual provider report in statusline state, including cache-write counts preserved by protocol-to-TUI conversion. Checked input minus cache-read/write rejects overflow, negative or invalid breakdowns and synthetic context fills, while retaining a measured zero. Keep display totals separate from ALab persisted goal/billing accounting and its acknowledgment holds.

Affected catalog features: `native-ui`, `goal-storage-recovery`, `qualification-fixtures`.

Preservation and integration requirements:

- Do not infer ledger, budget or continuation authority from display totals. Retain original dispatch/goal identity and acknowledged native accounting.
- Preserve cache-write input counts during conversion and omit unavailable or synthetic breakdowns rather than inventing zero.

Owned interactions inspected:

- Parent found no owned crate::token_usage::TokenUsage literal. The owned retry_notice ThreadTokenUsage fixture is the distinct protocol/defaulted type, so no demonstrated literal adaptation is required.

Source-declared candidates; all **not_run**:

- `codex-tui --lib statusline_totals_use_the_reported_token_breakdown` — `codex-rs/tui/src/chatwidget/tests/statusline_state_tests.rs` (upstream target91ae).

## Bounded validation and limits

Maintained `upstream_intake.snapshot` and `validate` accepted exactly twelve ordered commits, their canonical subjects/paths, decisions, nonempty evidence and current catalog feature IDs. The unchanged maintained CLI also exited 0: `review coverage complete: 12 commits; qualification is separate`. The committed incremental record is `alab/upstream/91ae3d10bf-from-347143f140-intake.json`, SHA256 `e54fafcb25930ca7bdcfddd1538f7debe2df9757327d50612a8bdc7f2de6f9ae`. A scan of 137 parseable committed JSON files in `alab/upstream` found no duplicate entries for this prefix; only list-valued commit records participate in that check.

The maintained status assessment using the original ALab base059301 and all three explicit intake files also passed: 106 reviewed, zero missing, pending or duplicate/error entries. Candidate and origin main lack target91ae, and qualification/adoption remain unproven. This is combined declaration coverage, not admission of an integrated source.

Concrete required owned adaptations are the five removed legacy-sandbox test calls and the service-tier fixture’s confirmed-settings/primary-executor-cwd fields (plus its earlier reviewed read-only-history field). Upstream file-backed audio remains off by default. Its private-handoff budget/transient metadata coverage is an explicit conditional gap, not a claimed production-loss reproduction. Existing boxed-Submission and active-context-store adaptations from the prior twelve-commit review remain pending integration requirements; none is retired as equivalent here.

Existing SourceV1/full-capsule/all-root receiver holds stay intact. No held route may be enabled to obtain a preservation test. Managed requirements, explicit human holds, current account/source identity, actual execution roots and acknowledged goal/storage accounting remain authoritative; status display, metadata, cached catalog rows and source review confer none of those grants. The recorded native compiler build-script-execution certification refusal is not waived.

Before integration, recompute committed admission coverage against the actual ALab base, merge current main, implement the declared adaptations, bind affected features to the final source and execute their real scenarios and the unchanged native gate. The committed intake declares reviewed decisions only. It does not qualify any integrated source or complete review through the cached upper bound.
