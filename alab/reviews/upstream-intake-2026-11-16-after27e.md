<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream intake after 27e2227b2b49 — source review, 2026-11-16

This review covers exactly `27e2227b2b49a541eb799af8f8efb7032b1ef202` through `336b19ab26e0d4e36b16d8a21d668632cd800207` (12 contiguous upstream commits). The ALab comparison source is `d9223d5341005361e9ca2e680da72beefe5d1ee0`. A later read of local `origin/main` observed `dc98611873ebad955ecb7f4aa87b28539a01f9fb`; its sole intervening commit is the weekly-reset preflight follow-up, and the committed-record census found no overlap for this range. Cached upstream initially named `e5839cbc591df02408d5009e287887c7d57a9167`; the coordinator later observed `090be4df0724cdef5758e6a0934ba557af8b6bbe`. The reviewed target remains fixed.

Decision: **9 adopt, 3 adapt**. The three demonstrated owned source adaptations are the removed quota trait assertion, the explicit thread-list inclusion field, and exact child token-budget configuration capture/restore. No implementation or upstream merge accompanies this record.

All test rows below are **not_run**. Function declarations, test annotations, package manifests and target roots were checked in immutable source: 70 selector rows referring to 67 declarations in 16 packages. Qualified and short selectors for three reused controls are listed under both relevant commits. Source annotations establish candidate variants, not compiled test discovery or body execution. No compiler, native admission, signed receipt, runtime result or final evidence binding is claimed.

## Policy and custody

- Keep packaged apps/plugins/tool_suggest and MCP-sharing activation opt-in. This intake does not activate cloud refresh, a provider, telemetry, a launcher, or a dormant server. Current ALab Sol 6.1 ultra defaults and explicit account/model selections remain unchanged.
- Keep account/auth/resource generations, destination executable/root/network identity, task/goal admission, actual result-write confirmation and cancellation ownership. A trace, catalog, displayed setting or API input is not authority or a persistence receipt.
- Preserve the explicit SourceV1/all-root receiver hold. Do not synthesize native/package receipts, serialize source process launch authority, or enable a held route to obtain tests.
- The separately reviewed, unqualified ALab policy follow-up has a final one-shot native API issuance contract; this intake does not import it or promise retroactive cancellation of accepted Core queue work.

## Canonical patches

Each digest uses `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA` including the commit header. Stored patch bytes were compared again to cached Git objects.

