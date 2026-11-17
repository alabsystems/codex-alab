<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Source intake: twelve commits after a11d, through 4f556

Exactly twelve contiguous upstream commits are reviewed: `a11d2403a5c8f346527379ccd061c2aa95e7edc4..4f556fdd6db3f614207bacc037b1bc4452e301bf`. The fixed ALab comparison is `50017324104c25b9e20f334703fc9292a8bcb6d4`. Parent supplied a managed upstream observation `a3fc418fb29d15fff9df779575aee3a5ca1ea73d`; this review used cached Git objects and performs no fetch or claim of newer remote state. Later cached origin observations are checked for metadata overlap independently of the fixed semantic comparison.

Decisions: **10 adopt, 1 keep-opt-in, 1 adapt**. All twelve canonical patches were independently serialized with `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA`, matched byte-for-byte and read in full, including production, tests, snapshots and manifests: **435,085 bytes / 10,636 lines**. This commit changes only this review and its intake JSON. It imports no upstream source and runs no Rust, application, compiler, native admission, build, lint or formatter. Every candidate is **not_run / UNRUN**, **final_binding=null**; source strings, tests and schema checks are not runtime, Send, stack, performance, hardware, durability or shipping proof.

The actual import frontier and private integration candidates remain separate. Prior required/unimplemented 81/92/f226 terminal Err/EOF and exactly-once tool completion, a394/598/e51 execution/root/source transfer, rollout-budget versus native Goal budget, original tool/request/Empty/error authority, policy issuance and retained usage/cancellation obligations remain OPEN. See the [after336 review](upstream-intake-2026-11-16-after336.md), [after5cb review](upstream-intake-2026-11-16-after5cb.md) and [preceding afterb8cb review](upstream-intake-2026-11-17-afterb8cb.md). Isolated repairs are not presumed present in comparison main or qualified by this record.

ALab packaged/aterm defaults, apps/plugins opt-in, real account/provider credentials, native cwd/root/parent/FD leases and held SourceV1/all-root receiver routes remain unchanged. Item-boundary interrupts stay off; no held route is enabled to obtain evidence. No existing ALab hook is demonstrated equivalent or authorized for removal. No new SQL migration/index numbering is introduced. Upstream dependency changes require normal maintained resolution later, never manual upstream lock entries.

## Decisions

