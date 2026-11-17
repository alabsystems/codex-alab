<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream source intake after e583

This records 12 contiguous cached upstream commits after `e5839cbc591df02408d5009e287887c7d57a9167` through `b8cbdbfeff72869e5d715480572aad24f739aa65`,
compared with fixed main `a34e913c9b5b3036dc203d30af6e98abe912a6b3`. Decisions: **10 adopt, 2 keep-opt-in**.
Only this review and its intake JSON change. No upstream code is imported and no
Rust, build, runtime test, native admission, compiler/pin/profile/cache/default or
held-route change or publication is performed. Candidate tests are **not_run /
UNRUN**, without execution bindings. The staged upstream import, separate ordinary
native component checks and application qualification remain distinct.

The supplied managed fetch observed cap
`44834c58fa99d0a04f222e18817b99b430f7c0e8` at 03:48:37 UTC Nov17, with 40 later
commits beyond this fixed target. This is a retained observation, not a claim that
the cap is the current remote tip. All 12 canonical patches were independently
reserialized with `git -c color.ui=false show --format=fuller --no-ext-diff
--no-renames --binary SHA`, matching retained bytes/digests: **302,636 bytes**.
All twelve production/test patches were read in bounded batches. The final-cohort
source and actual declaration/module/manifest identities are retained externally.
Source reads, intake validation and ownership census never qualify test execution.

## 01. 0d07dae8c9f7b0cd390aa4340fc1376b9d7c2e33 — adopt

