<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Source-only upstream intake after aea4c6ee4b

Review of the exact twelve linear cached commits `aea4c6ee4b652d84b416731b11c2b8299d767209..0184dfa6a48f0110dd9c6835522492908e5abcf3`, against fixed ALab main `d29e1f4de2b15d82de004ce6bb8c43eca8eb5b90`. Parent reports managed upstream cap `885a1df5ee5f18172233b12d6c94544a24e7b338` at 00:42 UTC; this review reads cached objects and does not independently authenticate fetch freshness. All twelve decisions are **adopt**, conditional on the explicit dependency/retention requirements below. No actual upstream merge/import, implementation, Rust build/test/lint, runtime, native admission, package, pin/default/profile/cache change or qualification occurs here. Test names are source declarations, **not_run**, with no runtime enumeration or final evidence binding.

The preceding eight active records cover 166 decisions through aea4; this candidate adds 12 for 178 across 9 records if maintained validation/census succeeds. Actual staged 61-import candidate and its unqualified status remain unchanged. Canonical patch bytes use `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA`; hashes/sizes below identify those exact bytes, including snapshots. The shell-login patch has 118 snapshot files, all exactly hash-field-value-only changes under recorded normalization; substantive source and assertions were reviewed separately. No source string is execution proof.

Still-open inherited requirements:

- 81/92 original Err/EOF through synchronous Post-hook/steer custody remains UNIMPLEMENTED and required; actual terminal-steering/request-count oracles must remain.
- f226 exactly-once terminal_outcome OnceLock rebase must preserve actual ALab native goal completion/Invocation/transfer hooks and real budget-stop/retained goal callbacks. Do not restore obsolete atomic.
- a394 actual initial_scroll_enabled/scroll_dirty strict terminal carry still required.
- 598 local exec ownership and e51 remote pending capsule/default-off/held route compatibility are not superseded.
- Generic request origin, queued/final native policy, current budget and native Goal nonreplay authority remain required/open; this source review is no native result or fix.
- SourceV1/full-root receiver remains held; packaged apps=false/plugins=false/tool_suggest=false and optional backends stay unchanged.

## 1. `09c711b783cf19f5ed60b6e26fcf4ef269acae00` — Use `clip.exe` for single-line ASCII clipboard copies in WSL (#61724)

Decision: **adopt**. Classes: fix. Features: native-ui, execution-continuity, qualification-fixtures.

Use bounded clip.exe for eligible single-line printable ASCII/tab copies under WSL, preserving installed Linux PowerShell adapter precedence and Unicode/multiline fallback.

Canonical patch SHA-256 `2661f0bdfcd9daeca0ccd8d572fb14794388cd479ea3f51432ff0570c804e26d`, 16225 bytes.

- Required integration behavior: Keep trusted system-directory discovery separate from user PATH/cwd and defer built-in PowerShell discovery until fallback; no command/text interpolation in the shell argument bridge.
- Required integration behavior: Retain the original overall deadline, 200 ms clip discovery/1 s clip writer subdeadlines, kill-and-reap before fallback, and exact released PowerShell command signature. Even-length ASCII gets a temporary trailing NUL; Unicode/newlines/control bytes use PowerShell.
- Required integration behavior: No clipboard writer, installed adapter or Windows process was invoked during source review. Preserve optional ALab clipboard/title suppression and raw-mode/source ownership.
- ALab source interaction: No owned direct WSL writer, system_directories, linux_system_executable or modified options caller was found. Linux-only production and Unix adapter fixtures are different runtime boundaries.
- Required source coverage gap: Actual supported WSL clip/OEM-text decoding, NUL handling, missing/failing writer and real Windows process retirement remain required platform evidence; a macOS shell-adapter test alone cannot qualify WSL behavior.

Source candidate selectors (all not_run):

- `codex-tui --lib installed_adapter_receives_compatible_command_and_never_copies_expired_work` — `codex-rs/tui/src/clipboard_copy/wsl_tests.rs:9` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual Unix shell adapter exercises byte eligibility/padding, precedence, compatible arguments, exact received text and expired work; cfg(all(test,unix)), not actual WSL clipboard.

## 2. `e649c6d74c92c638658af8f08290a1ea81d6872f` — Fix Unicode grapheme highlighting in history search (#61725)

Decision: **adopt**. Classes: fix. Features: native-ui.