| Commit | Decision | Subject | Patch SHA256 |
| --- | --- | --- | --- |
| `005f910210e8` | adopt | Require OAuth CONNECT metadata for Portals TCP tunnels (#61428) | `03109b004ac750e3d6a8dfba5dff935d67e117dd4948c006d5304696283ceb0f` |
| `6d919e65e20d` | adapt | Refresh cloud plugin and skill catalogs during step preparation (#61429) | `1269f9e73385673cf2aefba5fef4a5c304a50641ccd45f436147125efbbbc5c3` |
| `de83e7674a69` | adopt | Instrument MCP OAuth credential store locks (#61430) | `4720e650c5fb78c70aa2c89900e016d6fff8dba8944a9d4f5cbe199a66e901aa` |
| `9ac8c3f8d71e` | adopt | Close abandoned tool dispatch traces as aborted (#61434) | `2e8172a56df5a9098f1d2e0a459691f12dc1e0e94f56868ef36d9560bc37e407` |
| `8afe928a76fd` | adopt | Avoid starting dormant REPL servers for cleanup hooks (#61437) | `deeb7fd9e72e6005e5252c0f0e760c7d3ccf347a00d212a5d51a1f714c9356e4` |
| `9825e4b2fe69` | adopt | Use the provider default RPC in the TUI (#61438) | `d5c3e2c3960d6c7ee68c8a6c490cdd825e459329f0ad119a3e92f9881389dc27` |
| `e9919f4b4ef9` | adopt | Add process-local launch directories for shared MCP servers (#61444) | `6b6bf49894a77ea7cfdf43ed500e954f37428a492f39d77f31abf639d923f09f` |
| `32cec881b723` | adapt | Add thread ID inclusion filters to `thread/list` (#61446) | `55551314931341b6603b6aee0e5363579fd769fb5493907468f83a721ff0a731` |
| `d26406987e5c` | adopt | Add explicit cloud catalog refresh before turn admission (#61449) | `b2fa629d24c47006c1e4bf1df3cc64c476f4e291b2b271e5b28c0fed7c1883fa` |
| `c2c1ebba0a79` | adapt | Add optional tool output rendering for context window hints (#61452) | `467f14111a54096fb2ad08b03279db6a975dbff015d23dd62bacda36c6ff89ad` |
| `ecf54824c6e7` | adopt | Refactor session mail recording and compaction metadata capture (#61461) | `6ab5ad69421149d3bcd4f1b8bbb458982d19635b10059ff6c557b35215be1d96` |
| `336b19ab26e0` | adopt | Move JSON byte counting into `codex-utils-string` (#61465) | `d683f759fcd670ad4b064c3f2f7f979527795d86e2c9f5149521f640f8389965` |

## 005f910210e8 — adopt

For known shipped Portals gateway origins require x-portals-auth-mode: oauth in CONNECT metadata before reading any bearer credentials. Treat valid header tokens as opaque, sensitive bearer credentials instead of locally parsing unverified JWT type/version; the gateway still authenticates them. No gateway activation, credential selection or native permission is added.

Affected contracts: `account-isolation`, `provider-recovery`, `product-policy`, `qualification-fixtures`.

Required integration behavior:

- Retain exact known-gateway/SSH credential restrictions, sensitive headers, metadata/token bounds, renewal acknowledgement and rejection before token reads.
- Do not treat OAuth mode metadata or an opaque token string as proof of authentication, native source authority or tunnel admission. Preserve controlled reconnect without replaying old TCP streams.

ALab interactions:

- No owned TunnelCredential/read_bearer constructor or private helper callsite was found. Adopt upstream dependency removal, then resolve ALab dependencies without hand-editing upstream lock entries.

Limits and missing controls:

- The new parser control covers missing OAuth metadata and accepted opaque credentials. It does not exercise a live Portals account; no unrequested credentials or network tunnel are needed for this source review.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-tcp-tunnel --lib` | `portals_listener_requires_oauth_metadata` | `codex-rs/tcp-tunnel/src/lib_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-tcp-tunnel --lib` | `control_input_acknowledges_bearer_renewal_with_or_without_metadata` | `codex-rs/tcp-tunnel/src/lib_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-tcp-tunnel --lib` | `invalid_control_input_stops_credentials_without_exposing_secrets` | `codex-rs/tcp-tunnel/src/lib_tests.rs` @ `336b19ab26e0` | Source declaration only. |

## 6d919e65e20d — adapt

Move optional cloud plugin/skill freshness from turn-start callbacks to existing cancellable step preparation, after MCP reconciliation, and dirty discovery after every completed plugin install including skills-only installs. Remove requires_mcp_runtime instead of retaining a parallel startup path. Adapt the one ALab-owned quota-host assertion of that removed method while retaining its real early lifecycle/provenance behavior.

Affected contracts: `skill-discovery`, `account-isolation`, `product-policy`, `native-session`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Remove the obsolete requires_mcp_runtime assertion at alab-core/glue/quota_runtime_host_tests.rs79. That method was explicitly ignored for the already-asserted BeforeTaskRegistration phase; retain that phase and the existing real exec/non-exec, restored/child callback/provenance oracles. No new fixture mirroring the removed trait/default is required.
- Keep apps/plugins/tool_suggest disabled in packaged defaults, existing cloud-provider/config gates, account/resource freshness, same-auth fallback and retry backoff. Do not activate a provider to obtain test coverage.
- Preserve ALab assignment/native start admission, goal quota host timing, accepted input custody, prewarm exclusion and cancellation release. Later d264 adds an explicit data-only refresh API; it does not authorize bypassing these start guards.

ALab interactions:

- Only owned requires_mcp_runtime reference found: quota_runtime_host_tests.rs79. Host implements only BeforeTaskRegistration, for which the removed method was ignored. Existing phase and actual callback/provenance assertions retain the substantive contract; an independent source review confirmed this adaptation. The fixture compiles inside codex-app-server via extensions::quota_runtime_host.
- No owned cloud provider, PluginListQuery or SkillListQuery literal requiring constructor adaptation was found. The existing ALab defaults remain unchanged.

Limits and missing controls:

- remote_skills_only_install_refreshes_warmed_cloud_catalog and related loopback/plugin controls use skip_if_no_network; require actual non-skipped execution before runtime credit.
- At the final range target, the step-cancellation declaration is renamed step_discovery_reconciles_mcp_skips_prewarm_and_releases_lock_on_cancellation by d264; that final declaration is listed below.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `step_discovery_reconciles_mcp_skips_prewarm_and_releases_lock_on_cancellation` | `codex-rs/core/src/session/turn_start_mcp_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --test all` | `remote_skills_only_install_refreshes_warmed_cloud_catalog` | `codex-rs/core/tests/suite/request_plugin_install_skills.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-mcp-extension --lib` | `disabling_plugins_clears_cloud_catalog_and_skips_discovery` | `codex-rs/ext/mcp/src/cloud_plugin_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-skills-extension --lib` | `step_cloud_refresh_reuses_accepted_results_and_retries_failures` | `codex-rs/ext/skills/src/cloud_cache_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-app-server --lib` | `restored_member_provenance_is_retained_only_in_an_exec_host` | `codex-rs/alab-core/glue/quota_runtime_host_tests.rs` @ `d9223d534100` |  Owned include compiled by the listed host package, not by its physical glue directory. |
| `codex-app-server --lib` | `embedded_exec_and_non_exec_registries_do_not_share_admission` | `codex-rs/alab-core/glue/quota_runtime_host_tests.rs` @ `d9223d534100` |  Owned include compiled by the listed host package, not by its physical glue directory. |

## de83e7674a69 — adopt

Instrument blocking aggregate MCP OAuth store locks with bounded acquisition/hold observations and captured originating-client/operation tags. Standalone durations share existing startup buffering and opt-out limits. Lock policy/deadlines, conditional writes and pinned backend/no-fallback semantics remain unchanged; the file field drops before hold telemetry is recorded.

Affected contracts: `account-isolation`, `provider-recovery`, `product-policy`, `qualification-fixtures`.

Required integration behavior:

- Keep the ALab analytics/OTel/feedback opt-out defaults and bounded buffering. No credential, path, server URL or arbitrary client string is added to the fixed lock tags.
- Preserve original source/client attribution on worker threads, shared read/exclusive write policy, zero-timeout probe exclusion and file-lock release before keyring reads.
- Do not mistake lock metrics or timing for credential-source, storage-commit or native retirement evidence.

ALab interactions:

- No owned OAuthStoreLock constructor or removed private signature use was found. Existing ALab auth source/file-lock/native lifetime custody remains separate.
- The hold timer is a later struct field, so file release precedes its Drop telemetry; originator sync_scope only changes attribution and retains the chosen backend.

Limits and missing controls:

- The subprocess metric control isolates global installation; its declarations are source evidence, not execution or a new telemetry permission.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-rmcp-client --lib` | `aggregate_store_readers_share_access_while_writers_remain_exclusive` | `codex-rs/rmcp-client/src/oauth/tests/store_lock_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-rmcp-client --lib` | `secrets_reads_release_the_file_lock_before_keyring_access` | `codex-rs/rmcp-client/src/oauth/tests/store_lock_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-rmcp-client --lib` | `cancelled_rejection_retains_guard_and_preserves_replacement` | `codex-rs/rmcp-client/src/oauth/tests/credential_store_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-otel --lib` | `standalone_durations_share_startup_limits_and_opt_out` | `codex-rs/otel/src/metrics/buffered_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-otel --lib` | `opt_out_discards_pending_and_live_observations_until_reenabled` | `codex-rs/otel/src/metrics/buffered_tests.rs` @ `336b19ab26e0` | Source declaration only. |

## 9ac8c3f8d71e — adopt

Make tool dispatch trace ownership single-use: explicit completion/failure consumes the handle, and abandonment closes only its still-active trace as Aborted with no result payload. This repairs pending approval/interruption replay windows without claiming that a tool transaction, SQLite write or native effect rolled back.

Affected contracts: `execution-continuity`, `exec-handoff`, `goal-storage-recovery`, `qualification-fixtures`.

Required integration behavior:

- Preserve ALab retained goal completion, native source lifetime, actual write acknowledgements and result-receipt custody. A trace Aborted event must not settle, replay, confirm or discard those owners.
- Keep disabled tracing a no-op and preserve source/thread/turn/tool identity, one terminal trace event, result payload presence/absence and original timestamps.
- Do not infer execution or persistence rollback from a dropped dispatch handle or best-effort trace enqueue.

ALab interactions:

- No owned direct ToolDispatchTraceContext/ToolDispatchTrace clone, constructor or record_completed/failed caller was found. Integrate the upstream registry moves without moving or deleting the ALab completion/transfer hooks.

Limits and missing controls:

- Interrupted approval is a real app-server fixture; actual target/helper prerequisites and native execution remain necessary. Pure trace replay alone does not prove native transaction custody.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-rollout-trace --lib` | `dropping_tool_dispatch_closes_the_execution_window` | `codex-rs/rollout-trace/src/thread_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-app-server --test all` | `turn_interrupt_resolves_pending_command_approval_request` | `codex-rs/app-server/tests/suite/v2/turn_interrupt.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-rollout-trace --lib` | `runtime_code_cell_ids_can_repeat_across_threads_and_after_resume` | `codex-rs/rollout-trace/src/reducer/code_cell_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-alab-autopilot --lib` | `a_forced_sampling_teardown_preserves_the_committed_model_block_and_its_recovery` | `codex-rs/alab-autopilot/src/model_blocked_teardown_tests.rs` @ `d9223d534100` |  Owned include compiled by the listed host package, not by its physical glue directory. |

## 8afe928a76fd — adopt

Allow only a bundled node/cua REPL turn_ended cleanup to return empty success when its configured, private, local stdio, legacy-protocol connection remains unrequested and uncancelled. It avoids starting a process that does not exist. Ordinary calls, plugin/shared/remote/new-protocol connections and already-requested startup retain actual startup/attribution behavior.

Affected contracts: `execution-continuity`, `account-isolation`, `product-policy`, `qualification-fixtures`.

Required integration behavior:

- Preserve builtin hook provenance/server/tool checks and configured local/legacy/private eligibility; never use this skip mode as general tool approval or execution authority.
- Keep ALab Apps/plugins defaults and saved hook trust/policy, environment/source binding, live authentication and shared-connection custody unchanged.
- Do not bypass cleanup for an existing/started process; ordinary hooks must still start cached model-hidden tools and retain their timeout.
- The empty successful result describes skipped host housekeeping only; it is not an actual model-tool execution result, transaction rollback or native custody receipt.

ALab interactions:

- No owned HookMcpCall literal or latest_call_tool caller was found needing the new field/enum. Existing Core hook and session callsites are updated upstream.
- The skip result is empty and unattributed; actual calls still capture binding after startup. No code-mode server/default feature is enabled by this change.

Limits and missing controls:

- The real hooks fixture has connection/catalog test_case variants and skip_if_no_network. Require both actual non-skipped variants; source names are not runtime discovery.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-mcp --lib` | `cleanup_skips_only_an_unrequested_private_local_stdio_connection` | `codex-rs/codex-mcp/src/connection_manager_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --test all` | `mcp_tool_hook_starts_cached_model_hidden_tools` | `codex-rs/core/tests/suite/hooks_mcp.rs` @ `336b19ab26e0` | source cases: connection, catalog |
| `codex-core --test all` | `mcp_tool_hook_timeout_includes_cached_server_startup` | `codex-rs/core/tests/suite/hooks_mcp.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-hooks --lib` | `bundled_cleanup_trust_requires_matching_plugin_server_and_event` | `codex-rs/hooks/src/engine/mod_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --test all` | `hosted_plugin_runtime_ps_mcp_tool_calls_use_current_auth_manager_token` | `codex-rs/core/tests/suite/mcp_auth_refresh.rs` @ `336b19ab26e0` | Source declaration only. |

## 9825e4b2fe69 — adopt

Use the dedicated modelProvider/default/set RPC for an explicit default selection, carrying the provider ID and showing its trimmed display name (ID fallback). Unsupported servers get upgrade guidance rather than a generic configuration-write fallback. Only confirmed effective writes or ambiguous transport/deserialization outcomes invalidate the model catalog; overridden/rejected writes preserve it.

Affected contracts: `model-discovery`, `account-isolation`, `product-policy`, `qualification-fixtures`.

Required integration behavior:

- Preserve managed/user-layer validation from the earlier dedicated RPC, explicit CLI overrides, current chat/provider ownership and existing source/factory pairing. This sets future chat defaults, not a running thread or native execution grant.
- Keep packaged Sol6.1/ultra and inert Kimi provider definitions unchanged unless an explicit valid provider choice overrides them.
- Do not equate ambiguous save with confirmed success or route old servers through a lower-level unvalidated write.

ALab interactions:

- No owned Write::Default literal, provider save callback or removed RPC helper caller was found. Earlier thread-scoped model catalog and exec provider restoration requirements remain in force.

Limits and missing controls:

- The changed tests cover named providers and unknown-variant compatibility plus ambiguous outcomes. Blank-name fallback is source-checked here; no separate executed empty-name oracle is claimed.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-tui --lib` | `space_saves_once_retries_and_preserves_the_current_chat` | `codex-rs/tui/src/app/provider_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-tui --lib` | `only_ambiguous_write_failures_invalidate_the_model_catalog` | `codex-rs/tui/src/app/provider_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-tui --lib` | `unsupported_list_method_explains_server_compatibility` | `codex-rs/tui/src/app/provider_tests.rs` @ `336b19ab26e0` | Source declaration only. |

## e9919f4b4ef9 — adopt

Accept process-local launch cwd input and host-owned direct MCP sandbox metadata. Two feature gates, local stdio restrictions, configured executable equality/cwd preservation, and launcher same-host capability probe remain explicit. Default-off prevents new launch behavior by default.

Affected contracts: `account-isolation`, `exec-handoff`, `product-policy`, `qualification-fixtures`.

Required integration behavior:

- Preserve ALab local-loader provenance wrapper at ConfigManager startup. No launch overrides persisted into shared configuration.
- Run supplied config/direct-MCP controls; qualify direct stdio shared launch path separately before asserting that path.
- Before supporting enabled app-server launch maps across ALab account/exec handoff, prove the reachable host-flag path and rebind the admitted destination process map after ExecConfiguration reconstruction. Do not serialize source map authority. Current default-off behavior is not a demonstrated regression.
- process-local launch API: No supplied parser/launch test covers malformed/relative CLI entries, configured explicit cwd retained, sharing=false, remote/agent-plugin exclusion, and absence of shared-config writes. Add focused cases if this host-supplied feature is enabled or advertised; keep false otherwise.

ALab interactions:

- App-server ConfigManager has owned destination/local-input occurrence fences; rename apply_arg0_paths→apply_process_settings must retain them.
- CLI startup/exec custody hooks are outside changed AppServer option hunks. New AppServerRuntimeOptions default covers existing owned ..Default literal.

Limits and missing controls:

- Public hidden host options are local process inputs, not native execution authority. Remote direct MCP call metadata test is not proof of local launcher custody.
- Config::load_config_with_layer_stack initializes mcp_launch_cwds empty; owned ExecConfiguration::restore currently rebinds destination executable/network inputs, not this new process-local map. Conditional supported-path compatibility limit until host consumer and feature enablement are established.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `config::tests::mcp_launch_cwds_follow_the_runtime_feature` | `codex-rs/core/src/config/config_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-app-server --test all` | `suite::v2::mcp_tool::mcp_server_tool_call_uses_each_threads_sandbox_cwd` | `codex-rs/app-server/tests/suite/v2/mcp_tool.rs` @ `336b19ab26e0` | Local test app-server/model/MCP servers; does not exercise local stdio shared-process CLI launch path. |

## 32cec881b723 — adapt

Accept before-pagination indexed inclusion filter, null/omitted unrestricted and empty-none semantics, strict100-entry input bound, exclusions precedence and scoped ordering. Adapt owned explicit ListThreadsParams literal to preserve unrestricted semantics.

Affected contracts: `history-pagination`, `exec-handoff`, `account-isolation`, `qualification-fixtures`.

Required integration behavior:

- alab-core/glue/exec_recovery_tree_tests.rs665: add included_thread_ids: None.
- Preserve generated bindings/schema/precomputed artifacts from canonical commit. Saved-ID clients must check server support; indexed absence is not global deletion.

ALab interactions:

- Existing ALab running-resume projection/goal resume locks untouched in same thread_processor file.
- Inclusion route deliberately forces indexed metadata; unknown/unindexed rollouts do not match and are not repaired by the query.

Limits and missing controls:

- Index-only inclusions can exclude legitimate unindexed files by contract. Query-plan test is not latency qualification.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-app-server --test all` | `suite::v2::thread_list::thread_list_inclusions_reconcile_old_moved_and_archived_ids` | `codex-rs/app-server/tests/suite/v2/thread_list.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-app-server --test all` | `suite::v2::thread_list::thread_list_validates_id_filter_bounds_and_preserves_section_order` | `codex-rs/app-server/tests/suite/v2/thread_list.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-app-server --test all` | `suite::v2::thread_list::thread_list_relation_filters_read_spawn_graph_from_state_db` | `codex-rs/app-server/tests/suite/v2/thread_list.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-app-server --test all` | `suite::v2::thread_list::thread_list_respects_search_term_filter` | `codex-rs/app-server/tests/suite/v2/thread_list.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-app-server --test all` | `suite::v2::thread_list::current_account_history_refills_list_and_search` | `codex-rs/app-server/tests/suite/v2/thread_list.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-state --lib` | `runtime::threads::tests::included_thread_ids_use_primary_key_lookups_across_scopes` | `codex-rs/state/src/runtime/threads.rs` @ `336b19ab26e0` | Query-plan oracle, not runtime latency benchmark. |

## d26406987e5c — adopt

Accept explicit turn-independent catalog refresh, concurrent independent contributors and per-thread publication/snapshot lock. Cache/retry semantics retain generation checks and cancellation-pending invalidation. No turn/service authority is granted. No concrete native guard-order regression found.

Affected contracts: `skill-discovery`, `account-isolation`, `product-policy`, `native-session`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Keep ALab apps=false/plugins=false defaults and current provider activation. No new production call is introduced by this commit.
- Core callers must use refresh API; direct concurrent contributor hooks lose removed extension-private serialization.
- Run actual cancellation/concurrency/generation/disabled-source controls, require body execution for network-skipping tests.
- Retain existing no-MCP quota-before-task-registration oracle when earlier6d919 obsolete requires_mcp_runtime assertion is adapted.
- ALab integrated native/resource boundary: No added upstream test by itself proves ALab source-retirement and bounded native turn admission through a held cloud callback. Reuse actual source/task/goal/permission controls on integrated source; add a scoped gated contributor case only if those controls do not reach the changed await. Do not enable held receiver routes.

ALab interactions:

- Normal turn_input start retains extension TurnStartAdmission through submission, settings/renewal admission, task/source/exec launch custody and host AgentTurnAdmission before RegularTask.run. New catalog awaits occur later in cancellation-wrapped prepare_tools after regular lifecycle.
- Permission/environment/accessed-root snapshots and current MCP auth/resource generation checks are unchanged; explicit refresh is catalog data and never admission evidence.
- Frozen a437 ALab policy contract remains final one-shot native API issuance; no retroactive Core queue policy cancellation is claimed.
- Held SourceV1/all-root receiver route remains held. Optional turn_id=None applies only explicit cloud discovery; executor callbacks remain Some.

Limits and missing controls:

- Catalog callbacks are extensible async work; completion/cancellation must release global preparation lock and preserve already published dirty state.
- Supplied unit fake-contributor test proves overlap/lock boundary, not remote service availability; actual Core fixture supplies turn/request evidence.
- Two unrelated test-only flake fixes narrow matcher streaming test boundary and wait for inherited file locks.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `session::tests::turn_start_mcp_tests::step_discovery_reconciles_mcp_skips_prewarm_and_releases_lock_on_cancellation` | `codex-rs/core/src/session/turn_start_mcp_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `session::tests::turn_start_mcp_tests::explicit_cloud_refresh_runs_concurrently_and_awaits_all_results` | `codex-rs/core/src/session/turn_start_mcp_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --test all` | `suite::skills_extension::cloud_lifecycle_tests::explicit_cloud_refresh_publishes_before_admission_and_reuses_catalogs` | `codex-rs/core/tests/suite/skills_extension/cloud_lifecycle_tests.rs` @ `336b19ab26e0` | skip_if_no_network returns early when unavailable; require actual fixture body/network prerequisite, not a skipped success. |
| `codex-mcp-extension --lib` | `cloud_plugin::tests::cloud_source_policy_skips_startup_discovery` | `codex-rs/ext/mcp/src/cloud_plugin_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-mcp-extension --lib` | `cloud_plugin::tests::disabling_plugins_clears_cloud_catalog_and_skips_discovery` | `codex-rs/ext/mcp/src/cloud_plugin_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-mcp-extension --lib` | `cloud_plugin::tests::explicit_cloud_refresh_replaces_cached_catalog` | `codex-rs/ext/mcp/src/cloud_plugin_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-mcp-extension --lib` | `cloud_plugin::tests::cloud_refresh_reuses_catalogs_and_retries` | `codex-rs/ext/mcp/src/cloud_plugin_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-skills-extension --lib` | `state::cloud_cache_tests::step_cloud_refresh_reuses_accepted_results_and_retries_failures` | `codex-rs/ext/skills/src/cloud_cache_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-skills-extension --lib` | `state::cloud_cache_tests::late_discovery_cannot_publish_into_a_new_generation` | `codex-rs/ext/skills/src/cloud_cache_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-skills-extension --lib` | `state::cloud_cache_tests::late_read_cannot_return_or_populate_a_replaced_cache` | `codex-rs/ext/skills/src/cloud_cache_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-skills-extension --lib` | `state::cloud_cache_tests::cloud_refresh_replaces_catalogs_and_read_cache_for_every_accepted_result` | `codex-rs/ext/skills/src/cloud_cache_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-file-search --lib` | `tests::session_streams_updates_before_walk_complete` | `codex-rs/file-search/src/lib_tests.rs` @ `336b19ab26e0` | No longer itself a full filesystem-walker streaming timing proof; other traversal tests retain walker scope. |
| `codex-core-plugins --lib` | `marketplace_staging::tests::cleanup_preserves_live_process_and_reclaims_its_abandoned_checkout` | `codex-rs/core-plugins/src/marketplace_staging_tests.rs` @ `336b19ab26e0` | Requires real git/child process/filesystem support. |
| `codex-skills-extension --test skills_extension` | `cloud_discovery_is_gated_and_reports_failures` | `codex-rs/ext/skills/tests/skills_extension.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-skills-extension --test skills_extension` | `discovery_v2_tests::v2_empty_failed_and_partial_results_never_load_legacy_roots` | `codex-rs/ext/skills/tests/skills_extension/discovery_v2_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --test all` | `suite::scenarios::frozen_cloud_skills::frozen_cloud_skills_with_named_executor_request_history` | `codex-rs/core/tests/suite/scenarios_frozen_cloud_skills.rs` @ `336b19ab26e0` | skip_if_no_network body must execute; requires ExecServerProcess fixture version0.164.0; no live cloud credentials. |

## c2c1ebba0a79 — adapt

Accept optional agent.thread_hint function output for native context-window fields and textual hints while default legacy rendering stays false. Exhaustive ALab child-runtime capture must retain the new effective bool through real exec configuration restore.

Affected contracts: `exec-handoff`, `product-policy`, `model-discovery`, `qualification-fixtures`.

Required integration behavior:

- alab-core/glue/exec_child_configuration.rs139: bind context_window_as_tool_output in exhaustive TokenBudgetConfig match; set TokenBudgetConfigToml.context_window_as_tool_output=Some(*value).
- alab-core/glue/exec_configuration.rs454: set new field true in existing real child-runtime roundtrip; existing full token_budget equality oracle then proves preservation.
- Preserve inherited-prefix/child-hint snapshots and standalone world-state item ordering.
- effective configuration + ALab recovery: Exercise true through actual TOML load and false/default compatibility; existing supplied load_config test only expects false, while snapshots set struct field directly. Extend owned roundtrip true as noted. A focused agent-path/world-state-diff positive can supplement broad fork snapshots if final path changes.

ALab interactions:

- ALab memory context preserves raw ResponseItems/metadata; no hardcoded marker parsing for this new field found in owned paths.
- Apps/plugins defaults and source/exec custody remain unchanged.

Limits and missing controls:

- True changes request item type/placement and flattens textual hints into agent.thread_hint; preserve child-only window identity and ordinary history/compaction metadata.
- Broad integration snapshots require real request body execution and do not alone prove config-file true parsing.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `config::tests::load_config_resolves_token_budget_config` | `codex-rs/core/src/config/config_tests.rs` @ `336b19ab26e0` | Current cases do not parse context_window_as_tool_output=true from TOML. |
| `codex-core --test all` | `suite::scenarios::prediction_fork_preserves_context_above_compaction_threshold` | `codex-rs/core/tests/suite/scenarios.rs` @ `336b19ab26e0` | skip_if_no_network guard; actual body required. |
| `codex-core --test all` | `suite::scenarios::partial_answers::partial_answers_survive_forked_child_requests` | `codex-rs/core/tests/suite/scenarios_partial_answers.rs` @ `336b19ab26e0` | source cases: filtered, preserved legacy, preserved paginated, preserved code mode; test_case labels, not unverified generated full libtest IDs; base filter selects all4. skip_if_no_network guard; actual body required. |
| `codex-core --lib` | `config::exec_configuration::tests::child_runtime_settings_round_trip_without_replaying_parent_overrides` | `codex-rs/alab-core/glue/exec_configuration.rs` @ `d9223d534100` | Requires new field fixture adaptation before compilation; set true so carried new option is proven. Owned include compiled by the listed host package, not by its physical glue directory. |

## ecf54824c6e7 — adopt

Accept direct optional-delivery handle consumption and local compaction resume capture under same State lock. Actual result-write receipt still confirms only after successful rollout persistence on a live thread; no new receipt or accepted input is synthesized.

Affected contracts: `exec-handoff`, `history-pagination`, `execution-continuity`, `qualification-fixtures`.

Required integration behavior:

- Retain first-batch result_write confirmation, communication boundary order, pending confirmed send waits and live-thread qualifier.
- Use moved native mail selector; keep actual accepted-input/recovery, active compaction context and byte-budget controls in integrated regression selection.

ALab interactions:

- ALab startup_mailbox/input_snapshot controller publication sharing and NextTurn phase hooks unchanged.
- Compaction local capture occurs after history/world-state replacement under same state guard; no new await or unlocked read.
- Existing ALab weekly compaction hooks and active selected-step retention remain unchanged.

Limits and missing controls:

- Moved plain-message roundtrip does not prove owned worker-result ACK by itself; retain relevant first-batch receipt controls plus native confirmation ordering test.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `session::tests::mail_publication_tests::record_inter_agent_communication_sets_turn_id_in_rollout_and_resume` | `codex-rs/core/src/session/mail_publication_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `session::tests::inter_agent_communication_waits_for_confirmed_delivery_persistence` | `codex-rs/core/src/session/tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `session::tests::record_inter_agent_communication_preserves_item_id_in_rollout_and_resume` | `codex-rs/core/src/session/tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `session::tests::compaction_persists_resume_metadata_and_companion_records` | `codex-rs/core/src/session/tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `session::tests::remote_compaction_v2_retains_only_the_selected_step` | `codex-rs/core/src/session/tests.rs` @ `336b19ab26e0` | source cases: primary succeeds, fallback executes |
| `codex-core --lib` | `session::tests::interrupting_compaction_fallback_retains_last_known_step_context` | `codex-rs/core/src/session/tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `session::input_queue::exec_input_state::tests::root_and_controller_mail_share_one_publication_after_round_trip` | `codex-rs/alab-core/glue/exec_input_state.rs` @ `d9223d534100` |  Owned include compiled by the listed host package, not by its physical glue directory. |
| `codex-core --lib` | `session::input_queue::exec_input_state::tests::admitted_recovery_retains_accounting_controller_mail_and_original_startup_baseline` | `codex-rs/alab-core/glue/exec_input_state.rs` @ `d9223d534100` |  Owned include compiled by the listed host package, not by its physical glue directory. |
| `codex-core --lib` | `session::input_queue::exec_input_state::tests::recovered_start_leaves_deferred_mail_for_the_original_delivery_phase` | `codex-rs/alab-core/glue/exec_input_state.rs` @ `d9223d534100` |  Owned include compiled by the listed host package, not by its physical glue directory. |
| `codex-core --lib` | `context_manager::history::alab_exec_context::tests::memory_context_retains_item_allocations_across_actual_install` | `codex-rs/alab-core/glue/exec_model_context.rs` @ `d9223d534100` |  Owned include compiled by the listed host package, not by its physical glue directory. |

## 336b19ab26e0 — adopt

Accept exact streaming saturating JSON byte counter moved from core private module into public codex-utils-string; three Core caller imports switch without changing budgets, error behavior or retained output.

Affected contracts: `provider-recovery`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Retain actual HTTP/WebSocket/retained metadata budget tests reaching shared helper.
- Preserve ALab exec history snapshot hook and upstream source deletion as canonical import, not owned hand deletion.

ALab interactions:

- No remaining ALab-owned crate::utils::json caller found. Owned exec context allocation/metadata retention is adjacent only; no changed hook.

Limits and missing controls:

- Neighboring utils bounded_json UTF8 test is not a direct helper oracle; select Core helper consumers. Export visibility change is source-only until actual component checks.

Source-declared candidates, all **not_run**. Upstream declarations use final target `336b19ab26e0`; owned declarations use comparison source `d9223d534100`. Run only on the eventual admitted integrated source with truthful package/target and skip/body observations.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `client::tool_metadata::tests::http_message_budget_preserves_resources_until_the_message_still_exceeds_limit` | `codex-rs/core/src/client_tool_metadata_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `client::tool_metadata::tests::websocket_budget_uses_the_actual_delta_and_does_not_mutate_logical_history` | `codex-rs/core/src/client_tool_metadata_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `client::tool_metadata::tests::inventory_only_message_budget_counts_the_complete_encoded_envelope` | `codex-rs/core/src/client_tool_metadata_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `tools::executed_tool_calls::direct_tests::direct_recording_preserves_encoded_arguments_in_history` | `codex-rs/core/src/tools/executed_tool_calls_direct_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-core --lib` | `tools::executed_tool_calls::request_metadata::tests::direct_retained_metadata_budget_sheds_results_then_calls_across_refresh` | `codex-rs/core/src/tools/executed_tool_calls/request_metadata_tests.rs` @ `336b19ab26e0` | Source declaration only. |
| `codex-utils-string --lib` | `json::tests::bounded_json_counts_utf8_output_bytes` | `codex-rs/utils/string/src/json.rs` @ `336b19ab26e0` | This test exercises to_json_string_bounded, not direct serialized_json_bytes; Core controls above reach moved helper.Adjacent UTF-8 utility control; not a direct counting-helper oracle. |

## Required runtime boundary and remaining limits

The table is a candidate selection plan, not feature evidence. Preserve existing native/task/source/goal, accepted-input, compaction/recovery and actual terminal controls when integrating. Tests that require network, a platform, credentials, compiled helpers or a running service must execute their bodies under the admitted environment; a skip does not qualify the behavior. Remote MCP metadata controls do not prove local shared-process launch custody. The private local cleanup path must remain distinct from model tool execution.

For `6d919e65e20d`, remove only the obsolete `requires_mcp_runtime` assertion from the existing BeforeTaskRegistration fixture; retain its phase and actual exec/non-exec/restored/child callback provenance. The removed method was expressly ignored in that phase. No redundant fixture for a deleted default is needed.

For `c2c1ebba0a79`, retain the effective true bool through owned TOML capture and actual child-runtime restoration; also cover true config loading and false/default compatibility. Snapshots that set a struct directly do not prove TOML true parsing.

For `e9919f4b4ef9`, the new destination process map is initialized outside ordinary configuration reconstruction. The default-off/app-server-only scope has not established a reachable current ALab handoff regression. Before supporting an enabled path, prove the host consumer and rebind current destination inputs; never serialize source launcher authority. Missing parser/local launch controls are explicit requirements above.

For `9ac8c3f8d71e`, aborted traces describe abandoned observation only. Existing native goal completion, storage, ToolResult/ResultWrite confirmation and source custody remain authoritative; interruption is not proof of rollback. For `ecf54824c6e7`, keep actual first-batch delivery receipts and require successful persistence on a live thread before confirmation.

Maintained `upstream_intake.snapshot`/`validate` checks exact ordered coverage, subjects, paths, classes and current feature IDs. That source-data validation and the canonical patch/test declaration checks do not compile code, establish behavioral parity, qualify the application, admit execution, or satisfy final feature evidence.