| # | Commit | Decision | Subject |
|---|---|---|---|
| 1 | `b08fd8a6f175` | adopt | Filter thread listings before scanning rollout previews (#61902) |
| 2 | `837f8b50be09` | adopt | Pass a per-call voice session ID through TUI realtime startup (#61908) |
| 3 | `cff061fca9f8` | keep-opt-in | Add opt-in item-boundary interrupts for subagents (#61909) |
| 4 | `c9df641eac76` | adopt | Preserve native sandbox executable paths and PTY dispatch names (#61910) |
| 5 | `56e43660f8c8` | adopt | Default Statsig metrics exports to Zstandard compression (#61911) |
| 6 | `30be9534c8c3` | adopt | Fall back to the built-in status line for invalid providers (#61915) |
| 7 | `f8e58064fccf` | adopt | Preserve remote compaction summaries when interrupting child agents (#61916) |
| 8 | `a669d8ffb230` | adapt | Add sidebar image previews and present generated images after answers (#61919) |
| 9 | `fd05a402b294` | adopt | Track conversation files for the TUI sidebar (#61920) |
| 10 | `42248f5ef988` | adopt | Add conversation files to the TUI sidebar chooser (#61922) |
| 11 | `885a1df5ee5f` | adopt | Allow agent search by thread ID and Git branch (#61926) |
| 12 | `4f556fdd6db3` | adopt | Encapsulate app-server thread-list locking in `ThreadListState` (#61928) |

## 1. b08fd8a6f175 — adopt

Apply thread source/provider/cwd/attached filters to the first canonical session metadata before scanning rollout preview text.

Canonical SHA256 `12e96494bc30f08c2f1c19598e7bd92806344a5a6f95cacc409ccff187d5700f` (6,609 bytes / 175 lines).

Primary source at exact final cohort:

- `codex-rs/rollout/src/list.rs:958` — `struct SummaryFilters`.
- `codex-rs/rollout/src/list.rs:987` — `async fn read_head_summary`.

Integration requirements:

- Keep the first canonical SessionMeta as the filter identity; copied fork metadata later in a rollout cannot reclassify its owning source/provider/workspace. Preserve attached-thread selection and existing ordering/pagination.
- A filtered rollout may stop before preview extraction. Opening/decoding and buffered read-ahead still occur; this is not a zero-I/O or measured performance claim. No migration or index numbering is changed.

Coverage limits:

- The new real-file control supplies canonical Exec then copied Cli metadata and verifies interactive exclusion; it does not benchmark disk behavior or prove native admission.

## 2. 837f8b50be09 — adopt

Generate one voice-session UUID per call and pass that same value through the actual call header and app-server start request.

Canonical SHA256 `cd8c9cbd0c6b55017d44a4cbf3898e1815c65361cec02b79cdef2848ccc2abc8` (6,356 bytes / 142 lines).

Primary source at exact final cohort:

- `codex-rs/tui/src/realtime_call.rs:71` — `voice-session-id`.
- `codex-rs/tui/src/app_server_session/realtime.rs:24` — `voice_session_id`.

Integration requirements:

- The UUID is distinct from ThreadId and stays with its call through asynchronous result delivery. Keep existing call-attempt/backend ownership, cancellation and selected remote voice preference.
- No microphone, voice, Cloud, apps/plugins or credential default is activated. A correlation UUID is not account, Goal, native-source or execution authority. No owned direct thread_realtime_start caller or CreatedCall literal was found.

Coverage limits:

- The actual remote voice test checks both header and RPC UUID; its helper loops are one declaration, not additional macro cases. Hardware/audio-provider qualification remains unrun.

## 3. cff061fca9f8 — keep-opt-in

Retain optional V2 child interruption at one model-item boundary, with immediate tool/MCP cancellation and exact retained-item cleanup, behind its default-false configuration.

Canonical SHA256 `47c6e37adc51421879c83609d1331c033cefbbc387f4540b6dd5d0bcf7271258` (45,956 bytes / 1,084 lines).

Primary source at exact final cohort:

- `codex-rs/core/src/session/sampling_interrupt.rs:27` — `pub(crate) fn enabled`.
- `codex-rs/core/src/session/turn.rs:3030` — `let interrupt_at_boundary`.
- `codex-rs/core/src/tasks/mod.rs:660` — `pub(crate) fn interrupt_task`.

Integration requirements:

- interrupt_at_item_boundary stays false by default and only applies to V2 children with a real parent. V1/ordinary interrupt retains hard cancellation; no held SourceV1/all-root receiver route is enabled to obtain a control.
- Preserve separate sampling/tool/MCP cancellation, actual cell cancellation feature checks, between-item ordering, undispatched-tool aborted result, finished-token wait and no automatic idle restart. A retained model item is data, never proof that its tool dispatched or permission to run it.
- Reapply actual ALab assignment/task-launch/input-mailbox and goal-completion hooks through take_active_turn/abort_taken_tasks and cancellation. Drain/join real task/tool completion before source retirement; do not infer cancellation implies rollback. No direct owned MultiAgentV2Config, RunningTask/ActiveTurn constructor, interrupt_task or interrupt_agent caller was found at fixed main.
- Preserve original request/step Cause across interruption/recovery and keep real usage/current budget independent. The prior 81/92/f226 terminal-custody and retained native Goal obligations remain open; this source decision does not import private candidate repairs.

Coverage limits:

- The two message/tool macro cases run real stream/child/settings/resume/history paths but may skip without network. No executed cancellation/Goal/native acceptance evidence is claimed. The outer run_sampling_request signature is unchanged; owned outer helpers do not require an inferred ninth argument.

Revisit: Before enabling item-boundary interrupts for ALab children or portability/recovery routes, run both actual retained-item cases plus ordinary interrupt, held SyncPost Err/EOF steering, actual tool dispatch/completion/cancellation, retained Goal usage and source-handoff controls under the admitted native build; require fail-closed unsupported recovery without enabling held routes.

## 4. c9df641eac76 — adopt

Preserve the exact native sandbox executable separately from display argv and dispatch arg0 through pipe, inherited-terminal and PTY execution.

Canonical SHA256 `653585444277ff0170bc2db6aa650f92904883e35d8f6957fae68dc25ac63559` (47,865 bytes / 1,222 lines).

Primary source at exact final cohort:

- `codex-rs/sandboxing/src/manager.rs:106` — `pub native_program`.
- `codex-rs/sandboxing/src/spawn.rs:33` — `pub struct SpawnRequest`.
- `codex-rs/utils/pty/src/pty.rs:332` — `fn portable_launch_paths`.

Integration requirements:

- Move native_program through sandbox transforms and actual command preparation, use the native OsStr as the executable, and treat arg0 solely as explicit dispatch identity. Windows paths remain explicit UTF8 requirements rather than lossy authority substitutions.
- PTY dispatch with explicit arg0 uses the native launcher even without inherited FDs. Preserve controlling-terminal/FD custody and the existing native wrappers, admission descriptors, source cwd/root/parent and exec-tree leases. Child HOME/PATH resolution follows the actual supplied environment/cwd; do not grant a root from a display path.
- No owned direct SpawnRequest/SandboxExecRequest/CoreExecRequest/native PTY constructor caller was found in bounded ALab Rust sources. Upstream nix/Cargo.lock changes require the maintained resolver during eventual integration, never manual upstream lock edits.

Coverage limits:

- Actual native/nonUTF8 helper tests are platform scoped. completed_local_commands_preserve_exit_code has pipe plus a Linux-only cfg_attr test_case, not two unconditional cases. Existing controlling-terminal zero-fork checks are conditional on helper availability. No latency/portability/native permission qualification is implied.

## 5. 56e43660f8c8 — adopt

Use Zstd for the Statsig OTLP metrics default only when none of the four documented compression/header environment overrides is present.

Canonical SHA256 `1a6c3883a840ab3104c845aac6024b353f97cf69b5e2199508f981e802bccad5` (4,386 bytes / 109 lines).

Primary source at exact final cohort:

- `codex-rs/otel/src/metrics/client.rs:25` — `OTEL_EXPORTER_OTLP_METRICS_COMPRESSION`.
- `codex-rs/otel/src/metrics/client.rs:684` — `Compression::Zstd`.

Integration requirements:

- Presence via var_os, including empty or nonUTF8 values, of OTEL_EXPORTER_OTLP_METRICS_COMPRESSION, OTEL_EXPORTER_OTLP_COMPRESSION, OTEL_EXPORTER_OTLP_METRICS_HEADERS or OTEL_EXPORTER_OTLP_HEADERS preserves SDK-controlled behavior. Keep the existing exporter consent, Statsig headers and managed HTTP client.
- Compression changes neither telemetry consent nor event contents, credential source or ALab provider/plugin/apps defaults. Preserve exact dependency resolution without editing upstream lock entries.

Coverage limits:

- No new focused test declaration covers this default or the four presence overrides. Existing exporter consent and real loopback temporality tests are selected controls only; add/execute an actual compression/header override matrix before claiming that branch qualified. No bandwidth/CPU measurement.

## 6. 30be9534c8c3 — adopt

Fall back to builtin status rendering for a malformed winning provider while retaining strict validation and original diagnostic config layers.

Canonical SHA256 `c6b0d51417b08d3965371d670ccda948775d86673d178a4b6b854087d06ca745` (9,925 bytes / 242 lines).

Primary source at exact final cohort:

- `codex-rs/config/src/config_layer_source.rs:66` — `fn effective_values`.
- `codex-rs/config/src/strict_config.rs:231` — `fn status_line_provider_warning`.

Integration requirements:

- Keep the actual layer precedence, complete plugin/skill identity validation and source restrictions. Normal loading strips invalid effective provider data; strict mode still rejects malformed ignored/project/packaged values and the unmodified layer remains available to diagnostics.
- Builtin fallback does not bootstrap a plugin, activate cloud catalogs or widen approvals. Preserve ALab native display/clipboard/title and actual settings authority. No direct owned new provider helper/caller was found.

Coverage limits:

- Incomplete identities are a loop in one actual declaration. Config fallback is not a runtime provider, plugin or approval authorization test.

## 7. f8e58064fccf — adopt

Retain an actually installed remote compaction summary and an already finished answer when optional child item-boundary interruption arrives.

Canonical SHA256 `89b8f2b474ef52291da9dc6e3e04cd63633fe979e2c732bc8c22e41c7f35f77a` (28,753 bytes / 632 lines).

Primary source at exact final cohort:

- `codex-rs/core/src/compact_remote_v2.rs:250` — `SamplingInterrupt::unregister`.
- `codex-rs/core/src/session/turn.rs:728` — `CompactionPhase::PostTurn`.

Integration requirements:

- Keep sampling-interrupt registration alive until the real summary is installed; only then observe the request and skip the post-compaction hook. Failed request cleanup unregisters correctly. Already finished answer remains the real answer, and queued user input remains a later native turn.
- The feature gate from row3 remains off. Summary/answer retention grants no Source, EmptyResponse, Goal mutation or new request authority. Preserve original causal A for pre-next/Guardian compaction and explicit post-turn Goal denial; real fresh B must follow actual native admission.
- Keep ALab mailbox/accepted-input/history/compaction metadata, real usage/budget, task completion and retained native Goal source ordering. Prior request/Empty and terminal Err/EOF custody gaps remain open.

Coverage limits:

- One actual network-skippable declaration loops mid/post-turn cases with gates, settings ACK, resumed IDs/history and installed summary; source result retention is not native policy or durability proof.

## 8. a669d8ffb230 — adapt

Add bounded image decoding and generated-image/sidebar rendering while adapting ALab terminal startup handoff to preserve the exact new graphics capability.

Canonical SHA256 `4e30b93d7763cc5a8fc71a873b330382c174c34adbd9ebc729addf9a066c4632` (107,117 bytes / 2,616 lines).

Primary source at exact final cohort:

- `codex-rs/tui/src/sidebar_image.rs:56` — `pub(crate) fn decode`.
- `codex-rs/tui/src/terminal_probe/startup_replay.rs:8` — `fn parse_startup_input`.
- `codex-rs/tui/src/history_cell/patches.rs:323` — `struct PendingGeneratedImages`.

Integration requirements:

- Required owned adaptation: alab-tui/glue/upgrade_startup.rs:67 builds InitializedTerminal and must supply herdr_kitty_graphics. Capture/strict Snapshot/restore/finish must carry the actual Option<bool> fact, preserving true/false/unknown without requerying the terminal, inferring from environment or silently blessing missing transfer data. The existing Snapshot denies unknown fields, accepts version==1 and requires explicit optional-field presence; choose a reviewed schema/version/preflight treatment and update the actual startup-exec fixture together; retain no-query and partial-paste/native-owner oracles.
- Preserve the upgrade-input hook when startup_replay_input becomes parse_startup_input: feed its parsed replay bytes to codex_alab_tui::input::buffer_input, not directly to a separate crossterm owner. Consume only the actual probe reply; bracketed paste remains user input. The new probe is specifically unmultiplexed term_program=herdr, not an ALab aterm alias.
- Keep saved/completed-image and actual turn ordering across pagination/interruption and the original backend/source/request view for sidebar reads. Preserve validated WorkspaceFile/TrustedFile and copy authority; no frontend-local fallback for remote source.
- Decoder allocation limit is 32 MiB, supported formats PNG/JPEG/GIF/WebP and sidebar retained raster at most 768x768; transcript source file gate 32 MiB is separate from remote server reads. Keep Sixel/tmux placement budgets and terminal image cleanup on screen/overlay/exec changes. There is no explicit total PendingGeneratedImages count cap or server read-size proof.
- At fixed500 comparison, no owned CapturedSideDisplay/ImagePlacement/InterruptionNotice literal or removed new_interruption_notice caller was found. Generated image metadata and terminal probe responses do not authorize tools, Goal/Empty policy, receiver roots or native admission.

Coverage limits:

- Captured Kitty byte counters prove fixture output only, not actual terminal/device compatibility or speed. The inherited document-preview test explicitly uses a 16 MiB test thread and is not normal-depth/Send proof. Actual ALab startup-exec tri-state preservation requires an adapted real test; current source fixture does not test the new field.

## 9. fd05a402b294 — adopt

Track file destinations for each owning conversation and scan older item pages independently of transcript history without accepting stale backend/request completions.

Canonical SHA256 `2b247fb997a294470d583058a70836eabe8b2ce9bef208e287ceca21286ae398` (47,115 bytes / 1,188 lines).

Primary source at exact final cohort:

- `codex-rs/tui/src/app/sidebar_files.rs:17` — `pub(super) struct Files`.
- `codex-rs/tui/src/app/sidebar_files.rs:437` — `fn start_sidebar_files_scan`.
- `codex-rs/tui/src/app/event_dispatch.rs:102` — `AppEvent::SidebarFilesLoaded`.

Integration requirements:

- Inventory only links/plan/user text, successful file changes excluding deletions, completed saved images and prior reader opens. Preserve typed voice/review hiding across page boundaries, source line/column metadata, newest-reference ordering and dedup by destination.
- Read only the owning source/backend after actual replay completes. Exact backend/parent/request IDs fence pages; hide/thread-switch/reconnect abort the active handle and invalidate stale completion. Keep a separate next cursor, retry failed pages on reopen and stop repeated cursors; do not advance transcript history.
- Pages request 100 items and input link paths reject empty/control/>4096-byte values. Local directory probes apply only to actual embedded/server-local paths; remote unresolved paths stay with the original reader. This inventory is not filesystem permission, no-follow containment, accepted input, native ownership or a Goal receipt.
- No total inventory/older_turn/opened_files cap is introduced; scanning older history may cost multiple reads/allocations. Do not claim bounded whole-conversation memory or zero I/O. Preserve backend account/source custody, existing app event FIFO, native history/mailbox and clipboard trust.

Coverage limits:

- Real embedded-server controls cover wrong displayed thread, hidden/reopened chooser, stale reconnect page and unchanged transcript cursor; pure inventory controls cover ordering/remote paths/review+voice. No live cross-account remote permissions or full-history memory measurement.

## 10. 42248f5ef988 — adopt

Display conversation files in the sidebar chooser with stable selection, separate wheel/path scrolling and exact press-release target validation.

Canonical SHA256 `65ea0b791597ab091d4fbb6b5ce39dacc3814a812b27a59e9779d41b8f5d5ae9` (117,182 bytes / 2,898 lines).

Primary source at exact final cohort:

- `codex-rs/tui/src/app/side_focus.rs:26` — `pub(super) struct SidebarPress`.
- `codex-rs/tui/src/app/sidebar_files.rs:297` — `ThreadParamsMode::Cloud`.
- `codex-rs/tui/src/sidebar_chooser.rs:419` — `pub(crate) fn navigate`.

Integration requirements:

- Files open through the original backend and displayed source thread; chooser opening clears location only for whole-file preview while transcript links retain coordinates. Cloud inventory stays unavailable; unsupported Windows/sandbox hosts display a reason and cannot activate preview rows.
- Match mouse down/up by parent, full sidebar input state, target, rectangle and button; reject drags, stale/reordered/scrolled targets and hidden selection. Selection follows destination across inventory reorder. File path display is presentation, not a local read or authority grant.
- Preserve ALab actual human-input grace (press, not release), overlay/modal holds, native thread/fork/close, backend bookkeeping and validated copy/PRIMARY semantics. Existing liveness control moves to agents_navigation_tests without behavior deletion. No owned ChatTabs/SidebarStyle/ConversationFrame literal or removed ScrollState/TabStrip::hit caller was found.

Coverage limits:

- Tests cover actual app-side press/release and inventory/path/scroll geometry. They do not qualify a physical mouse/terminal, remote credential switch, native approval or root receiver route.

## 11. 885a1df5ee5f — adopt

Include thread ID and recorded Git branch in case-normalized live and persisted agent search.

Canonical SHA256 `61f35f3f7d251f85df092df7d1c1d1935b53af14875f5ad94db05f7a02fd3be9` (4,942 bytes / 120 lines).

Primary source at exact final cohort:

- `codex-rs/tui/src/app/agents_overview_discovery.rs:271` — `pub(super) fn matches_query`.

Integration requirements:

- Retain current data source/account/backend and existing archived/read-only/interactive source filters and cursor semantics. Search matches recorded metadata only; it does not run Git, change branch, wake a thread or establish native identity.
- The mixed-source discovery wrapper verifies persisted branch/ID plus paginated previews; the live-center declaration verifies branch/ID/name editing behavior. No owned matches_query direct caller requires adaptation.

Coverage limits:

- Do not credit the Cloud-only interactive-page control with persisted legacy preview rejection; those checks are in the separate mixed-source discovery declaration. No search latency measurement.

## 12. 4f556fdd6db3 — adopt

Encapsulate the existing shared one-permit thread-list serialization state without changing acquisition ordering or error mapping.

Canonical SHA256 `1eff6f7293d24583f6a313e68e0df00068fea07f0c07e16c1c77eaf5ef124d25` (8,879 bytes / 208 lines).

Primary source at exact final cohort:

- `codex-rs/app-server/src/request_processors/thread_list_state.rs:8` — `pub(crate) struct ThreadListState`.
- `codex-rs/app-server/src/request_processors/thread_list_state.rs:21` — `pub(super) async fn acquire_exclusive`.

Integration requirements:

- Both ProjectRequestProcessor and ThreadRequestProcessor retain the same Arc<ThreadListState>; acquire_exclusive still uses one Semaphore permit and the same internal-error response. Default construction is shared, not one gate per request.
- Module/type/method visibility permits actual request-processor children and listener fixture; no owned ThreadListState constructor was found. Preserve ALab listener event ordering, retained recovery turn, actual source/goal metadata and account isolation. No later parallel-resume behavior is included.

Coverage limits:

- Selected existing listener tests use the changed fixture but add no new focused serialization/concurrency declaration. Encapsulation alone is not measured throughput or race qualification.

## Static candidate inventory

**44 upstream selector rows  / 44 declarations, eight packages and 61 actual module/include edges**, plus one existing ALab-owned declaration compiled inside `codex-tui --lib` at fixed comparison main. There are 46 cases from direct test attributes before platform filtering, plus one additional Linux-only `cfg_attr(test_case)` PTY case; platform cfg and network/helper skips remain prerequisites. Loop iterations, snapshot assertions and subprocess child invocations are not extra declarations. Source-attached manifest/target, attributes, parent cfg and exact bodies are retained in the external source packet. Filters below are declaration substring selectors, not claimed generated exact macro names. All are not_run/UNRUN, with null binding.

| Row | Package / target | Actual declaration | Scope |
|---|---|---|---|
| 1 | `codex-rollout --lib` | `tests::test_listing_filters_use_canonical_session_metadata` | Canonical first SessionMeta controls filtering despite copied fork metadata. |
| 1 | `codex-rollout --lib` | `tests::test_source_filter_excludes_non_matching_sessions` | Existing source filter remains restrictive. |
| 2 | `codex-tui --lib` | `app::tests::realtime_requests::remote_voice_start_routes_v3_offer_and_effective_preference` | Actual mock call HTTP header and app-server RPC retain one per-call UUID distinct from thread ID; helper loops are not new declarations. |
| 2 | `codex-tui --lib` | `app::tests::realtime_requests::voice_cancel_restart_only_attaches_the_new_attempt` | Existing cancellation/restart preserves current actual voice attempt. |
| 3 | `codex-core --test all` | `suite::agent_execution::agent_mailbox::interrupt_retains_one_sampled_item` | Two declared message/tool cases: actual interrupted child retains one sampled item, discards later output and resumes with real history; may skip without network. |
| 3 | `codex-core --lib` | `tools::handlers::multi_agents::tests::multi_agent_v2_interrupt_agent_accepts_task_name_target` | Actual V2 manager submission uses InterruptAgent rather than hard Interrupt. |
| 4 | `codex-core --lib` | `unified_exec::tests::completed_local_commands_preserve_exit_code` | Pipe plus Linux-only macro PTY/nonUTF8 helper retain actual exit17; conditional macro separately inventoried. |
| 4 | `codex-core --lib` | `unified_exec::process_manager::tests::exec_server_params_use_path_uri_and_env_policy_overlay_contract` | Environment/path parameters retain real exec-server policy overlay. |
| 4 | `codex-core --lib` | `exec::tests::exec_full_buffer_capture_ignores_expiration` | Real output drain remains independent from process expiration. |
| 4 | `codex-sandboxing --lib` | `manager::tests::inherited_terminal_preserves_native_linux_helper_and_uses_its_dispatch_name` | Unix actual native/nonUTF8 launcher direct+PTY execution preserves dispatch arg0; direct status.success() and PTY exit0/output are asserted. Exit17 belongs to the separate completed_local_commands_preserve_exit_code control. |
| 4 | `codex-sandboxing --lib` | `manager::tests::transform_linux_seccomp_uses_helper_alias_when_launcher_is_not_helper_path` | Existing transformation retains helper dispatch name. |
| 4 | `codex-utils-pty --lib` | `tests::pty_with_dispatch_name_uses_child_home_and_path` | Unix actual custom-arg0 PTY searches only child PATH/HOME and preserves native executable. |
| 4 | `codex-utils-pty --lib` | `tests::spawn_helper_tests::pty_helper_establishes_a_controlling_terminal_without_forking` | Existing real helper controlling terminal and helper-available fork accounting; availability condition is not unconditional zero-fork proof. |
| 5 | `codex-otel --lib` | `metrics::client::tests::counter_consent_uses_the_active_exporter` | Existing exporter consent remains necessary; does not cover new four-variable Zstd selection. |
| 5 | `codex-otel --test tests` | `suite::otlp_http_loopback::otlp_http_exporter_honors_cumulative_temporality_env` | Real loopback confirms metric exporter environment handling for temporality, not new compression override matrix. |
| 6 | `codex-tui --lib` | `local_settings::tests::malformed_status_line_provider_uses_project_and_user_validation_rules` | Winning invalid provider falls back normally, strict validation still rejects malformed project/user values. |
| 6 | `codex-tui --lib` | `local_settings::tests::status_line_provider_requires_a_complete_identity_in_the_selected_profile` | Incomplete identity loop retains builtin fallback plus strict errors; one declaration. |
| 6 | `codex-cli --test features` | `interactive_validates_config_before_requiring_terminal` | Actual strict CLI rejects malformed status provider before terminal initialization. |
| 7 | `codex-core --test all` | `suite::scenarios::compaction::interrupt_during_remote_compaction_preserves_summary_on_resume` | One network-dependent declaration loops mid/post-turn interruption; actual gate, setting ACK, summary installation and resumed requests; does not prove Goal authority. |
| 8 | `codex-tui --lib` | `app::history_pagination::tests::generated_image_from_an_older_page_follows_its_answer` | Older-page generated image moves after actual answer and before turn completion. |
| 8 | `codex-tui --lib` | `app::history_pagination::tests::resume_places_interruption_before_searches_split_across_pages` | Actual interruption turn metadata and pagination boundaries retained. |
| 8 | `codex-tui --lib` | `app::owned_transcript::tests::owned_details_keep_the_composer_cursor_and_screen` | Image protocol changes retain composer cursor/screen. |
| 8 | `codex-tui --lib` | `app::sidebar_document::tests::focused_markdown_link_opens_read_only_preview_only_with_workspace` | Actual embedded UI image/text/LFS fallback and overlay retention; upstream fixture uses explicit 16MiB test thread, not normal-depth proof. |
| 8 | `codex-tui --lib` | `chatwidget::tests::exec_flow::image_generation_call_adds_history_cell` | Generated image remains deferred until turn completion and follows final answer. |
| 8 | `codex-tui --lib` | `custom_terminal::tests::native_kitty_moves_an_image_without_resending_other_visible_images` | Captured terminal bytes retain uploaded IDs and move placement without retransmitting unchanged pixels; no hardware qualification. |
| 8 | `codex-tui --lib` | `history_cell::tests::image_generation_call_renders_saved_path` | Saved image adds document-only target; no terminal reader link when documents disabled. |
| 8 | `codex-tui --lib` | `terminal_probe::startup_replay::tests::herdr_graphics_reply_selects_previews_without_replaying_the_response_as_keys` | Exact probe OK/unsupported/no response consumed while bracketed paste remains input. |
| 8 | `codex-tui --lib` | `transcript_view::layout::tests::fullscreen_image_previews_follow_layout_without_entering_text_surfaces` | Generated image after answer, clipping/expansion, copy text stability and missing-file fallback. |
| 8 | `codex-tui --lib` | `transcript_view::search::tests::search_resumes_from_the_text_in_a_page_with_a_relocated_image` | Search continuation retains exact older text anchor after image relocation. |
| 9 | `codex-tui --lib` | `app::sidebar_files::tests::files_from_missed_pages_precede_older_cached_paths_and_reader_opens` | Reconnected missed/live/cache/opened ordering. |
| 9 | `codex-tui --lib` | `app::sidebar_files::tests::sidebar_files_keeps_remote_paths_for_the_owning_reader` | Remote unresolved destination retained with original source location; no local-directory probe. |
| 9 | `codex-tui --lib` | `app::sidebar_files::tests::sidebar_files_skip_local_directories_and_keep_extensionless_files` | Only authorized local host directory filtering, extensionless files retained. |
| 9 | `codex-tui --lib` | `app::sidebar_files::tests::sidebar_files_respect_page_boundaries_for_typed_voice_and_review_items` | Typed voice/review markers across pages hide private internal paths. |
| 9 | `codex-tui --lib` | `app::tests::pagination_completion_tests::closing_the_first_preview_populates_the_files_chooser` | Actual embedded app-server reader close refreshes inventory. |
| 9 | `codex-tui --lib` | `app::tests::pagination_completion_tests::sidebar_files_load_older_items_without_advancing_transcript_history` | Actual request/cursor, wrong-display suppression, hidden chooser and stale reconnect completions. |
| 10 | `codex-tui --lib` | `app::sidebar_files::tests::sidebar_files_chooser_matches_the_grouped_file_inventory` | Grouped files, deleted/directory/web omission, coordinate metadata, full-path scroll and row hit geometry. |
| 10 | `codex-tui --lib` | `app::sidebar_files::tests::sidebar_files_follow_latest_mentions_and_preserve_selection` | Selection retained by destination when inventory reordered. |
| 10 | `codex-tui --lib` | `app::sidebar_files::tests::sidebar_files_scrolls_the_body_without_wrapping_or_moving_the_wheel_selection` | Keyboard clamping, wheel independent viewport and hidden-row hit rejection. |
| 10 | `codex-tui --lib` | `app::tests::handle_start_side_seeds_navigation_before_thread_started` | Actual sidebar press/release, mismatched release rejection and native side-chat lifecycle. |
| 10 | `codex-tui --lib` | `app::tests::agents_navigation_tests::refresh_agent_picker_thread_liveness_prunes_closed_metadata_only_threads` | Existing real server liveness control moved module; no behavior deletion. |
| 11 | `codex-tui --lib` | `app::agents_overview::tests::command_center::live_center_search_row_appears_only_while_editing` | Live center matches case-normalized branch/thread ID/name and retains editing-only row. |
| 11 | `codex-tui --lib` | `app::agents_overview::tests::discovery::overview_show_more_refills_archived_rows_and_retries_without_losing_rows` | Actual persisted discovery branch/thread ID search plus paginated preview; wrapper selects mixed-source helper path. |
| 12 | `codex-app-server --lib` | `request_processors::thread_lifecycle::listener_shutdown_tests::late_command_cancellation_reaches_the_current_subscriber` | Existing listener cancellation path uses shared ThreadListState fixture. |
| 12 | `codex-app-server --lib` | `request_processors::thread_lifecycle::listener_shutdown_tests::listener_shutdown_releases_runtime_and_preserves_other_subscribers` | Existing actual listener shutdown preserves other subscriptions after state encapsulation. |
| 8 | `codex-tui --lib` | `tui::alab_upgrade_startup::tests::startup_exec_preserves_the_screen_without_queries_and_completes_the_paste` | Actual same-PID PTY startup capture/restore no-query and partial-paste control. Required tri-state graphics assertions must be added during adaptation; current control does not test new upstream field. |

Required runtime boundary: eventual integration must run applicable selected controls on actual admitted packages plus unchanged ALab startup-exec/input/terminal cleanup, actual source/account/native execution/Goal/cancellation and held SyncPost Err/EOF controls. The row5 four-variable compression/header matrix, row8 graphics transfer tri-state and relevant remote/platform boundaries need real coverage; current source candidates do not fill those gaps. No broad gap closes and no default activates from this intake.

## Separate newer-main compatibility observation

Newer-main compatibility at a4c6d583e9f0cd68e2168e2c364ee45e03e959dc (separate from fixed500 comparison): eight owned ImagePlacement literals now exist in retained composer/TUI image production and test paths. Add explicit destination-local terminal_id handling while preserving actual renderer/raster/unlink custody; never copy a source terminal ID as destination authority. Exact caller/blob and two current-main test declarations are retained separately. The ScrollState concern is refuted by the direct import in actual request_user_input parent. The current-main and fixed comparison share upstream merge-base `ce3031018ef130e653ed4d9c5bb971652de97161`; there are no alab/upstream path changes between them. No actual import count is inferred. The 941-path concurrent ALab delta was not whole-reviewed here.

Actual additional current-main candidates (both not_run/UNRUN, null binding):

- `codex-tui --lib transcript_image::composer::alab_retained_cache::tests::exact_native_cache_keeps_unavailable_non_utf8_pre_epoch_and_same_raster_alias` — Actual native cache codec retains same raster alias and explicit unavailable/nonUTF8/pre-epoch DATA; not a renderer/native authority grant.
- `codex-tui --lib retained_tui_images::tests::real_completed_local_write_and_clear_retire_only_after_same_frame_finishes` — Actual local terminal writer/flush and same-frame commit retain unlink owner until successful retirement; no hardware renderer qualification.

These two declarations are additional to the 44 upstream plus one fixed-main startup declarations. Their parent/module paths and source blobs are bound in the external packet; neither already covers the required new field. No row10 adaptation is inferred from the removed ScrollState re-export.

Further exact a4c6 row8 DATA obligations: retained_text_layout.rs captures/restores TextLayout and must retain the new image_left_aligned bit rather than defaulting or rerendering. retained_history_separator_cells.rs production/test literals and strict codec must preserve actual FinalMessageSeparator.turn_id (Some/None). retained_history_image_cells.rs and its fixture literal must preserve the new ImageGenerationHistoryCell.id in the actual generated-cell codec, never derive it from display lines or current state. The native history graph has a closed exact-variant codec and currently cannot encode the new InterruptionNotice; supported typed transfer must retain its original optional turn identity, or remain explicitly unsupported. No flattening, default identity, late recapture, source-root activation or graphics/native permission follows from these DATA fields. Pending generated-image state likewise cannot be silently discarded by an unsupported transfer. This is a bounded current-main codec/caller addendum, not a whole concurrent-main audit. All existing source selectors and decision counts are unchanged; new codec/field behavior still needs actual integrated tests.