Map history-search matches onto complete Unicode graphemes and render the separate bash prompt independently.

Canonical patch SHA-256 `cf04f070f17be523f82500f03300e14cd7600fb7c608444fe1c369619132c5ed`, 7616 bytes.

- Required integration behavior: Preserve byte ranges used by textarea, lowercase-expansion handling, deduplication of identical grapheme ranges and the separately rendered ASCII ! prefix; do not alter stored command text or submit a search preview.
- Required integration behavior: Retain escape/Ctrl+C restoration of structured draft, pending paste flush and ALab stash ownership.
- ALab source interaction: Changed private helper has only upstream internal/test callers; owned draft capture operates on actual composer state and is not replaced by highlight ranges.

Source candidate selectors (all not_run):

- `codex-tui --lib history_search_match_ranges_preserve_unicode_boundaries` — `codex-rs/tui/src/bottom_pane/chat_composer/history_search.rs:778` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Combining marks, case folding and repeated matches use exact original UTF-8/grapheme boundaries.
- `codex-tui --lib history_search_highlights_matches_until_accepted` — `codex-rs/tui/src/bottom_pane/chat_composer/history_search.rs:1001` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual composer render checks prompt-only/body styles and combining-mark highlight.
- `codex-tui --lib history_search_esc_restores_original_draft` — `codex-rs/tui/src/bottom_pane/chat_composer/history_search.rs:1109` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual composer draft restored after preview cancellation.

## 3. `eff3036bf41b4b00bb5d34c6ec0ca3a1ff131baa` — Preserve graphemes in mention search match highlighting (#61726)

Decision: **adopt**. Classes: fix. Features: native-ui, skill-discovery.

Render whole grapheme clusters for fuzzy mention-name/path highlights, retaining the different scalar/grapheme index conventions.

Canonical patch SHA-256 `040f65f4ce73dd766a27991a28b115a66c67fceda48587addcf6129584087b69`, 7068 bytes.

- Required integration behavior: Keep primary scalar match indices separate from filesystem path grapheme indices and bold a cluster if any indexed part matches; width truncation must not split joined emoji.
- Required integration behavior: Display highlighting must not mutate mention insertion, file paths, selected tool identities or plugin/skill policy.
- ALab source interaction: No owned caller or literal of these private SearchResult/render helpers was found. Later cloud-label row 630 adds a field to this fixture; its missing literal is fixed by 0184.

Source candidate selectors (all not_run):

- `codex-tui --lib fuzzy_matches_preserve_graphemes_when_truncated` — `codex-rs/tui/src/bottom_pane/mentions_v2/render_tests.rs:169` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual ratatui buffer and bold-cell assertions for joined-emoji plugin/path truncation; use final0184 literal fix.

## 4. `ca0bbbdf0c83cb64f2fb48cf0d5ffbc3d4409ab9` — Optimize default and attached thread listings with targeted indexes (#61727)

Decision: **adopt**. Classes: fix. Features: history-pagination, execution-continuity, qualification-fixtures.

Use targeted visible-interactive timestamp and attached-parent/archive indexes with literal built-in source predicates.

Canonical patch SHA-256 `261ea05284aae7d5ecd90b0c3cf056e8dab401263c9d3b4442ab85ef12c63224`, 8187 bytes.

- Required integration behavior: Adopt together with subsequent eb53 numbering/test repair before claiming any usable prefix: ca0 alone duplicates upstream migration 0063 and omits ThreadFilterOptions.included_thread_ids in its new test.
- Required integration behavior: Literal SQL values are the four fixed built-in encoded sources, selected only for the exact default interactive set and existing filter conditions. Preserve provider/cwd/source/archive/relation/search/cursor filters and stable timestamp/id ordering.
- Required integration behavior: DROP INDEX changes an index, not thread data or lifecycle custody. Extra indexes impose migration/storage/write work; query-plan selection is not a measured latency or zero-I/O claim.
- Required integration behavior: ALab auth-wall tables use separate owned schema setup and no 0063/0064 numbered migration; do not edit upstream migration contents/checksums or use a failed migration as permission to reset a database.
- ALab source interaction: No owned ThreadFilterOptions literal or caller of private query builder was found. Existing history projections and native goal/journal source ownership remain independent.
- Required source coverage gap: ca0 intermediate prefix is source-known invalid until eb53; this record is not a pristine build or actual database-upgrade result.

