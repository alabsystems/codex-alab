<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream intake after 5cb38e79842a — source review, 2026-11-16

Exactly twelve contiguous cached commits after `5cb38e79842a0e0a6dca5bb97874163e8ee50129` through `aea4c6ee4b652d84b416731b11c2b8299d767209`. Fixed ALab comparison `290ce4685274b0099c36959a8493793b6f41022b`. Cached upstream cap observed as `d5a478e31c3296e544d53424a7a6c995e7dfb0f5`; this is a local cached observation, not a current remote claim. No fetch or actual upstream merge occurred.

Later managed-origin observation supplied by root is `1d8f34bca6a012a2cbc03680f7fa72659e323a5f`. Its 15 changed paths since the fixed comparison are maintained Python runtime/smoke/hash controls and reviews, SSE diagnostic fixture coverage, and the one-line owned chronology-lock diagnostic. All selected owned crosspoint, declaration, manifest, catalog and defaults paths in this review are unchanged. Preserve those newer maintained controls for any eventual exact-source qualification; this review neither rebases their evidence nor claims its older comparison is final. A fresh origin-record scan found no overlapping decision row for this twelve-commit range.

**9 adopt, 2 adapt, 1 keep-opt-in.** Only this review and its matching intake JSON are source changes. All test rows below are **not_run**. Source declarations, package manifests, target roots and registration chains were inspected; no generated runtime inventory, actual test body completion, native admission or qualification is claimed.

## Boundaries and inherited obligations

- Keep packaged apps/plugins/tool_suggest disabled, explicit model/provider/project/account choices and all existing SourceV1/full-capsule/all-root receiver holds. Explicit remote RPC operation does not activate native restoration.
- Preserve actual input/history/mail/result/callback/stdio ownership, roots/parents/source constructors and native goal mutation/accounting/lease custody. A client enum, bearer token, display cache, wire ID or terminal snapshot is not authority.
- The [after336 review](upstream-intake-2026-11-16-after336.md) requires **UNIMPLEMENTED** terminal Err/EOF custody at `81b995a6c9a4c978f399775cfa5ce50702368749`, its `92c1a4dc2e1ff83dd0d2f06f15f03df21b8cb2f2` dependency, and the owned completion/invocation adaptation at `f226c9ccfb9762eb5b8fa5d45b43262304f5a360`. The [afterc8 review](upstream-intake-2026-11-16-afterc8.md) additionally retains real budget-stop/native-goal and retained-usage control gaps. This batch implements or supersedes none of them.
- Pending unqualified ALab policy source commits `b2451d506fdc4f1dc894ef225d8913eaaf42f648` and `a437366676fb1647cca66ef65d926db72d620066` are not imported or treated as implemented. Their boundary is final one-shot API issuance, not retroactive Core queue cancellation. No new EOF/nonreplay fix is asserted by these UI/client changes.
- Tests with a real local server/PTY/Core component remain distinct from a signed native image/host transfer or a real remote service. Keep explicit package/binary/platform prerequisites; never enable held routes merely to obtain evidence.

## Canonical patches

Digests cover `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA`, including headers. Exact bytes are retained externally and checked against Git.