Avoid unnecessary rollout item copies during child thread creation (#61833)

Count only persisted inherited records without collecting them, and reuse telemetry-filtered items for append metadata while preserving persisted child boundaries and ordinals.

Classes: refactor. Feature owners: history-pagination, execution-continuity.

Retain native history/rollout and child-parent attribution. Avoided vector collection is a source change, not a measured allocation or latency result.

Canonical patch SHA256 `5c6429e9735532006a11b143fbbb9a2edc05a8e8e31c6f83a8ecc7077b04258e`, 8,334 bytes.

## 02. 586203ad13f2e3153f5436f07942e50a8581f125 — adopt

Avoid redundant store reads for loaded thread metadata (#61837)

Reuse one persisted metadata read for loaded metadata-only thread/read, overlay actual live settings, and load history only when requested; preserve trimmed titles and legacy preview precedence.

Classes: refactor, fix. Feature owners: history-pagination, native-ui.

Loaded ephemeral threads still refuse includeTurns. A metadata-only response is neither a history snapshot nor mutation authority. Preserve actual ALab session/title ownership.

Canonical patch SHA256 `7e16a4f1b8b45388765628800eda45e28c5145d0f846be3144d2640814f942a0`, 12,783 bytes.

## 03. bc4a2544b8c02669446c111d6178f50524b7f52d — adopt

Resolve fork ancestry lazily for descending history pages (#61840)

Walk descending fork history only as far as the cursor, requested rows and next-page lookahead require, while retaining newer overrides and eager ancestry for ascending reads, update replay, anchors and inherited summaries.

Classes: refactor, fix. Feature owners: history-pagination, execution-continuity, qualification-fixtures.

An unused missing ancestor may be skipped, but an ancestor required by lookahead or summary still fails. Retain cycle detection, selected-rollout identity, cursor ownership and immutable ancestor reference rules.

Canonical patch SHA256 `345fb178b72a564c6e2a167ca5ea5c80e7d46b357f4be63e00450bd3ee6d0c0e`, 43,265 bytes.

## 04. 090be4df0724cdef5758e6a0934ba557af8b6bbe — adopt

Close late command approval items when turns are interrupted (#61841)

Complete a command item opened by a late approval as Declined when a turn transition cancels that approval, through the current thread listener/subscribers with its original turn ID and start timestamp.

Classes: fix. Feature owners: product-policy, execution-continuity, native-ui.

This repairs that late item completion only. PendingInterruptQueue remains an untagged ConnectionRequestId vector and terminal-event drains remain unconditional; the prior late-A/new-B interrupt acknowledgement candidate is not closed.

No client response grants new execution permission. Preserve actual declined completion, replacement-subscriber and native cancellation/terminal custody checks.

Canonical patch SHA256 `a46fd4f64f9e86c04228d299d23dd039d384ab0248a963920abce6eb13ed1288`, 17,288 bytes.

## 05. e0241d4fae4c87b406aacaf11d9ceb8548879873 — adopt

Defer code-mode tool catalog construction until first access (#61845)

Construct code-mode tools and ALL_TOOLS lazily on first access, retain writable cell-local catalogs and fixed callback slot order, and create data properties without inherited setter calls.

Classes: refactor, security. Feature owners: model-discovery, execution-continuity, product-policy.

The service normalizes metadata once into an Arc-backed immutable catalog; fresh V8 contexts retain per-cell mutation isolation. Preserve task/cancellation limits and policy-authorized dispatch; no new tool authority is implied.

Read final05+06 together rather than preserve obsolete intermediate ToolCatalog element types. No measured CPU/memory benefit is claimed.

Canonical patch SHA256 `9efa8fe26f98160d4ea776a500c32e708db18528bcb796019a146209c2190c70`, 28,352 bytes.

## 06. fef47a6769414444b1ce37aa8188b55bf2624f27 — adopt

Reuse code-mode tool catalogs across executions (#61846)

Negotiate session-local catalog revisions over stdio and gRPC, reuse only an acknowledged exact catalog, retain immutable per-cell snapshots, and share equal definitions through weak process-local indexes.

Classes: feature, protocol, refactor. Feature owners: model-discovery, execution-continuity, account-isolation.

Older hosts retain self-contained full requests. A client cache guard spans admission and publishes replacement only after accepted execution; cancellation leaves no acknowledged replacement, and concurrent admissions may send full catalogs.

Runtime rejects zero, unknown and same-revision conflicting snapshots. Late full requests retain their own tools without overwriting a newer cache. Client CatalogAdmission and ToolDefinition sharing use full ToolDefinition equality, including schemas and descriptions. The in-process runtime normalizes to EnabledToolMetadata (global name, tool name, description and kind); its revision conflict and metadata-sharing checks compare those normalized fields, without schemas.

Catalog revision, pointer equality and process-local sharing are neither account credentials nor permission grants. Preserve real remote host/session close, cancellation and ALab native execution ownership.

Canonical patch SHA256 `dc956a22cdb46a67577e1065af246fe949c771953465ad78f5b2bae85f0484d9`, 48,567 bytes.

## 07. effad7568c7945c524923e71f1919d64e7b614aa — keep-opt-in

Allow network policy to enable transparent TCP capture (#61851)

Retain configuration-requested transparent TCP capture behind explicit managed network policy, default false, Linux managed sandbox and enabled SOCKS5; require executor capability and preserve forced network enforcement.

Classes: feature, security. Feature owners: product-policy, execution-continuity, exec-handoff.

No ALab default or managed profile is changed. Remote RPC capture requires an executor-local proxy, unsupported/native launches refuse, and absent capability is not silently accepted.

Local prepared context is stripped outside Linux managed sandboxes; remote launch follows the executor OS and capability. Attachment policy can require capture but cannot remove inherited denials/review selectors.

Actual Linux /dev/net/tun, namespaces, proxy route, allowed/denied target and unavailable-capability controls remain required. This Mac source review cannot qualify the cfg-Linux path.

Canonical patch SHA256 `d45372886fdb906a4bc230318ef6f0f1f5ef8ae73a927865c27e9a574d0415ed`, 48,862 bytes.

Revisit: Before any ALab activation, qualify the actual supported executor/network/account/cancellation boundary with existing native policy and holds unchanged; keep default off and do not substitute source tests for live admission.

## 08. 35145aba212e923f987ccefaf9086ceba31e3b7c — adopt

Broadcast read-state changes to experimental clients (#61852)

Broadcast durable read-state changes to initialized experimental clients even when unsubscribed, preserving notification opt-out filtering and equality-token revision semantics.

Classes: feature, protocol. Feature owners: history-pagination, native-ui.

Reconnect still requires list refresh for missed changes. A broadcast is not a durable delivery receipt, thread subscription grant, cross-thread revision clock or write authorization.

Canonical patch SHA256 `a316f8797f7e7cdb5f9581498676bd908768da55caa32641d402d549df51ff4e`, 10,248 bytes.

## 09. 3257e1b3eb04a43a8721ad0e59470327807eb844 — adopt

Allow model metadata to control multi-agent tools in code mode (#61854)

Honor optional model multi_agent_tools_in_code_mode metadata while preserving runtime fallback when absent and the existing Direct/CodeMode/CodeModeOnly policy resolution.

Classes: model, protocol. Feature owners: model-discovery, product-policy, execution-continuity.

True selects ToolExposure::Direct availability, not unconditional direct visibility: effective tool_mode and exec-wrapper policy still choose actual direct/nested exposure. False retains DirectModelOnly; None preserves the existing non_code_mode_only setting.

No ALab defaults configure this field or tool_mode. Bounded owned-source searches find no ModelInfo literal requiring the added None field; revalidate real caller/metadata updates when integrating later main.

Use actual absent/null/false/true serialization and all tool-mode/runtime fallback matrix controls; do not infer model metadata can bypass feature, sandbox or native invocation admission.

Canonical patch SHA256 `24cfa5acb339d650d70f3e2718606e4f7498bd89f9e53e0331714edc70a59cf3`, 15,051 bytes.

## 10. 457a191479f15ce517d70e9a7bc4caf9ddd6d404 — keep-opt-in

Add opt-in tool-call attribution for upstream proxies (#61860)

Keep tool-call attribution disabled unless the executor explicitly designates a matching upstream proxy; strip guest-supplied IDs and send the controller ID only to that selected HTTP hop or outer HTTPS CONNECT.

Classes: feature, security, telemetry. Feature owners: product-policy, execution-continuity, account-isolation.

Do not set CODEX_TOOL_CALL_ATTRIBUTION_PROXY in ALab defaults or this review. Direct targets and other proxies receive no attribution; endpoint protocol/address equality, not origin port, selects the hop.

The designated proxy must consume and strip this internal correlation header. Correlation is not identity, permission, usage accounting, receipt ownership or proof; actual network policy/TLS/account binding remains authoritative.

Canonical patch SHA256 `e1c984f00e5e311d6e9456983a174a2e6fd71f53f0ecb2805e3293726733b924`, 21,891 bytes.

Revisit: Before any ALab activation, qualify the actual supported executor/network/account/cancellation boundary with existing native policy and holds unchanged; keep default off and do not substitute source tests for live admission.

## 11. 237d8b104ed06ae46c5ed9a54693672dea21856c — adopt

Report remote plugin local versions from matching cached packages (#61865)

Report remote plugin local_version only from the materialized active package with matching remote identity receipt and manifest, and preserve the original receipt when conflicting same-version replacement download fails.

Classes: fix, security. Feature owners: skill-discovery, account-isolation.

Filesystem package reads move to spawn_blocking. The existing no-receipt migration remains explicit; names/catalog versions alone do not establish identity.

Share/search responses without a matching local package leave local_version unset. Retain enabled-policy/interstitial and actual account-owned package validation; a version display is not install or execution authority.

Canonical patch SHA256 `0d8fe36cc8bd691c684cdbf8f4bc973d2b97a71d60dd6da009ec3181b6411d38`, 28,017 bytes.

## 12. b8cbdbfeff72869e5d715480572aad24f739aa65 — adopt

Preserve SQLite row metadata in thread listings (#61866)

Project direct SQLite listing rows in collected order without losing each occurrence title, section position or entry time, and retain legacy indexed-name precedence and best-effort rollout/search enrichment.

Classes: fix, refactor. Feature owners: history-pagination, native-ui.

Repeated rows may legitimately differ during pagination refill and keep their individual metadata. Project/section/parent filters remain enforced in the direct SQLite query; the removed final project retain is not an authorization deletion.

DB-only indexed-name read failures still report errors; ordinary rollout/search enrichment remains best effort. Preserve actual pagination/cursor and session history identity.

Canonical patch SHA256 `90f9c61935dfd1bf965b47677dfe7955f4568d0c3226587d106b0a46107e305c`, 19,978 bytes.

## Source interactions and unresolved boundaries

The bounded fixed-main ALab caller census finds no direct literal or helper caller
for the searched ModelInfo, network/exec wire types, removed metadata helper or
catalog ToolDefinition boundary. Both ALab defaults omit multi_agent/tool_mode,
transparent_tcp and attribution settings. No demonstrated ALab source adaptation
or default activation is required by that census. This does not prove arbitrary
downstream clients compile, and every later main composition must be checked.

Code-mode05–06 must integrate as a cohort: the initial Arc catalog becomes ordered
Arc references to immutable per-tool metadata. V8 lazy properties stay writable
and cell-local; callback indices refer to each cell's original snapshot. Client
try-lock guard spans actual admission; a cancelled replacement never becomes an
acknowledged client cache. Hosts may retain a received revision before a later
admission fails, so the client must still re-send a full newer snapshot. Host
revision validation, older-host fallback, session binding, cancellation and close
are distinct from content-sharing equality. No measured allocation/latency claim
or new capability to invoke tools is inferred from this source architecture.

Transparent TCP07 remains false by default and opt-in under managed network policy.
Linux sandbox selection, SOCKS5 enablement, executor-local listener, remote
capability, inherited denials/review domains and native protected profile remain
authoritative. Actual supported Linux command/TUN/namespace/route/denial/refusal
tests remain required; this Mac's source review does not execute cfg-Linux cases.
Attribution10 is separately opt-in through an executor-owned environment setting,
which this change does not set. Guest correlation headers are stripped, and only
the designated selected upstream hop receives the controller ID; that proxy must
consume/strip it. Correlation never proves account identity, request authority,
Goal accounting or a result receipt. No SourceV1/full-capsule/all-root receiver,
permission override, native environment or managed profile is activated.

The new model metadata09 does not force tools into both routes. Its true branch
sets availability to ToolExposure::Direct, while `is_directly_visible` and
`is_hidden_by_code_mode_only` still apply effective tool_mode and nested-tool
classification. `code_mode_tools` requires CodeMode/CodeModeOnly plus allowed exec
wrapper policy before returning available tools. False stays DirectModelOnly;
omission preserves runtime fallback. The one source matrix declaration loops over
12 metadata/runtime/mode combinations, not twelve generated libtest declarations.

Late approval04 closes an item opened after its original turn already ended and
uses the current listener's subscribers, original turn and original start time.
It does not change the untagged PendingInterruptQueue or unconditional old
terminal-event drains. The inherited schedule with pending B interrupt followed
by late A terminal remains a source-derived premature-ACK candidate, requiring an
actual deterministic control before any isolation claim. Read-state08 broadcasts
are scoped by initialized experimental capability and opt-outs, and reconnect
needs a list refresh; notification delivery is not an acknowledged durable receipt.

Thread listing12 preserves the actual collected SQLite row on each occurrence,
including repeated IDs whose update time/section position differs. Project filters
are still passed to `list_threads_db` and select that path. Ordinary rollout/search
names remain best-effort, while DB-only indexed-name errors still fail. Plugin11
requires matching remote receipt and manifest for a displayed local version;
no-receipt migration is retained explicitly and conflicting/unreadable identity
requires a replacement. Failed download preserves the prior receipt, not a claim
that a new package was installed. Native account/source/install authority remains
separate from displayed metadata.

## Inherited obligations remain open

The preceding e583 intake's obligations are preserved: original decoded terminal
Err/EOF custody across PostModelRequest hooks and steering; actual TurnAborted
priority; native exactly-once completion, accepted-input/root/parent/history and
local/remote exec-transfer custody; history/notes Some(step) and explicit-empty
catalog coverage; Guardian default-off account/network/cancellation/write and
nonforeground Goal/budget qualification; generic/auth/quota and natural execution
late-origin authority; stale GoalTool success versus valid retained-reset behavior;
policy/EOF final decisions and ordinary/Compact/upgrade issuance; and all explicit
held receiver/native permission limits. Earlier source-only review or a separately
bound ordinary component result is not a current-main/application qualification.
This cohort does not close those findings, implement the proposed terminal carrier,
activate Guardian, or replace native usage and receipt ownership.

The Trust application gate remains separately refused at unsupported required
build-script-execution verification; no unchanged certification retry, pin change,
proof exclusion or release-hook restoration is justified by these metadata files.

## Candidate tests and runtime limits

There are 32 statically checked declarations across ten actual package manifests,
representing 34 declared cases with the four explicit test_case labels in two
macro rows. These are function substring filters, not asserted exact generated
libtest IDs. Run every listed macro variant, retain actual nonempty inventory,
skip conditions, platform and footer/result facts. No test below ran for this
review. Module/manifest facts are textual source registration, not runtime listing.

| Row | Package / target / function filter | Declared variants | Source and intended scope |
|---|---|---|---|
| 1 | `codex-thread-store --lib live_thread_does_not_derive_metadata_from_inherited_items` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/mod.rs:1165` — Persisted filtered parent/child ordinals and cold replay, not inherited metadata leakage. **not_run** |
| 2 | `codex-app-server --test all thread_read_loaded_reads_stored_title_and_requested_history_without_rollout_path` | single declaration | `b8cbdbfeff72:codex-rs/app-server/tests/suite/v2/thread_read.rs:1008` — One metadata read and trimmed title, with requested history through the actual store. **not_run** |
| 3 | `codex-thread-store --lib newest_indexed_pages_do_not_read_unused_ancestors` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/thread_history/read_tests.rs:35` — Missing unused ancestor versus required exact-boundary lookahead. **not_run** |
| 3 | `codex-thread-store --lib descending_pages_stop_in_the_parent_even_when_resuming_there` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/thread_history/read_tests.rs:112` — Parent-resumed cursor preserves newer overrides; inherited summary still resolves required ancestry. **not_run** |
| 3 | `codex-thread-store --lib list_items_update_ordinals_use_selected_rollout_id` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/thread_history/read_tests.rs:1199` — Descending selected-rollout item identity and update ordering. **not_run** |
| 4 | `codex-app-server --lib late_command_cancellation_reaches_the_current_subscriber` | single declaration | `b8cbdbfeff72:codex-rs/app-server/src/request_processors/thread_listener_shutdown_tests.rs:97` — Late completion goes to the replacement subscriber. **not_run** |
| 4 | `codex-app-server --test all turn_interrupt_closes_late_command_approval_from_completed_turn` | single declaration | `b8cbdbfeff72:codex-rs/app-server/tests/suite/v2/turn_interrupt.rs:370` — Original-turn Declined command completion after a later interrupt; not the untagged interrupt-drain gap. **not_run** |
| 5 | `codex-code-mode-runtime --lib catalog_globals_preserve_mutations_within_each_cell` | single declaration | `b8cbdbfeff72:codex-rs/code-mode-runtime/src/service_tests.rs:1424` — Catalog mutations persist within a cell and reset across cells. **not_run** |
| 5 | `codex-code-mode-runtime --lib catalog_globals_ignore_inherited_setters` | single declaration | `b8cbdbfeff72:codex-rs/code-mode-runtime/src/service_tests.rs:1462` — Object/Array inherited setters are not invoked constructing catalog values. **not_run** |
| 6 | `codex-code-mode-runtime --lib catalog_revisions_freeze_each_cells_tools_and_isolate_sessions` | single declaration | `b8cbdbfeff72:codex-rs/code-mode-runtime/src/service_tests.rs:551` — Snapshot lifetime, unknown/conflicting revision refusal, session isolation and equal metadata sharing. **not_run** |
| 6 | `codex-code-mode --lib references_require_an_acknowledged_matching_admission` | single declaration | `b8cbdbfeff72:codex-rs/code-mode/src/tool_catalog_tests.rs:9` — Abandoned admission cannot become acknowledged reuse; changed catalog invalidates the snapshot. **not_run** |
| 6 | `codex-code-mode-host --lib supported_capabilities_are_negotiated_when_optional_or_required` | single declaration | `b8cbdbfeff72:codex-rs/code-mode-host/src/host_tests.rs:287` — Actual optional/required capability negotiation, including catalog revisions; loop inside one declaration. **not_run** |
| 7 | `codex-core --lib switching_profiles_updates_live_transparent_tcp` | single declaration | `b8cbdbfeff72:codex-rs/core/src/session/tests.rs:1334` — Live permission-profile proxy refresh; no managed Linux command qualification on this Mac. **not_run** |
| 7 | `codex-core --lib exec_server_params_use_path_uri_and_env_policy_overlay_contract` | single declaration | `b8cbdbfeff72:codex-rs/core/src/unified_exec/process_manager_tests.rs:179` — Local cfg-Linux versus remote managed capture propagation; unsandboxed requests strip capture. **not_run** |
| 7 | `codex-exec-server --lib proxy_config_selects_transparent_tcp_without_executor_switch` | single declaration | `b8cbdbfeff72:codex-rs/exec-server/src/process_sandbox_tests.rs:283` — cfg(target_os=linux): managed proxy capture and refusal of native or controller-supplied RPC capture. **not_run** |
| 8 | `codex-app-server --test all read_state_changes_reach_unsubscribed_experimental_clients` | single declaration | `b8cbdbfeff72:codex-rs/app-server/tests/suite/v2/thread_read_state.rs:40` — Actual experimental observer receives changes; stable and opted-out sockets do not. **not_run** |
| 8 | `codex-app-server --test all read_state_methods_require_experimental_capability` | single declaration | `b8cbdbfeff72:codex-rs/app-server/tests/suite/v2/thread_read_state.rs:120` — Read-state methods still require experimental capability. **not_run** |
| 9 | `codex-core --lib multi_agent_catalog_code_mode_follows_tool_mode_and_preserves_runtime_fallback` | single declaration | `b8cbdbfeff72:codex-rs/core/src/tools/spec_plan_tests.rs:3148` — One declaration loops over four metadata/runtime choices and all three tool modes. **not_run** |
| 9 | `codex-protocol --lib model_info_preserves_multi_agent_code_mode_presence` | single declaration | `b8cbdbfeff72:codex-rs/protocol/src/openai_models.rs:1957` — Absent/null/false/true metadata roundtrip. **not_run** |
| 9 | `codex-core --test all catalog_namespace_prefixes_follow_the_selected_model` | code_mode_only, code_mode_with_direct_tools | `b8cbdbfeff72:codex-rs/core/tests/suite/scenarios_indirect_namespace_prefixes.rs:30` — Both explicit macro cases; skip_if_no_network and skip_if_wine_exec may skip bodies. **not_run** |
| 10 | `codex-network-proxy --lib inbound_connect_attributes_the_selected_upstream` | single declaration | `b8cbdbfeff72:codex-rs/network-proxy/src/http_proxy.rs:1838` — Real inbound CONNECT tunnel child captures only controller attribution at designated upstream. **not_run** |
| 10 | `codex-network-proxy --lib tool_call_header_is_scoped_to_the_configured_upstream_and_execution` | single declaration | `b8cbdbfeff72:codex-rs/network-proxy/src/upstream_tests.rs:208` — HTTP and outer HTTPS CONNECT use distinct actual execution IDs. **not_run** |
| 10 | `codex-network-proxy --lib direct_and_other_proxy_requests_strip_tool_call_headers` | single declaration | `b8cbdbfeff72:codex-rs/network-proxy/src/upstream_tests.rs:241` — Direct, disabled and other-proxy requests strip guest attribution. **not_run** |
| 10 | `codex-network-proxy --lib split_upstreams_attribute_only_the_selected_hop_regardless_of_origin_port` | single declaration | `b8cbdbfeff72:codex-rs/network-proxy/src/upstream_tests.rs:276` — Selected proxy scheme, not unusual origin port, controls attribution. **not_run** |
| 11 | `codex-core-plugins --lib local_remote_plugin_version_requires_a_materialized_package_and_matching_identity` | single declaration | `b8cbdbfeff72:codex-rs/core-plugins/src/manager_tests.rs:137` — Manifest and exact remote identity receipt required for local version. **not_run** |
| 11 | `codex-core-plugins --lib sync_same_version_conflicting_identity_preserves_receipt_after_download_failure` | single declaration | `b8cbdbfeff72:codex-rs/core-plugins/src/remote/remote_installed_plugin_sync.rs:640` — Failed TLS replacement keeps original receipt and manifest; no external service. **not_run** |
| 11 | `codex-app-server --test all plugin_installed_prefers_remote_curated_conflicts_when_remote_plugin_enabled` | without default, default disabled | `b8cbdbfeff72:codex-rs/app-server/tests/suite/v2/plugin_list.rs:285` — Both declared default-policy cases preserve catalog/local version distinction. **not_run** |
| 11 | `codex-app-server --test all plugin_share_list_returns_created_workspace_plugins` | single declaration | `b8cbdbfeff72:codex-rs/app-server/tests/suite/v2/plugin_share.rs:533` — Unmaterialized remote share has no claimed local version. **not_run** |
| 12 | `codex-thread-store --lib duplicate_legacy_rows_keep_the_indexed_name` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/list_threads_tests.rs:20` — Repeated legacy rows retain distinct update/section position and indexed name. **not_run** |
| 12 | `codex-thread-store --lib list_threads_preserves_sqlite_title_search_results` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/list_threads.rs:473` — Direct SQLite title search retains row title and preview. **not_run** |
| 12 | `codex-thread-store --lib section_listing_uses_sqlite_metadata_without_reading_rollouts` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/list_threads.rs:755` — Recent/section results retain section position and entry time. **not_run** |
| 12 | `codex-thread-store --lib list_threads_returns_local_rollout_summary` | single declaration | `b8cbdbfeff72:codex-rs/thread-store/src/local/list_threads.rs:692` — Ordinary listing keeps indexed-name fallback when state DB unavailable. **not_run** |

The table is a bounded candidate set, not a claim to enumerate every inherited
test touched mechanically by added fields. Keep full owned/pristine gate coverage
when integrating. Missing supported Linux capture/refusal/native host policy,
cross-account catalog/cancellation/remote close, old-terminal/new-interrupt and
inherited native Goal/terminal/receiver controls remain mandatory where applicable.

## Provenance

The external packet `~/.ubd/codex-upstream-next12-aftere583-20261117/`
retains immutable canonical patches/index and supplied managed-fetch observation,
source blob identities, actual declarations/attributes/package/target/module
facts, bounded fixed-main caller census and maintained intake/202-commit coverage
and source-ownership checks. The root prepared only the initial pending snapshot
and canonical/fetch packet; this author independently read all twelve patches and
made the final decisions. Only this review and
`alab/upstream/b8cbdbfeff-from-e5839cbc59-intake.json` enter the source commit.
All compilation/runtime/admission/import/proof/publication status for this review
is UNRUN or not performed, regardless of names, helper metadata or cached labels.