Source candidate selectors (all not_run):

- `codex-state --lib default_sidebar_listing_uses_the_interactive_timestamp_index` — `codex-rs/state/src/runtime/threads.rs:2850` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Scratch SQLite EXPLAIN QUERY PLAN requires the interactive updated-time index; corrected final test field.
- `codex-state --lib attached_thread_discovery_preserves_parent_lifecycle_and_cwd` — `codex-rs/state/src/runtime/threads.rs:1901` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual state attached-thread/cwd/parent ownership control.
- `codex-state --lib archived_thread_pages_use_ordered_indexes_without_sorting` — `codex-rs/state/src/runtime/threads.rs:2903` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual archived query plans and sort/ordering controls.
- `codex-app-server --test all thread_list_empty_source_kinds_defaults_to_interactive_only` — `codex-rs/app-server/tests/suite/v2/thread_list.rs:1811` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. App-server default CLI+chatgpt inclusion excludes exec source using real scratch rollouts.

## 5. `eb53da6c3ab0787f22707fcdc7f26ffd7316055e` — Fix thread listing migration numbering and test setup (#61730)

Decision: **adopt**. Classes: fix. Features: history-pagination, build-and-adoption, qualification-fixtures.

Repair the preceding upstream index migration collision by moving it to 0064 and complete the new query-plan fixture literal.

Canonical patch SHA-256 `9da1505f509f767edf6aa11137f36b0da855accc6814ee856684d11766451aa9`, 4406 bytes.

- Required integration behavior: Retain existing 0063 session-model migration and identical index SQL bytes at 0064; no ALab renumbering or migration-ledger bypass.
- Required integration behavior: Set included_thread_ids=None only in the upstream unrestricted-list fixture; retain actual caller inclusion filters. The earlier ca0 comment still says0063 but is not executed migration authority.
- ALab source interaction: Canonical no-renames patch represents exact delete/add SQL bytes; tree at target contains unique0063 and0064 migrations. No owned migration number collision found.

Source candidate selectors (all not_run):

- `codex-state --lib default_sidebar_listing_uses_the_interactive_timestamp_index` — `codex-rs/state/src/runtime/threads.rs:2850` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. StateRuntime scratch init plus corrected actual query plan executes final migration set; not an installed-database upgrade certificate.

## 6. `540eb31ece8b5fea95062795183fc00a05340c85` — Keep bidirectional TUI headers stable during shimmer (#61731)

Decision: **adopt**. Classes: fix, packaging. Features: native-ui, product-policy.

Keep bidi-sensitive animated headers as one static dim span and schedule only clock ticks unless progress animation still needs frames.

Canonical patch SHA-256 `b52f3076d6d5939009b4d4e95f6e8a911804b4dc24221e994ff1a14eea086f39`, 10386 bytes.

- Required integration behavior: Detect strong R/AL or Arabic-number bidi classes without reordering logical text. Retain reduced motion, progress updates and ordinary animated headers.
- Required integration behavior: Carry upstream unicode-bidi workspace/TUI dependency and upstream lock delta normally; never hand-edit upstream lock entries. No rendering throughput or power-saving measurement is claimed.
- ALab source interaction: ALab header/status integration and optional interface remain; no owned caller of changed shimmer helper or dependency pin was found.

Source candidate selectors (all not_run):

- `codex-tui --lib rtl_headers_remain_stable_during_the_sweep` — `codex-rs/tui/src/summary_shimmer_tests.rs:8` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Hebrew/Arabic/mixed-direction/Arabic-number snapshots compare both shimmer implementations and initial/sweep frames.
- `codex-tui --lib static_bidi_headers_only_schedule_clock_ticks_without_progress` — `codex-rs/tui/src/status_indicator_widget.rs:352` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual frame requester deadlines preserve 1,000 ms static ticks versus 32 ms progress/ordinary shimmer.

## 7. `ac2c4a339c15d96c9cd32b242ef487d48d27961b` — Preserve agent message phases in the cloud backend (#61737)

Decision: **adopt**. Classes: fix, protocol. Features: history-pagination, exec-handoff, native-session.

Preserve explicitly supplied cloud commentary/final/partial phases through wire summaries into app-server agent items.