| Commit | Decision | Subject | SHA256 |
| --- | --- | --- | --- |
| `87fccdc1bc0b` | adopt | Defer plugin manifest loading until marketplace entries are selected (#61686) | `66c226252054e033a63dd971da9ae034367580011a86561f1a7e77f7df976014` |
| `375a6b1653ed` | adopt | Add TUI review ownership and risk guidance (#61688) | `28808cd4582c127a264e5e89c7d824fc52ca6401f337933858a03c8833294900` |
| `9017fd33f3b9` | adopt | Keep pasted text inline in TUI goals (#61690) | `36c319c6200771e93ffa62e2211405559e1dbe6cc2d1862c8b128aba96e0d3fb` |
| `03cb329415f3` | adopt | Fix status detail truncation for wide Unicode characters (#61692) | `590b75663f1cd48249c282ee51e252ae76fb743639a97d7387aa4f71d4099fd7` |
| `1a2a9299c1ae` | adopt | Discard unsaved experimental feature edits on cancel (#61694) | `590c870df3d62c3a000f5b51635cff7a2429a0994780f86df141a7100f1320df` |
| `a394ff9cf66e` | adapt | Restore the terminal's original alternate-scroll mode (#61697) | `8e1f3179701c5977ded2f75d7c808922101b679197970950f25b72016bbc7336` |
| `598df2d01585` | adapt | Refactor exec to use the shared app-server client (#61706) | `53aa7576b259a5e7ef2a6ddecf86d747e1d2f1bc4648dbd58944b5557e0471d8` |
| `e51c551fb406` | keep-opt-in | Support remote app servers in `codex exec` (#61707) | `becee86e527c1d1b1360ffa7f466edadeeaef11bda3d65a0fb5287a982072cd4` |
| `b048d4a95cac` | adopt | Avoid legacy transcript reads in the agents overview (#61709) | `7260239dac2a10f6976d9d064b82289bc2a628ff90640e540c84452285dc1c71` |
| `96e0673f6ec4` | adopt | Enable slash-command completion for unavailable threads (#61710) | `43b3c4866c54a89298d045feac6355af3511644e8dc8d8db082607738205f8b4` |
| `58d5d3a0404b` | adopt | Open Windows desktop sessions through the registered URL handler (#61711) | `9fd55aee0b659c62d69376f87fedc73ac07eecbaa9b16a70f4e9d116621f1fbe` |
| `aea4c6ee4b65` | adopt | Restore raw mode after returning from external editors (#61713) | `d50167f527ca153cabea00b2eb5117cb279bbb81d004f1fc8dd3e238fda0dda1` |

## 87fccdc1bc0b — adopt

Select validated marketplace entries by policy, visibility and first-source/scope precedence before reading their plugin manifests; retain selected metadata and installed-cache resolution.

Affected contracts: `product-policy`, `skill-discovery`, `qualification-fixtures`.

Required integration behavior:

- Preserve the first valid source claiming its plugin ID before product visibility, and the visible marketplace path claiming before cross-scope ID filtering. Do not revive a shadowed plugin from a later scope.
- Keep malformed selected-manifest warnings and raw marketplace errors, while avoiding metadata reads for discarded rows. This is a narrower read reduction, not zero filesystem I/O or a measured latency claim.
- Retain packaged apps=false, plugins=false and tool_suggest=false; preserve actual auth-selected curated catalog, local/global source isolation and MCP policy.

ALab interactions:

- MarketplaceSelection is private request-local path/ID state; validated entry loading retains ManifestCache and installed active-version metadata. No owned API, literal or direct caller of the replaced private helpers was found in the bounded ALab search.

Source test candidates (all not_run):

- `codex-core-plugins --lib list_marketplaces_skips_manifests_for_discarded_rows_without_changing_precedence` — `codex-rs/core-plugins/src/manager_tests.rs:392` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual scratch marketplace/config/read-warning control; explicit feature-on cases are isolated fixture setup, not a product default.
- `codex-core-plugins --lib list_marketplaces_returns_empty_when_feature_disabled` — `codex-rs/core-plugins/src/manager_tests.rs:4319` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual scratch marketplace/config/read-warning control; explicit feature-on cases are isolated fixture setup, not a product default.
- `codex-core-plugins --lib list_marketplaces_excludes_plugins_with_explicit_empty_products` — `codex-rs/core-plugins/src/manager_tests.rs:4364` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual scratch marketplace/config/read-warning control; explicit feature-on cases are isolated fixture setup, not a product default.

## 375a6b1653ed — adopt

Retain upstream TUI ownership and risk-review metadata; it changes neither runtime behavior nor ALab authority or autonomous publication rules.

Affected contracts: `native-ui`, `qualification-fixtures`.

Required integration behavior:

- Preserve the OWNERS and TEMPEST_REVIEW files as upstream review metadata. They are not executable proof, an installed permission grant or a new ALab human-approval workflow.
- Continue respecting applicable project instructions and source review of input, cancellation, notification ordering and old-server compatibility.

ALab interactions:

- Only two documentation/ownership paths change. No Rust, executable, build target, dependency or ALab configuration changes.

Required coverage or platform limits:

- No executable regression is introduced or needed for this documentation-only row; source/path review is its evidence and makes no runtime claim.

## 9017fd33f3b9 — adopt

Expand actual pending pasted goal text before image processing and length checks, retaining short text inline and materializing oversized Unicode objectives through the existing managed objective file.

Affected contracts: `native-ui`, `goal-storage-recovery`.

Required integration behavior:

- Preserve text-element offsets after paste expansion, stale placeholder handling, copied image attachments and explicit replacement confirmation.
- Keep app-server-host file selection and bounded goal-length behavior. Goal objective presentation/materialization does not itself ACK native GoalService effects or authorize a new turn.

ALab interactions:

- The change is inside upstream goal_files and its actual app test. No owned GoalDraft literal or materialize_goal_draft caller was found; ALab native goal persistence, mutation chronology and source leases remain separate.

Source test candidates (all not_run):

- `codex-tui --lib set_thread_goal_draft_materializes_long_objective_and_confirms_before_paste` — `codex-rs/tui/src/app/tests.rs:9372` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual app-server-backed goal draft/inline paste/image/Unicode materialization and editor snapshot control; not a native mutation receipt.

## 03cb329415f3 — adopt

Truncate the entire final status-detail line by terminal display width, including indentation and ellipsis, instead of counting characters in its last span.

Affected contracts: `native-ui`.

Required integration behavior:

- Retain line/grapheme width rules, styles and existing detail line limits; do not infer task, usage or goal state from shortened display text.

ALab interactions:

- Uses existing line_truncation helpers. No owned status widget literal or changed API call was found.

Source test candidates (all not_run):

- `codex-tui --lib details_overflow_adds_ellipsis` — `codex-rs/tui/src/status_indicator_widget.rs:526` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Rendering width and wide-Unicode snapshot control; no native authority or execution semantics.

## 1a2a9299c1ae — adopt

Make cancel discard uncommitted experimental toggles, including after a failed write or remapped cancel key, while accept retains the existing save path.

Affected contracts: `native-ui`, `product-policy`.

Required integration behavior:

- Preserve acceptance writes and failed-write retry semantics; Esc/Ctrl+C/remapped cancel must not emit a new SaveExperimentalFeatures or special-control update.
- Cancellation of unsaved UI edits is not rollback of a previously successful server write. Keep packaged/explicit feature policy and ALab stash/keymap behavior unchanged.

ALab interactions:

- Only upstream popup/input behavior and snapshots change; no ALab caller of the modified private popup method was found.

Source test candidates (all not_run):

- `codex-tui --lib experimental_features_discovery_preserves_server_order_and_accepts_new_keys` — `codex-rs/tui/src/bottom_pane/experimental_features_view_tests.rs:90` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual popup event channels, failed writes and cancel without retry.
- `codex-tui --lib experimental_features_empty_error_and_cancel_remain_usable` — `codex-rs/tui/src/bottom_pane/experimental_features_view_tests.rs:202` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Empty/error popup usability and discard control.
- `codex-tui --lib experimental_features_cancel_discards_toggle` — `codex-rs/tui/src/chatwidget/tests/popups_and_settings.rs:3153` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual remapped cancel leaves the event channel empty.

## a394ff9cf66e — adapt

Adopt actual DEC1007 observation/restoration and write-failure retry, and extend ALab terminal handoff state so the original scroll mode and pending restoration survive native replacement.

Affected contracts: `native-ui`, `exec-handoff`, `build-and-adoption`, `qualification-fixtures`.

Required integration behavior:

- REQUIRED owned adaptation: extend strict alab-tui::upgrade_startup::Screen plus glue/upgrade_screen.rs checkpoint/adopt with the actual initial_scroll_enabled and scroll_dirty booleans; update the owned explicit JSON fixture. Do not silently default a transferred unknown state or re-probe the already altered terminal.
- Restored startup returns before the ordinary probe. Without this adaptation the new static AlternateScreen has false/false and cannot restore an initially enabled source mode. This is a source-derived integration gap, not an executed handoff failure.
- Keep strict old/new reader behavior explicit: missing required state or unknown new fields must refuse rather than admit guessed restoration facts. Terminal data grants no SDK/execution authority.
- Retain bounded startup response filtering, queued typeahead/paste framing, both initial scroll states and cleanup retry after write failure; preserve native raw-mode and screen/input ownership.

ALab interactions:

- Owned glue/upgrade_screen.rs currently captures seven old fields; strict Screen in alab-tui/src/upgrade_startup.rs has neither new field. tui::init ALAB restore returns before the ordinary query, and glue/upgrade_startup.rs adopts the snapshot directly. The existing hook can carry the additional data without a new broad upstream hook.

Required coverage or platform limits:

- Existing same-PID startup/raw-mode controls do not yet exercise both original DEC1007 states through the new fields, strict old/new decoding or pending scroll restoration after native handoff. Extend actual controls before qualification; vector-writer tests alone are not native exec evidence.

Source test candidates (all not_run):

- `codex-tui --lib startup_probe_parses_batched_terminal_responses` — `codex-rs/tui/src/terminal_probe.rs:694` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Unix startup-probe parser/typeahead control; no actual terminal query execution claim.
- `codex-tui --lib startup_probe_finds_responses_after_large_typeahead` — `codex-rs/tui/src/terminal_probe.rs:782` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Unix startup-probe parser/typeahead control; no actual terminal query execution claim.
- `codex-tui --lib startup_replay_preserves_osc_looking_bracketed_paste` — `codex-rs/tui/src/terminal_probe/startup_replay_tests.rs:26` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Pure bounded replay/framing control; exact recognized terminal reports are intentionally filtered even when interleaved with paste.
- `codex-tui --lib startup_replay_omits_terminal_color_replies_interleaved_with_bracketed_paste` — `codex-rs/tui/src/terminal_probe/startup_replay_tests.rs:35` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Pure bounded replay/framing control; exact recognized terminal reports are intentionally filtered even when interleaved with paste.
- `codex-tui --lib alternate_scroll_is_restored_across_handoff_and_exit` — `codex-rs/tui/src/tui/alternate_screen_tests.rs:102` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Vector-writer terminal state model; not a native terminal handoff.
- `codex-tui --lib cleanup_write_failure_does_not_pop_the_wrong_screen` — `codex-rs/tui/src/tui/alternate_screen_tests.rs:250` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Vector-writer terminal state model; not a native terminal handoff.
- `codex-tui --lib startup_exec_preserves_the_screen_without_queries_and_completes_the_paste` — `codex-rs/alab-tui/glue/upgrade_startup_tests.rs:16` at `290ce4685274b0099c36959a8493793b6f41022b`. Owned private controlling-PTY/same-PID unit-executable exec. Must be extended for original scroll states; not a signed package/image admission.
- `codex-alab-tui --lib native_exec_retains_raw_input_and_restores_the_source_original_mode` — `codex-rs/alab-tui/src/terminal_modes_tests.rs:35` at `290ce4685274b0099c36959a8493793b6f41022b`. Owned actual PTY/same-PID original termios and input ownership; not a complete app upgrade proof.

## 598df2d01585 — adapt

Move exec to the shared AppServerClient while retaining ALab local native capabilities and the single original event/output owner across extracted finish_turn.

Affected contracts: `exec-handoff`, `native-session`, `account-isolation`, `provider-recovery`, `goal-storage-recovery`, `build-and-adoption`, `execution-continuity`, `qualification-fixtures`.

Required integration behavior:

- REQUIRED owned adaptation: select/extract the real InProcess client/request handle for Controller::start and Recovery::{resume,recover}. Shared request_handle now returns an enum; only the local variant has ALab bootstrap_exec/activate_exec_tree. Do not emulate these typed local operations with remote JSON-RPC resume/start.
- REQUIRED owned adaptation: retain the local handoff/recovery context through finish_turn, including controller, recovered error_seen, recovered-start suppression, original turn/thread, renderer, worktree/configuration and interrupt receiver. Keep biased drain, interruption refusal, EOF handoff completion and successful-original-turn canary ordering.
- Keep startup ticket/grant/capsule validation before managed checkout mutation and before stdin/fresh prompt parsing. Preserve original root/parent/source, accepted input/mailboxes/history, stdio and callback/result ownership, real goal/native epoch/lease boundaries, reviewer and ephemeral memory state.
- Keep same-PID/actual source admission and sealed captures; the shared client enum grants no native transfer authority. Full SourceV1/all-root receiver stays held. Preserve existing boxed top-level exec dispatch and body/lifecycle laziness.
- Mechanical owned fixture adaptation: exec_title_tests.rs create_with_ansi must pass Some(&config), retaining its real local report/version/fixture-prompt oracle. Do not weaken the local case to None.

ALab interactions:

- Controller::start takes InProcessAppServerRequestHandle; Recovery::resume/recover and its activation path take &InProcessAppServerClient. Owned exec_suspend_facade implements local-only request variants on those types. Upstream new finish_turn has no ALab recovery context; reapply intent in owned adapters with thin marked hooks.
- Remote address parsing/token extraction moves from TUI/CLI into app-server-client, with TUI re-exports. Explicit ports, wss or loopback-ws bearer restriction, Unix socket semantics and trim/missing/empty refusal stay intact. No owned call to removed remote_addr_supports_auth_token or additional EventProcessor implementation was found.

Required coverage or platform limits:

- An integrated actual local event-pump/shared-client handoff regression is still required after adaptation. Existing local Core/app-server recovery controls do not prove the new extracted event owner, signed native admission, retained complete forest or disabled held route.
- Inherited REQUIRED and UNIMPLEMENTED 81/92 terminal Err/EOF custody, f226 goal/completion hooks and real budget-stop/native-goal complements remain required; shared-client completion events do not supersede them.

Source test candidates (all not_run):

- `codex-app-server-client --lib resolve_remote_addr_accepts_websocket_url` — `codex-rs/app-server-client/src/remote_addr_tests.rs:8` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib resolve_remote_addr_accepts_secure_websocket_url` — `codex-rs/app-server-client/src/remote_addr_tests.rs:19` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib resolve_remote_addr_accepts_default_socket` — `codex-rs/app-server-client/src/remote_addr_tests.rs:30` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib resolve_remote_addr_accepts_relative_socket_path` — `codex-rs/app-server-client/src/remote_addr_tests.rs:42` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib resolve_remote_addr_accepts_absolute_socket_path` — `codex-rs/app-server-client/src/remote_addr_tests.rs:53` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib resolve_remote_addr_rejects_invalid_remote_addresses` — `codex-rs/app-server-client/src/remote_addr_tests.rs:66` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib read_remote_auth_token_from_env_var_reports_missing_values` — `codex-rs/app-server-client/src/remote_addr_tests.rs:82` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib read_remote_auth_token_from_env_var_trims_values` — `codex-rs/app-server-client/src/remote_addr_tests.rs:91` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-app-server-client --lib read_remote_auth_token_from_env_var_rejects_empty_values` — `codex-rs/app-server-client/src/remote_addr_tests.rs:100` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Moved endpoint/token unit contract; no remote connection, credential value disclosure or native execution.
- `codex-exec --lib the_exec_report_opens_with_codex_alab_s_own_version` — `codex-rs/alab-core/glue/exec_title_tests.rs:32` at `290ce4685274b0099c36959a8493793b6f41022b`. Owned actual human renderer report in isolated identity children; Some(config) adaptation required.
- `codex-app-server --lib exec_recovery_preserves_original_settings_and_requires_live_admission` — `codex-rs/alab-core/glue/exec_recovery_admission_tests.rs:30` at `290ce4685274b0099c36959a8493793b6f41022b`. Actual Core/app-server local recovery component; fixture admission callback is not a native host/image receipt.
- `codex-app-server --lib ephemeral_exec_recovery_restores_original_turn_and_settings_without_storage` — `codex-rs/alab-core/glue/exec_recovery_admission_tests.rs:44` at `290ce4685274b0099c36959a8493793b6f41022b`. Actual memory-backed local recovery component; not remote or held all-root admission.

## e51c551fb406 — keep-opt-in

Retain explicit remote exec using the authenticated server configuration/credentials and normal RPC lifecycle; keep it separate from local ALab native recovery and disabled receiver routes.

Affected contracts: `exec-handoff`, `native-session`, `account-isolation`, `provider-recovery`, `product-policy`, `execution-continuity`, `qualification-fixtures`.

Required integration behavior:

- Keep explicit --remote opt-in, endpoint conflict refusal and the wss/loopback token policy. Do not select a daemon/remote by a new ALab default or migrate local credential/config authority to the server.
- Preserve rejection of unsupported local model/config/project/worktree/source/schema/image/permission flags and UUID-only resume. Server-owned configuration and normal Never approval requests are not an ALab native human-hold bypass or local claim proof.
- Depend on the 598 local-only adapter. A pending inherited local recovery ticket/capsule must not silently enter the early remote branch and issue fresh work; reject that incompatible routing before remote startup. The current source does not prove that an ordinary admitted local replacement can legitimately change its original argv to --remote; this is an explicit compatibility/activation obligation, not a reproduced loss.
- Keep local BoundOutput file ownership and output IDs separate from remote roots/stdio. Actual thread/turn events correlate server work; they are not native source/root/goal epoch/lease receipts or portable checkpoint capabilities.
- Treat disconnect/EOF as failure before completion, not proof of remote cancellation or rollback. Thread.status Active rejection is a preflight snapshot, not an exclusive lease over a later TurnStart; do not claim it solves concurrent remote ownership.

ALab interactions:

- remote::run returns before ordinary local config and ALab Recovery::load. It has no local bootstrap/activate capability. The existing CLI source/native constructor hooks still precede dispatch, but neither a bearer token nor a remote completion can replace typed local grant/capsule validation. No owned exhaustive ExecCli literal or EventProcessor implementation requiring the new fields/method was found.

Required coverage or platform limits:

- Need exact integrated remote active-thread/disconnect/unsupported-option and local-custody incompatibility controls before any broader ALab activation. The existing workload-identity test is a real local server/CLI/model fixture with server-only test credential, not a real remote service or native exec-transfer qualification. Do not enable SourceV1/all-root restoration to obtain evidence.

Revisit: Before enabling any ALab remote-native recovery/transfer, implement and execute the local-only compatibility guard and actual remote ownership/error controls with real supported admissions; explicit user remote operation does not activate held SourceV1/all-root routes.

Source test candidates (all not_run):

- `codex-cli --test queue remote_session_commands_with_workload_identity_use_server_auth` — `codex-rs/cli/tests/queue.rs:263` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Unix actual CLI/app-server/loopback model and server-only test auth; requires a completed codex binary and actual body execution.
- `codex-cli --test queue remote_exec_rejects_conflicting_endpoints` — `codex-rs/cli/tests/queue.rs:636` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual CLI process validation before missing token lookup; completed codex binary prerequisite.
- `codex-cli --bin codex exec_help_documents_cyber_access_program` — `codex-rs/cli/src/exec_args_tests.rs:8` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual clap help declaration including remote help; not remote execution.

## b048d4a95cac — adopt

Avoid legacy rollout replay for overview previews, using paginated latest-turn reads only and preserving observed live prompt/reply data across refresh, revert and closure.

Affected contracts: `native-ui`, `history-pagination`.

Required integration behavior:

- Keep backend/request/thread identity and stale-read guards, paginated source items, live legacy prompt precedence, and observed last reply after close.
- Missing preview, NotLoaded status or an empty metadata result is not deletion, stopped work, receipt settlement or recovery admission. No general performance/latency improvement is measured here.

ALab interactions:

- Private overview activity visibility widens to sibling use; no owned literal/caller of the changed type or helper was found. ALab retained history/input and goal metadata remain distinct from display caches.

Source test candidates (all not_run):

- `codex-tui --lib overview_cloud_discovery_uses_only_interactive_pages` — `codex-rs/tui/src/app/agents_overview_discovery_tests.rs:22` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Cloud request-channel ThreadList source-kind, cursor and returned-page ordering through next_batch.
- `codex-tui --lib overview_show_more_refills_archived_rows_and_retries_without_losing_rows` — `codex-rs/tui/src/app/agents_overview_discovery_tests.rs:66` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Local WebSocket discovery helper checks archived pages, cursor retry, retained rows, paginated previews and refusal of legacy turn-list reads.
- `codex-tui --lib shared_overview_keeps_rows_and_replays_changes_over_stale_reads` — `codex-rs/tui/src/app/agents_overview_tests.rs:735` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual UI cache/revert/notification ordering control.
- `codex-tui --lib agents_overview_details_render_markdown` — `codex-rs/tui/src/app/agents_overview_tests.rs:1432` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual latest-reply retention after ThreadClosed plus markdown rendering.

## 96e0673f6ec4 — adopt

Permit restricted unavailable-thread slash completion for eligible recovery/local commands while preserving argument validation, blocked commands, custom keys and Vim search/escape behavior.

Affected contracts: `native-ui`.

Required integration behavior:

- Keep normal and unavailable input modes distinct; the restricted route validates selected builtin eligibility, single-line input and arguments before invoking popup handling.
- Preserve pasted-command expansion without dispatch, disabled direct input and ALab prompt-stash/history key priority. Completing a recovery command is UI dispatch, not fresh model, native claim or goal admission.

ALab interactions:

- No owned direct caller of the changed private handle_key_event_with_slash_popup signature was found. No ALab activation or permission check is removed.

Source test candidates (all not_run):

- `codex-tui --lib unavailable_thread_dispatches_recovery_and_local_commands` — `codex-rs/tui/src/bottom_pane/chat_composer/reconnect_tests.rs:180` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual restricted composer/key/result control; not backend/native admission.
- `codex-tui --lib unavailable_thread_accepts_highlighted_recovery_completion` — `codex-rs/tui/src/bottom_pane/chat_composer/reconnect_tests.rs:211` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual restricted composer/key/result control; not backend/native admission.
- `codex-tui --lib unavailable_thread_vim_escape_and_search_keep_popup_closed` — `codex-rs/tui/src/bottom_pane/chat_composer/reconnect_tests.rs:278` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual restricted composer/key/result control; not backend/native admission.
- `codex-tui --lib unavailable_thread_expands_pasted_commands_without_dispatching` — `codex-rs/tui/src/bottom_pane/chat_composer/reconnect_tests.rs:385` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Actual restricted composer/key/result control; not backend/native admission.

## 58d5d3a0404b — adopt

Use the Windows registered codex URL handler after checking one of the two supported package publisher IDs, rather than starting a packaged internal executable path.

Affected contracts: `native-ui`, `product-policy`.

Required integration behavior:

- Retain existing URL quoting and launch failure reporting. A publisher/package presence check followed by Start-Process URL dispatch is not proof of which executable ultimately owns the registered handler.
- Do not install or launch a desktop application for source review; preserve ALab local/optional presentation and native source identity independently.

ALab interactions:

- Only the Windows PowerShell script generator changes. No new test declaration or ALab direct call to this private helper was found; non-Windows source cannot qualify the actual registered handler.

Required coverage or platform limits:

- No named source regression specifically validates this script/registered-handler change. Actual supported Windows package/handler and rejection controls are required on an eligible machine; this Mac/source review provides no such evidence.

## aea4c6ee4b65 — adopt

Reset the stale raw-mode cache after external programs and reapply terminal input modes, retaining ALab native original-termios ownership.

Affected contracts: `native-ui`, `exec-handoff`, `build-and-adoption`, `qualification-fixtures`.

Required integration behavior:

- Keep the Unix-only reset before set_modes and existing error handling; editor cancellation or exit does not permit input consumer overlap or native handoff admission.
- Preserve ALab disable_raw_mode/enable_raw_mode imports and native original termios wrapper. The new call resolves through that owned wrapper; do not replace it with direct crossterm calls or remove native ownership as redundant.
- Retain actual canonical/echo-off and edited-draft/screen assertions under both editor presentation modes. Editor/terminal restoration is not SDK, source, goal or request authority.

ALab interactions:

- tui.rs already imports the ALab native disable/enable wrappers. native::disable restores and retires the retained original; the next enable captures the restored original and clears stale crossterm state. No additional production adaptation is identified beyond the separately required a394 screen-state carry.

Source test candidates (all not_run):

- `codex-tui --test all external_editors_keep_the_screen_and_return_the_draft` — `codex-rs/tui/tests/suite/external_editor_tests.rs:12` at `aea4c6ee4b652d84b416731b11c2b8299d767209`. Unix actual controlling PTY/editor subprocess, stty sane and tcgetattr oracle; included via suite::focus_palette::external_editor. Requires completed codex-tui or codex binary; no editor launched by this review.
- `codex-alab-tui --lib native_exec_retains_raw_input_and_restores_the_source_original_mode` — `codex-rs/alab-tui/src/terminal_modes_tests.rs:35` at `290ce4685274b0099c36959a8493793b6f41022b`. Existing actual PTY/same-PID native original mode control; not signed package/native image qualification.

## Source-check scope

There are 41 selector rows, 40 distinct declarations and 7 packages. CLI clap tests use `--bin codex`; CLI queue tests use the auto-discovered `--test queue`; the Unix editor PTY test is included under `suite::focus_palette::external_editor` in `--test all`. Owned recovery tests are hosted by `codex-app-server --lib` through MessageProcessor's local suspend module, not by codex-core. The owned renderer case is hosted by codex-exec, and native mode/startup controls by their actual ALab/TUI hosts.

These are source candidates and scoped controls, not a completed feature bundle. In particular the new DEC1007 state must be added to actual same-PID handoff controls; the extracted shared-client exec pump needs an integrated local ownership test; remote active/disconnect/inherited-local-custody checks and Windows registered-handler execution remain explicit gaps. Existing config parser/metadata and vector-writer controls cannot stand in for those runtime boundaries.

No Rust/compiler/native/application/formatter/lint run, pin/lock/profile/cache/SDK/default change, source implementation, actual merge, install, admission or publication occurred. All execution statuses remain UNRUN/not_run and final_binding is null. Complete intake coverage validates source decision bookkeeping only.