Canonical patch SHA-256 `76a30f745479aef33ba29b272ca582cccb014fe75bd1d154ba6c194988370c8d`, 9084 bytes.

- Required integration behavior: Known oneof tag with sentinel1 maps to matching phase; absent, unknown tag or other value staysNone, not an invented FinalAnswer.
- Required integration behavior: Retain item/turn IDs, timestamps, questions, ordering, redacted Debug and owned model-context/phase custody. A phase is presentation/history metadata, not a native receipt, goal-stop origin, policy fence or Cloud execution grant.
- Required integration behavior: Cloud/remote backends remain opt-in and SourceV1/all-root receiver stays held; do not equate live cloud events with complete local history/accepted input.
- ALab source interaction: No owned cloud ItemSummary::AgentMessage literal/exhaustive match was found. ALab MessagePhase use is the existing protocol enum and does not need this new cloud enum field.
- Required source coverage gap: Named modified controls cover FinalAnswer and missing phase; actual Commentary, PartialAnswer and unknown numeric/tag controls are not newly supplied by this patch and remain required before claiming all variant runtime coverage.

Source candidate selectors (all not_run):

- `codex-app-server-client --lib completed_reply_preserves_the_full_text_phase_questions_and_timestamp` — `codex-rs/app-server-client/src/cloud_backend/events_tests.rs:57` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Conversion retains final phase, actual full text/questions and timestamp.
- `codex-cloud-client --lib item_lifecycle_keeps_identity_and_timestamps` — `codex-rs/cloud-client/src/item_tests.rs:35` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Real protobuf lifecycle bytes decode final phase and identity/timestamps.
- `codex-cloud-client --lib item_history_paginates_with_explicit_filters_and_preserves_rows` — `codex-rs/cloud-client/src/history_tests.rs:15` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. HTTP/2 history pages preserve rows/filters with missing phase.
- `codex-cloud-client --lib item_summaries_redact_sensitive_fields` — `codex-rs/cloud-client/src/item_tests.rs:388` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Phase field does not expose private item text in Debug.

## 8. `f1ebc7b372bc837b7c6386113e867256d7cc7036` — Reuse matching thread inventories for lightweight MCP status pages (#61738)

Decision: **adopt**. Classes: fix, refactor. Features: account-isolation, skill-discovery, execution-continuity, product-policy.

Reuse connected MCP tool inventories only when exact page registrations, client settings, environments, permissions and authentication match and credentials are fixed.

Canonical patch SHA-256 `8e6cc05d31954e651b13273986f8e814f2aecdd5705042bca525345cce47ea90`, 74030 bytes.

- Required integration behavior: Retain auth token/account/user/FedRAMP comparison and exact ready Environment Arc identities, client config, exposure, cwd/MXC/requirements and per-server permission comparison. A same server name is insufficient; scoped environment keys stay distinct.
- Required integration behavior: No runtime-auth, stored OAuth, EMA or dynamic env/header/helper source qualifies as fixed credentials. Apps keeps independent auth/filtering; EMA uses only its admitted thread connection; remaining servers discover separately and full detail retains resources/templates.
- Required integration behavior: Limit failure inspection and inventory work to the requested page; status metadata is not a tool invocation, connection/source lease or automatic MCP runtime activation.
- Required integration behavior: Preserve ALab assignment publication guard in Session::publish_mcp_projection and captured credential/transport source retirement. Matching token bytes are not ALab source authority. Keep apps/plugins/tool_suggest packaged off and all held routes unchanged.
- ALab source interaction: New McpClientConfig groups six client fields; no owned McpConfig literal/client-field access or private status-helper caller was found. Owned Config capture rebuilds under fresh destination helpers rather than serializing McpRuntime inventories.
- ALab source interaction: Core mcp_runtime.rs refactor retains the publication method containing ALAB(accounts) guard outside the modified binding helper. Status reuse optimization is not equivalent to account retirement.
- Required source coverage gap: Exact source matching/selected fixtures cannot qualify every policy/auth/environment race; run actual source-retirement controls plus selected MCP integration fixtures after adaptation/import.

Source candidate selectors (all not_run):

- `codex-app-server --test all mcp_server_status_discovers_thread_inventory` — `codex-rs/app-server/tests/suite/v2/mcp_server_status.rs:130` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Two test_case variants compare fixed Authorization one initialization versus stored OAuth rediscovery, and Full actual resource/template calls.
- `codex-app-server --test all dormant_thread_status_returns_live_inventory_without_waking_the_thread` — `codex-rs/app-server/tests/suite/v2/mcp_server_status.rs:1091` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual app-server two-thread stdio inventories retain second NotStarted state; explicit lazy startup fixture flag only.
- `codex-app-server --test all mcp_server_status_keeps_same_named_environment_inventory_separate` — `codex-rs/app-server/tests/suite/v2/mcp_server_status.rs:1736` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual paginated ToolsAndAuthOnly same-name environment keys preserve origin/title/tools/auth.
- `codex-app-server --test all mcp_server_status_list_discovers_only_the_requested_page` — `codex-rs/app-server/tests/suite/v2/mcp_server_status.rs:236` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual server request counts bound discovery to requested page.
- `codex-app-server --test all mcp_server_status_list_reports_thread_runtime_connections` — `codex-rs/app-server/tests/suite/v2/mcp_server_status.rs:1246` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual config replacement and connected runtime statuses preserve fixed-header source.
- `codex-mcp --lib read_only_policy_does_not_reuse_a_writable_connection` — `codex-rs/codex-mcp/src/connection_manager_tests.rs:5893` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual reconciliation requires a distinct read-only connection.
- `codex-mcp --lib view_only_changes_reuse_connection_and_preserve_the_old_step` — `codex-rs/codex-mcp/src/connection_manager_tests.rs:7392` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual connection sharing retains old frozen binding.
- `codex-mcp --lib shared_apps_snapshot_requires_writable_discovery_and_a_shared_cache` — `codex-rs/codex-mcp/src/connection_manager_tests.rs:2247` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Hosted Apps cache cannot bypass read-only/discovery constraints.
- `codex-login --lib a_fresh_cache_read_renews_application_policy_without_renewing_retained_credentials` — `codex-rs/alab-core/glue/auth_epoch_snapshot.rs:449` at `d29e1f4de2b15d82de004ce6bb8c43eca8eb5b90`. Existing owned source-epoch/transport policy control, not MCP reuse proof by itself.

## 9. `630c0d75abbe74784a35673926868cc4133ef6fa` — Mark cloud threads with a cloud icon in the TUI (#61742)

Decision: **adopt**. Classes: fix. Features: native-ui, history-pagination, account-isolation.

Decorate cloud thread labels independently of underlying names, search text and backend/thread identity, retaining markers across disconnect.

Canonical patch SHA-256 `5e1c821ec9055b7bef2139d2e81ce4e887c4425923d8840a33dd6afbf6baf7e3`, 60406 bytes.

- Required integration behavior: Retain first-observed backend owner and separate cloud backend identity from live handles. Cloud icon, title fallback and labels never confer account/source/workspace/approval/goal authority.
- Required integration behavior: Account for prefix display width before truncation in pickers, side tabs, mentions and task rows; leave stored names, inserted thread:// IDs, raw search and original ownership unchanged.
- Required integration behavior: Preserve ALab parked-backend bookkeeping, selected identity/status publication and title/clipboard suppression. New ThreadParamsMode carries actual Cloud versus Embedded settings ownership, not an ALab native grant.
- Required integration behavior: Take subsequent0184 required test literal correction before qualifying this prefix. Existing a394 terminal-scroll state carry and598/e51 local/remote exec ownership obligations remain open.
- ALab source interaction: No owned TaskMention/SearchResult/ChatTabs/AgentsOverviewRow/BackendRegistry literal or thread_settings_owner reference was found. Existing owned interface uses native fields without changing their owner or enabling cloud.
- Required source coverage gap: 630 prefix has missing SearchResult.label_style in the earlier fuzzy fixture until 0184; do not weaken that test or claim this intermediate source builds.

Source candidate selectors (all not_run):

- `codex-tui --lib live_center_columns` — `codex-rs/tui/src/app/agent_center_tests.rs:294` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual live overview columns/cloud+local labels and narrow ellipsis.
- `codex-tui --lib handle_start_side_seeds_navigation_before_thread_started` — `codex-rs/tui/src/app/tests.rs:4144` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual side tabs/chooser cloud labels and navigation preserve widths and native requests.
- `codex-tui --lib expanded_session_details_include_metadata` — `codex-rs/tui/src/resume_picker.rs:4831` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual detail rendering prefixes only named cloud session within constrained width.
- `codex-tui --lib mixed_backend_resume_table_snapshot` — `codex-rs/tui/src/resume_picker.rs:5114` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Mixed backend picker and narrow cloud row preserve ownership/order.
- `codex-tui --lib thread_identity_renders_names_and_fallbacks_on_both_surfaces` — `codex-rs/tui/src/chatwidget/tests/terminal_title.rs:286` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Embedded/cloud real status/title fallbacks and truncation.
- `codex-tui --lib task_mention_popup_and_selected_composer_snapshots` — `codex-rs/tui/src/bottom_pane/chat_composer.rs:7811` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Cloud label remains display-only in actual mention insertion.
- `codex-tui --lib parked_completion_matches_backend_and_exact_turn_without_changing_the_displayed_ui` — `codex-rs/alab-tui/glue/backend_bookkeeping_tests.rs:102` at `d29e1f4de2b15d82de004ce6bb8c43eca8eb5b90`. Existing owned actual parked/native backend ownership control; cloud marker is not a replacement authority.
- `codex-tui --lib conflicting_backend_notifications_preserve_the_registered_thread_owner` — `codex-rs/alab-tui/glue/backend_bookkeeping_tests.rs:332` at `d29e1f4de2b15d82de004ce6bb8c43eca8eb5b90`. Existing owned actual parked/native backend ownership control; cloud marker is not a replacement authority.

## 10. `5a6bbd29a45d81c69395c12f0d6fa5b31747aaa6` — Clarify shell-specific login guidance for `exec_command` (#61744)

Decision: **adopt**. Classes: fix, protocol. Features: execution-continuity, skill-discovery, qualification-fixtures.

Describe omitted exec login as inherited and expose PowerShell profile guidance only when the actual environment reports PowerShell.

Canonical patch SHA-256 `f737742ffad8c1d4ddde387eb0b615ea8975d254cd35e9e6f2e5d6e44999b5cf`, 210157 bytes.

- Required integration behavior: Retain actual shell execution/login configuration, exec permissions, TTY/environment parameters and runtime admission; this patch changes specification text/options construction, not shell startup authority or a new default.
- Required integration behavior: Keep shell readiness driven by actual selected turn environments. Portable snapshots may normalize only exact known PowerShell wording; other description changes must remain observable.
- Required integration behavior: Prompt-prefix compatibility accepts only exact released image/login tool-description changes and still compares the full result; it is not a blanket golden-file waiver.
- Required integration behavior: Preserve ALab model/tool completion, native Invocation attribution, current goal budget and prior81/92/f226 terminal/goal custody requirements. No autonomous shell profile or held remote path is enabled.
- ALab source interaction: No owned removed CommandToolOptions/create_exec_command_tool helper caller or ExecCommandHandlerOptions literal was found. All118 snapshot changes are exact hash-value-only diffs after hash-field normalization; actual source assertions and shell options were read independently.

Source candidate selectors (all not_run):

- `codex-core --lib login_description_follows_the_reported_shell` — `codex-rs/core/src/tools/handlers/shell_spec_tests.rs:101` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual schema wording for reported PowerShell/unix booleans.
- `codex-core --lib exec_command_tool_matches_expected_spec` — `codex-rs/core/src/tools/handlers/shell_spec_tests.rs:10` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Full actual ToolSpec schema including login/permissions remains exact.
- `core_test_support --lib portable_tool_schema_normalizes_only_the_exact_powershell_login_description` — `codex-rs/core/tests/common/context_snapshot/context_snapshot_tests.rs:386` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual portable normalizer checks exact Unix/PowerShell schema and embedded code-mode description; changed advice stays distinct.
- `codex-core --test all remote_test_env_exposes_target_shell_and_exec_guidance_to_model` — `codex-rs/core/tests/suite/remote_env.rs:360` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual remote executor reports selected shell and model-facing login spec; test target/platform prerequisites still apply.
- `codex-core --test all pending_attachment_installs_configuration_before_waiting_turn_resumes` — `codex-rs/core/tests/suite/remote_env.rs:1946` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual pending/ready environment requests compare schema; only exact shell guidance varies after readiness.
- `codex-tui --lib cloud_resume_picker_reconciles_admitted_steers` — `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs:322` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Existing real app-server cloud resume/steer transcript control; narrower bullet filter excludes unrelated rows, retains actual admitted responses.

## 11. `710bee73a89385aaf2afb41d885edf82473d941f` — Refactor history and notes tool registration (#61747)

Decision: **adopt**. Classes: refactor. Features: skill-discovery, account-isolation, exec-handoff.

Split history and notes builders while selecting each namespace description from the actual step ExtensionData and optional actual backend.

Canonical patch SHA-256 `eb9069f4c7eac3de5bba0b68ce7674e57cc474ebe8bbbed1d8f97c043ec4e15a`, 9005 bytes.

- Required integration behavior: An absent backend returns no tools; HTTP-backed installation keeps thread/config/prompt/tool registration together and preserves backend auth, routes, cancellation and encrypted history output.
- Required integration behavior: Retain captured model-specific HistoryToolMessages/NotesToolMessages at the actual step. Explicit empty description remains distinct from missing; do not substitute current global model/catalog on restored work.
- Required integration behavior: The refactor does not activate history-notes token-budget features, cloud refresh, apps/plugins or remote execution. Existing child token-budget capture/history/accepted-input obligations remain open.
- ALab source interaction: Builders remain crate-private and no owned direct call to removed signatures or literal was found. ALab child config explicitly preserves use_history_notes_extension independently of runtime authority.
- Required source coverage gap: Existing sparse-catalog control calls tool_spec directly; bundled contracts call the combined builder with step=None. Neither alone exercises the new per-namespace Some(step) or absent-backend path. An actual builder/ExtensionData control with model-specific and explicitly empty history/notes messages plus no-backend refusal remains required; context-management-tools scenarios are not substituted for this distinct history/notes contract.

Source candidate selectors (all not_run):

- `codex-history-notes-extension --lib bundled_tool_contracts` — `codex-rs/ext/history-notes/src/tools_tests.rs:195` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual generated history+notes tool schemas retain bundled contract.
- `codex-history-notes-extension --lib sparse_catalog_messages_preserve_defaults_and_explicit_empty_descriptions` — `codex-rs/ext/history-notes/src/tools_tests.rs:166` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. ToolSpec distinguishes sparse/default and explicitly empty descriptions.
- `codex-history-notes-extension --lib preserves_encrypted_history_output` — `codex-rs/ext/history-notes/src/tools_tests.rs:22` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Actual tool output metadata retains encrypted history rather than exposing private payload.
- `codex-app-server --test all app_server_uses_configured_notes_backend_for_context_window_hints` — `codex-rs/app-server/tests/suite/v2/history_notes_extension.rs:39` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Five test_case variants preserve native hints/capability/no-notes/failure-no-bridge/explicit bridge routing; fixture activation only.

## 12. `0184dfa6a48f0110dd9c6835522492908e5abcf3` — Fix the missing label style in the mention truncation test (#61762)

Decision: **adopt**. Classes: fix. Features: native-ui, qualification-fixtures.

Complete the mention truncation fixture with the new plain label field introduced by630 without altering its Unicode/style assertions.

Canonical patch SHA-256 `8b3ade50b23763c5d716c26e5b49ed707b396b3e095f60bc0bf2efa6abb3b582`, 1185 bytes.

- Required integration behavior: Retain all existing actual rendered text and bold-cell assertions; Plain models the local file result and does not activate cloud.
- Required integration behavior: This repairs an upstream fixture compile omission, not a tested runtime result or permission to relax future required fields.
- ALab source interaction: One upstream test line only. It completes the earlier eff grapheme and630 cloud-label composition; no owned production adaptation.

Source candidate selectors (all not_run):

- `codex-tui --lib fuzzy_matches_preserve_graphemes_when_truncated` — `codex-rs/tui/src/bottom_pane/mentions_v2/render_tests.rs:169` at `0184dfa6a48f0110dd9c6835522492908e5abcf3`. Same actual grapheme/style fixture on final complete literal; duplicate selector row is deliberate dependency evidence.

Every selector in the private review-data packet is bound to an actual test declaration, attributes, package manifest, target root and actual module inclusion chain. Macro labels are retained as attributes rather than invented libtest names. Platform/native/helper prerequisites still apply at execution. The two upstream broken-prefix cases (ca0 until eb53;630 until 0184) remain explicit; no retry, migration reset, weakened test or proof waiver is authorized. The record is review admission metadata only.
