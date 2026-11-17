<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Source intake: twelve commits after b8cb, through a11d

Exactly twelve linear upstream commits are reviewed here: `b8cbdbfeff72869e5d715480572aad24f739aa65..a11d2403a5c8f346527379ccd061c2aa95e7edc4`. The fixed ALab comparison is `38ecb45641fd91023dd07628dc645a130b6456e0`. The upstream head `c826ca128699b5464724c8757638ae79f51a45f2` was observed by the managed fetch at `2026-11-17T05:46:04.016587+00:00`; no newer remote observation is claimed.

The decision is **11 adopt, 1 keep-opt-in**. Every retained canonical patch was independently reserialized with `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA` and read in full, including production, tests, snapshots and manifests: **499,516 bytes / 11,579 lines**. This changes only this review and the matching intake JSON. It imports no upstream source and runs no Rust, application, compiler, native admission, build, lint or formatter. All test candidates are **not_run / UNRUN**, with **final_binding = null**. No runtime, Send, stack, performance, certification or shipping claim follows.

The actual source-import frontier remains separate from the reviewed frontier and private integration previews. These decisions do not resolve prior required/unimplemented adaptations, including the original terminal-outcome custody at `81b995a6c9a4c978f399775cfa5ce50702368749` with dependent `92c1a4dc2e1ff83dd0d2f06f15f03df21b8cb2f2` / `f226c9ccfb9762eb5b8fa5d45b43262304f5a360`, rollout-budget/native-Goal separation, native policy issuance, request/error provenance, caller cancellation and source transfer. The prior [after336 review](upstream-intake-2026-11-16-after336.md) and [preceding aftere583 review](upstream-intake-2026-11-17-aftere583.md) remain independently binding. Pending policy/origin fixes in isolated candidates are not presumed to be in comparison main.

ALab packaged and aterm defaults remain unchanged: explicit model selection, disabled apps/plugins, existing native authority gates, credentials, bounded account/provider recovery and user-held SourceV1/all-root receiver routes retain their contracts. No SDK, toolchain, profile, pin or lock changes are part of this intake. No existing ALab hook is shown equivalent to these changes; none is authorized for removal.

## Decisions

| # | Commit | Decision | Subject |
|---|---|---|---|
| 1 | `77873dd0d284` | adopt | Preserve plugin skill picker ranking across version changes (#61871) |
| 2 | `dba22f2acc3b` | adopt | Support flat subgraphs in native Mermaid flowcharts (#61878) |
| 3 | `d5a478e31c32` | adopt | Preserve local Markdown document targets in transcript rendering (#61880) |
| 4 | `59d8c797896c` | adopt | Add app-server file preview read helpers (#61887) |
| 5 | `be5db96264e2` | adopt | Add read-only document previews to the TUI sidebar (#61888) |
| 6 | `7c3f5e2cc290` | adopt | Add search, selection, and link navigation to document previews (#61889) |
| 7 | `3a51865c8cca` | adopt | Open source file links at line and column targets in the TUI (#61890) |
| 8 | `c0d47262f33f` | adopt | Acknowledge agent messages and clarify how to wake idle recipients (#61894) |
| 9 | `ef6ce8100a91` | adopt | Bound rollout trace capture with configurable storage limits (#61896) |
| 10 | `ce7079b50637` | keep-opt-in | Add opt-in UTC dates to environment context (#61897) |
| 11 | `641b4cac88d0` | adopt | Tie Linux sandbox proxy bridges to the launcher lifetime (#61898) |
| 12 | `a11d2403a5c8` | adopt | Fix proc mount fallback detection for Bubblewrap 0.12 (#61900) |

## 1. 77873dd0d284 — adopt

Aggregate confirmed plugin-skill usage by marketplace/plugin/relative skill identity across cache versions while retaining ordinary path matching and match-tier priority.

Canonical SHA256: `5704a6bc8f93407dd2e9e4abab41bdb8115b9ea1e5d4f00111d591270b1fb67a` (9,715 bytes / 233 lines).

Primary source, at the exact final cohort:

- `codex-rs/tui/src/bottom_pane/skill_popup.rs:133` — `fn plugin_skill_key`.
- `codex-rs/tui/src/bottom_pane/mention_usage.rs:98` — `fn codex_home`.

Integration obligations:

- MentionUsage supplies its actual storage-derived home, configured and canonical plugin roots; only normal marketplace/plugin/version components ending in SKILL.md are normalized. Canonicalization is local picker construction work, not a catalog/network activation.
- Keep ALab disabled plugin/apps defaults, actual package identity and installation policy. Frequency is ranking data, not execution permission. No owned with_frequencies caller or literal needs adaptation at fixed comparison.

Coverage limits:

- Actual persisted old-version/symlink selection is tested by the composer declaration; the separate match-tier control sets frequencies directly and does not prove the new normalizer. No CPU or filesystem-latency measurement.

## 2. dba22f2acc3b — adopt

Parse and render bounded flat Mermaid subgraphs in all four flow directions while preserving unsupported-source and TooWide fallback.

Canonical SHA256: `71986022693dbbe6d136bdcba7343fb8d76e87eb4569f62e4ba92d1e8eb3d4b4` (31,245 bytes / 657 lines).

Primary source, at the exact final cohort:

- `codex-rs/mermaid/src/lib.rs:20` — `const MAX_SOURCE`.
- `codex-rs/mermaid/src/parse.rs:16` — `pub(super) fn parse`.
- `codex-rs/mermaid/src/draw.rs:17` — `pub(super) fn render`.

Integration obligations:

- Reject nested groups, local direction overrides, group-ID edges, empty/duplicate/unclosed groups; retain existing graph/node/edge/label caps. Quoted titles may contain otherwise meaningful syntax.
- No dependency, execution, rendering-default or ALab hook removal is needed. Grouping and external lanes remain presentation only.

Coverage limits:

- The four directions and invalid-input list are loops within two declarations, not additional declared cases. Source bounding is not a measured renderer cost.

## 3. d5a478e31c32 — adopt

Preserve inert local Markdown document targets and copy/link geometry through transcript and narrow-table rendering, enabling interaction only for workspace readers.

Canonical SHA256: `54cef4c44e2cef96b04a7f11b62425e9dc995f9be52139e63d73743c5599b3f7` (36,810 bytes / 842 lines).

Primary source, at the exact final cohort:

- `codex-rs/tui/src/markdown_render/local_links.rs:48` — `fn sidebar_file_link`.
- `codex-rs/tui/src/terminal_hyperlinks.rs:218` — `fn reader_destination`.
- `codex-rs/tui/src/history_cell/spoken_artifacts.rs:65` — `is_document`.

Integration obligations:

- Document destinations do not become terminal OSC-8 destinations by default. Preserve actual validated WorkspaceFile/TrustedFile precedence and existing web links, combining/emoji geometry and POSIX literal backslashes.
- Deleted width::char_width has no owned caller at fixed comparison; no owned TerminalHyperlink literal or TextLayout link_at caller was found. No existing ALab artifact or native permission hook is equivalent to this UI metadata addition.

Coverage limits:

- Target syntax is not a filesystem proof. Resolution and reads are introduced separately in rows 4–7; no containment/no-follow guarantee follows from a parsed path.

## 4. 59d8c797896c — adopt

Resolve and read preview files through the original source-thread app-server request view, rechecking workspace identity around the read and validating textual content.

Canonical SHA256: `c99c69d0cb66e5121fa6410b6596eb62024558c316b037fa29f7da9363b0a51f` (10,346 bytes / 259 lines).

Primary source, at the exact final cohort:

- `codex-rs/tui/src/app_server_session/fs.rs:94` — `fn read_preview_file`.
- `codex-rs/tui/src/app_server_session/fs.rs:40` — `fn preview_workspace`.

Integration obligations:

- Use metadata-only thread/read and the app-server environment host/cwd. Refuse unsupported Windows workspace reads, invalid UTF8/control bytes and a changed source workspace; do not guess a frontend local path.
- Absolute and parent-relative paths remain subject to existing server filesystem authority. The helper explicitly follows symlinks; only a server-side open could enforce no-follow. No new root/receiver route or backend credential is granted.

Coverage limits:

- The mock WebSocket test checks actual RPC parameters and 1.2 MB text plus workspace/content failures. It is not a live remote permissions, symlink race or no-follow oracle.

## 5. be5db96264e2 — adopt

Add asynchronous read-only document tabs to the existing workspace sidebar, retaining per-backend/source request identity, user focus and chat-tab lifecycle.

Canonical SHA256: `bcb7a65ee5e5bc632ef3a9c5fef4b8ae160b59662dfb5fcde81bc80d117fe7c2` (93,745 bytes / 2,103 lines).

Primary source, at the exact final cohort:

- `codex-rs/tui/src/app/event_dispatch.rs:101` — `AppEvent::SidebarDocumentLoaded`.
- `codex-rs/tui/src/app/sidebar_document.rs:396` — `fn document_preview_available`.
- `codex-rs/tui/src/app/startup.rs:1567` — `AppEvent::SidebarDocumentLoaded`.

Integration obligations:

- Keep the existing owned-screen/workspace/sidebar availability gates and Cloud exclusion. Resolved replies require the current backend/parent; loaded data is accepted only for its exact retained document request/backend, even when that backend is parked.
- Existing document snapshots can remain readable; new interaction still checks the document backend. Closed request IDs are not resurrected. Preserve ALab human-input grace, overlay/modal authority, account owner, startup boxing, backend bookkeeping and event FIFO hooks.
- The new SidebarTab and tab geometry helpers have no owned caller/literal at fixed comparison. Preserve pending chat fork/close/native-thread behavior and saved divider width. No ALab hook is made obsolete.

Coverage limits:

- One real embedded-app-server/UI declaration covers open/read/tab/focus/overlay/close and is expanded by rows 6/7. Preview text is data, not native ownership or execution authority.

## 6. 7c3f5e2cc290 — adopt

Reuse transcript search, selection, Markdown copy and typed document navigation in sidebar previews while canceling hidden PRIMARY publication and preserving composer paste targets.

Canonical SHA256: `188107f817c05d8a6034aa39c36afff0070c4668d5d9ffaec0f1ab76508a7c97` (90,706 bytes / 2,088 lines).

Primary source, at the exact final cohort:

- `codex-rs/tui/src/app/sidebar.rs:130` — `fn cancel_hidden_document_primary`.
- `codex-rs/tui/src/app/sidebar_document.rs:527` — `fn handle_document_input`.
- `codex-rs/tui/src/terminal_hyperlinks.rs:52` — `markdown_source`.

Integration obligations:

- Copy feedback and PRIMARY publication retain actual visible document ownership through focus/overlay/reconnect changes; composer-validated clipboard reads bypass the sidebar rather than being retargeted.
- Local reader links resolve relative to the current document through the original app-server source. Distinct explicit Markdown occurrences retain source offsets across wrapping; cache equality includes provenance while rendering equality intentionally excludes it.
- The upstream preview test explicitly spawns a 16 MiB stack thread. Preserve that declared upstream fixture scope; it is not normal-depth ALab application/Send/stack qualification and changes no build/profile limits.

Coverage limits:

- The same UI declaration exercises offline copy, hidden PRIMARY, search/navigation and duplicate truncated titles. Row6 does not bound all Markdown preview allocation; source limits arrive in row7 and apply to source previews.

## 7. 3a51865c8cca — adopt

Open typed source-file targets with line/column metadata, bounded source rendering, syntax highlighting and matching hover/click geometry without changing trusted terminal file links.

Canonical SHA256: `a5c784d7ffdaa752d8b8dae88d5986b474327df2c788ac1be78f1f24b7080374` (79,032 bytes / 1,911 lines).

Primary source, at the exact final cohort:

- `codex-rs/tui/src/app/sidebar_document.rs:138` — `MAX_SOURCE_BYTES`.
- `codex-rs/tui/src/app/sidebar_document.rs:350` — `fn source_target`.
- `codex-rs/tui/src/markdown_render/preferences.rs:63` — `fn with_visible_local_paths`.

Integration obligations:

- Workspace previews remain unavailable for Cloud and Windows workspaces. Source/location previews cap retained source at 1 MiB and 10,000 lines; plain Markdown remains rendered Markdown and is not covered by those source-only caps. Server read happens before this display cap.
- Keep explicit FileLink/ReaderLink separation from web/browser links; normalize nonzero line/column, preserve CRLF/grapheme positioning and exact streamed citation metadata. Parked transcript rendering temporarily exposes local paths and restores preference synchronously.
- Document-link preference participates in render/layout caches and refreshes on backend/config/session changes. No owned ViewAction match, TerminalHyperlink literal or removed layout helper needs adaptation at fixed comparison; preserve ALab validated artifact hooks.

Coverage limits:

- Existing actual UI and typed link tests cover CRLF target/highlight/tab reuse/Escape and streaming metadata. There is no new dedicated runtime declaration for both source-limit boundaries or Windows exclusion; do not claim these predicates executed from source alone.

## 8. c0d47262f33f — adopt

Return a visible acceptance message for an actual queue-only message to the root agent, preserving existing non-root wake guidance and TriggerTurn behavior.

Canonical SHA256: `1db787c5ca80502c0601206e500d39fdd4af3e1c8905b436d903e39166312c9e` (4,026 bytes / 83 lines).

Primary source, at the exact final cohort:

- `codex-rs/core/src/tools/handlers/multi_agents_v2/message_tool.rs:87` — `Message accepted.`.

Integration obligations:

- The accepted result follows the actual manager send result and activity event. It reports queue acceptance, not recipient consumption, turn completion or durable client acknowledgement.
- No implicit wake, extra recipient permission, background authority or ALab mailbox/handoff receipt change is introduced. Preserve owned accepted-input/queued-mail/compaction custody.

Coverage limits:

- The unit retains the actual manager operation oracle; guidance/snapshot text alone is not delivery evidence.

## 9. ef6ce8100a91 — adopt

Bound enabled rollout-trace payload and bundle storage, reuse exact prepared items and retain explicit partial-capture lifecycle information instead of fabricating complete evidence.

Canonical SHA256: `29506defb055c1e98abcd2c84ffacb2088af361947b4c6cdadbf51c84ff51564` (64,790 bytes / 1,507 lines).

Primary source, at the exact final cohort:

- `codex-rs/rollout-trace/src/writer.rs:137` — `fn store_body`.
- `codex-rs/rollout-trace/src/writer.rs:49` — `pub struct TraceCaptureLimits`.
- `codex-rs/rollout-trace/src/shard.rs:64` — `fn append_with_limit`.
- `codex-rs/rollout-trace/src/payload.rs:99` — `fn capture_parts`.
- `codex-rs/core/src/session/session.rs:1907` — `config.rollout_trace_limits`.
- `codex-rs/rollout-trace/src/thread.rs:169` — `fn start_child_thread_trace`.

Integration obligations:

- Recording remains opt-in through the existing trace-root environment mechanism. Defaults are 64 MiB per new body/separated value and 256 MiB per shared root/child bundle; they are trace-storage limits, not upstream rollout budget or native Goal budget.
- Preserve complete raw JSON bytes/identity, partial references, surviving content/tool links, compressed-size accounting and reserved event space. Event exhaustion ends at the preceding full event and stays Unclosed; closed partial captures are explicitly ClosedWithPartialPayloads.
- Trace data never becomes a certified receipt, source admission or Goal/accounting authority. Preserve ALab cancellation/write/input custody independently. ExecConfiguration captures/restores raw config layers through the actual constructor, not a fixed field clone; no owned Config/TraceCaptureLimits/start_root literal needs adaptation at fixed comparison.
- No manual lock edit, recording default activation or credentials/provider change. Reduced state cache is generated separately and is outside the stated captured bundle budget.

Coverage limits:

- Existing tests cover compressed omission/later events, byte-exact reconstruction, event-space prefix, client cancellation and CLI startup failure. There is no newly added focused root+child combined-budget or cap-plus-cancellation race declaration; the shared writer is source-proven, not a runtime observation. Dedup index/memory and CPU cost remain unmeasured.

## 10. ce7079b50637 — keep-opt-in

Keep disabled-by-default UTC environment dates opt-in, preserving local-date behavior when absent and tracking the date basis in persisted context snapshots.

Canonical SHA256: `5e15de83de4f3f05f6fdcd1ae0daa83a4bdc85d990bebd2d6652c7628647c151` (23,471 bytes / 507 lines).

Primary source, at the exact final cohort:

- `codex-rs/features/src/lib.rs:2307` — `key: "utc_environment_date"`.
- `codex-rs/core/src/session/world_state.rs:377` — `Feature::UtcEnvironmentDate`.
- `codex-rs/core/src/context/world_state/environment.rs:44` — `current_date_is_utc`.

Integration obligations:

- Do not enable utc_environment_date in ALab packaged/aterm defaults or infer timezone authority from context text. Enabled snapshots render current_date_utc and an unavailable clock uses the matching basis tag.
- A basis-only change emits a context update even for the same calendar date; legacy local snapshots and actual local/remote compaction prefixes remain supported. Native Goal clocks, accounting timers and deadline policy remain independent.

Coverage limits:

- Actual configured UTC request-prefix tests cover midnight rollover and same-day stability, plus two local/UTC removal/recovery cases. No ALab default or shipped prompt behavior is activated by this metadata review.

## 11. 641b4cac88d0 — adopt

Bind Linux host proxy bridge lifetime to outer sandbox supervisors, preserve exit status/signal forwarding and isolate proxy descriptor pressure from mount cleanup.

Canonical SHA256: `71bc0549bd9a7ef4fcc3b3519af968cc785978b76bc16ad5e4e1cc16a9f948fd` (49,441 bytes / 1,250 lines).

Primary source, at the exact final cohort:

- `codex-rs/linux-sandbox/src/linux_run_main.rs:621` — `fn start_bwrap_supervisors`.
- `codex-rs/linux-sandbox/src/proxy_lifecycle.rs:39` — `fn private_bridge_control`.
- `codex-rs/linux-sandbox/src/launcher.rs:155` — `if require_system`.
- `codex-rs/linux-sandbox/src/tcp_net_setup.rs:37` — `pub(super) fn dispatch`.

Integration obligations:

- Keep existing explicit managed-network/transparent-TCP gates and endpoint allowlists; do not activate the prior opt-in capture/attribution features. Transparent mode still requires supported system bwrap and refuses unsupported capability.
- Fork before starting bridge threads; host bridge descriptors/token remain in the hardened supervisor, command child receives only sandbox controls. Transparent setup re-execs before namespace creation to clear inherited credential memory, while route validation and guest token stripping stay intact.
- Preserve process-group cancellation, parent-thread death, SIGCHLD status recovery, independent synthetic mount cleanup and private descriptor table fallback. No ALab source/account/Goal/native permission hook is equivalent or removable.

Coverage limits:

- The live Linux tests may skip for unavailable bwrap/namespaces; Mac source review supplies no Linux execution evidence. They cover normal/group/launcher/caller death and FD pressure, not a certified all-kernel guarantee.

## 12. a11d2403a5c8 — adopt

Recognize only supported same-line /proc or /newroot/proc mount failures for the existing proc-less fallback, retaining explicit PID inheritance and fail-closed unrelated errors.

Canonical SHA256: `59ee7d9378c8d2fa6a23cabede7e80868e08857be195ded1d108b809f63b623b` (6,189 bytes / 139 lines).

Primary source, at the exact final cohort:

- `codex-rs/linux-sandbox/src/linux_run_main.rs:1219` — `fn is_proc_mount_failure`.

Integration obligations:

- Do not broaden fallback to other paths, errnos or separate lines. Transparent TCP continues to require proc and does not enter the proc-less fallback; namespace/permissions remain native launch policy.
- Keep the actual bwrap wrapper destination variants and command/PID assertions. No ALab hook removal, sandbox-default change or held route activation.

Coverage limits:

- Two positive and four negative macro variants are source declarations; the positive errno loop is not six generated cases. The integration has two actual destination variants and Linux prerequisites.

## Declared test candidates and target bindings

All source rows below bind to `a11d2403a5c8f346527379ccd061c2aa95e7edc4`, **not_run**, `final_binding = null`. There are **40 selector rows / 38 distinct function declarations / 46 declared macro cases / 6 Cargo packages / 56 module or include edges**. The same document UI declaration appears for rows 5, 6 and 7; it is counted once in distinct totals. Substring filters identify actual functions, not invented exact generated libtest names. Rust comments and normal/raw/byte strings are masked before attribute/function/module discovery, so quoted `]`, semicolons, sample functions and assertions do not invent cases. Macro labels are retained literally; loops are not expanded into declared cases.

The target roots and package manifests are statically read, not obtained by Cargo execution. In particular, CLI `--test rollout_trace` is an auto-discovered integration target despite its unrelated explicit package-test entry. Core and Linux integration controls use `--test all`, distinct from each package’s `--lib`. Actual module/include chains (including the Markdown `include!`) are recorded in the external source packet.

| Rows | Package / target | Source declaration / substring filter | Macro variants | Oracle and limits |
|---|---|---|---|---|
| 1 | `codex-tui --lib` | `codex-rs/tui/src/bottom_pane/chat_composer.rs:7167`<br>`bottom_pane::chat_composer::tests::skill_popup_prefers_confirmed_selections_within_match_tier` | 1 declaration | Actual persisted selection survives plugin cache version and symlink changes. |
| 1 | `codex-tui --lib` | `codex-rs/tui/src/bottom_pane/skill_popup.rs:495`<br>`bottom_pane::skill_popup::tests::match_tier_precedes_preference_and_plugin_rank` | 1 declaration | Match tier stays ahead of stored preference; direct frequency setup does not exercise new normalization. |
| 2 | `codex-mermaid --lib` | `codex-rs/mermaid/src/tests.rs:210`<br>`tests::flat_subgraphs_and_cross_group_edges` | 1 declaration | One declaration loops all four directions, quoted/semicolon titles, grouping and TooWide fallback. |
| 2 | `codex-mermaid --lib` | `codex-rs/mermaid/src/tests.rs:279`<br>`tests::invalid_or_unsupported_subgraphs_keep_source` | 1 declaration | One declaration checks fourteen unsupported inputs; not fourteen macro cases. |
| 2 | `codex-tui --lib` | `codex-rs/tui/src/markdown_render/mermaid_tests.rs:20`<br>`markdown_render::mermaid::tests::mermaid_fences_use_native_renderer_for_every_family` | 1 declaration | Native Markdown fence routing includes flat groups. |
| 3 | `codex-tui --lib` | `codex-rs/tui/src/history_cell/messages_tests.rs:345`<br>`history_cell::messages::tests::spoken_artifacts_link_only_real_workspace_files_and_preserve_existing_urls` | 1 declaration | Actual workspace artifact validation and existing URL retention. |
| 3 | `codex-tui --lib` | `codex-rs/tui/src/markdown_render_tests.rs:2191`<br>`markdown_render::markdown_render_tests::narrow_header_only_table_retains_local_file_target` | 1 declaration | Header-only fallback retains interactive document metadata. |
| 3 | `codex-tui --lib` | `codex-rs/tui/src/markdown_render_tests.rs:2220`<br>`markdown_render::markdown_render_tests::narrow_header_only_table_preserves_link_columns_and_escaped_pipes` | 1 declaration | Escaped pipes, combining marks and emoji retain link geometry. |
| 3 | `codex-tui --lib` | `codex-rs/tui/src/markdown_render_tests.rs:2240`<br>`markdown_render::markdown_render_tests::narrow_table_record_labels_keep_local_file_targets` | 1 declaration | Both loop widths retain record-label target metadata. |
| 3 | `codex-tui --lib` | `codex-rs/tui/src/markdown_render_tests.rs:2262`<br>`markdown_render::markdown_render_tests::posix_file_uri_keeps_literal_backslash_in_preview_target` | 1 declaration | POSIX target keeps literal backslash independently of display. |
| 3 | `codex-tui --lib` | `codex-rs/tui/src/transcript_view/text_tests.rs:432`<br>`transcript_view::text::tests::rendered_markdown_file_targets_are_only_interactive_in_the_workspace` | 1 declaration | Metadata is inert without workspace document interaction. |
| 4 | `codex-tui --lib` | `codex-rs/tui/src/app_server_session/fs_preview_tests.rs:14`<br>`app_server_session::fs::preview_tests::remote_preview_requests_the_source_thread_and_validates_text` | 1 declaration | Mock WebSocket RPC uses original thread/workspace; validates UTF8/control characters and changed workspace, not live filesystem permissions. |
| 5 | `codex-tui --lib` | `codex-rs/tui/src/app/sidebar_document_tests.rs:19`<br>`app::sidebar_document::tests::focused_markdown_link_opens_read_only_preview_only_with_workspace` | 1 declaration | Real embedded server preview, tab reuse/close/overlay and backend request identity; expanded by rows6/7. not(windows); explicit upstream 16 MiB child test-thread stack, not a normal-depth/native ALab qualification. |
| 6 | `codex-tui --lib` | `codex-rs/tui/src/app/sidebar_document_tests.rs:19`<br>`app::sidebar_document::tests::focused_markdown_link_opens_read_only_preview_only_with_workspace` | 1 declaration | Same declaration adds real search/selection/offline copy/hidden PRIMARY and duplicate/truncated title checks; explicit upstream 16 MiB test thread. not(windows); explicit upstream 16 MiB child test-thread stack, not a normal-depth/native ALab qualification. |
| 6 | `codex-tui --lib` | `codex-rs/tui/src/markdown_render/web_links_tests.rs:200`<br>`markdown_render::web_links::tests::empty_reader_local_links_keep_a_visible_destination` | 1 declaration | Empty/zero-width document labels retain visible path and target. |
| 6 | `codex-tui --lib` | `codex-rs/tui/src/transcript_view/text_tests.rs:466`<br>`transcript_view::text::tests::document_link_navigation_preserves_occurrences_across_wraps` | 1 declaration | Distinct same-target link occurrences stay distinct over three loop widths. |
| 6 | `codex-tui --lib` | `codex-rs/tui/src/transcript_view/select_mode_tests.rs:835`<br>`transcript_view::select_mode::tests::keyboard_table_mark_selects_cells_and_swaps_rectangle_corners` | 1 declaration | Existing table selection cursor geometry uses updated visual modifier. |
| 7 | `codex-tui --lib` | `codex-rs/tui/src/app/sidebar_document_tests.rs:19`<br>`app::sidebar_document::tests::focused_markdown_link_opens_read_only_preview_only_with_workspace` | 1 declaration | Same actual UI fixture adds CRLF source location, highlight, tab reuse and Escape order. not(windows); explicit upstream 16 MiB child test-thread stack, not a normal-depth/native ALab qualification. |
| 7 | `codex-tui --lib` | `codex-rs/tui/src/markdown_render/local_links_tests.rs:19`<br>`markdown_render::local_links::tests::sidebar_keeps_line_and_column_for_extensionless_source_links` | 1 declaration | Extensionless line/column parsing and web exclusion. |
| 7 | `codex-tui --lib` | `codex-rs/tui/src/streaming/render_tests.rs:119`<br>`streaming::render::tests::incremental_file_citations_preserve_metadata_unicode_and_markdown` | 1 declaration | Actual streamed citation target metadata, Unicode and visible output checked separately. |
| 7 | `codex-tui --lib` | `codex-rs/tui/src/transcript_view/input_tests.rs:178`<br>`transcript_view::input::tests::workspace_file_links_show_the_title_with_subtle_hover_and_open_the_same_location` | 1 declaration | Mouse hover and actual click return identical typed source target. |
| 8 | `codex-core --lib` | `codex-rs/core/src/tools/handlers/multi_agents_tests.rs:1316`<br>`tools::handlers::multi_agents::tests::multi_agent_v2_send_message_accepts_root_target_from_child` | 1 declaration | Actual manager send operation to root retains queue-only kind; accepted text is not a receiver-consumption receipt. |
| 9 | `codex-rollout-trace --lib` | `codex-rs/rollout-trace/src/capture_tests.rs:27`<br>`writer::capture_tests::parts_reuse_the_complete_identity_and_round_trip_in_order` | 1 declaration | Exact raw JSON bytes and full identity reuse, explicit partial status. |
| 9 | `codex-rollout-trace --lib` | `codex-rs/rollout-trace/src/capture_tests.rs:71`<br>`writer::capture_tests::compressed_budget_admits_small_later_events_after_an_omission` | 1 declaration | Real compressed bundle on-disk budget, omitted body and readable later event. |
| 9 | `codex-rollout-trace --lib` | `codex-rs/rollout-trace/src/capture_tests.rs:125`<br>`writer::capture_tests::exhausting_event_space_leaves_a_readable_prefix` | 1 declaration | No partial event publication; replay remains Unclosed. |
| 9 | `codex-rollout-trace --lib` | `codex-rs/rollout-trace/src/reducer/conversation_tests.rs:1377`<br>`reducer::conversation::tests::late_tool_start_links_model_call_and_output_versions_in_order` | 1 declaration | Complete-image path preserves conversation order/tool associations. |
| 9 | `codex-rollout-trace --lib` | `codex-rs/rollout-trace/src/reducer/conversation_tests.rs:1382`<br>`reducer::conversation::tests::missing_image_preserves_message_order_and_late_tool_links` | 1 declaration | Partial image retains sibling text, positions and late tool/output links. |
| 9 | `codex-core --lib` | `codex-rs/core/src/client_tests.rs:1775`<br>`client::tests::dropped_response_stream_traces_cancelled_partial_output` | 1 declaration | Actual client stream drop records one partial response; no fixed total payload count. |
| 9 | `codex-core --lib` | `codex-rs/core/src/client_tests.rs:2050`<br>`client::tests::dropped_backpressured_response_stream_traces_cancelled_partial_output` | 1 declaration | Backpressured reader cancellation preserves one partial response. |
| 9 | `codex-cli --test rollout_trace` | `codex-rs/cli/tests/rollout_trace.rs:33`<br>`enabled_recording_cannot_silently_fail_to_start` | 1 declaration | Actual CLI/app-server startup errors for unusable path/tiny manifest budget; explicit truncated closed capture after cleanup. Body prerequisites/skip paths: cargo_bin(. |
| 10 | `codex-core --lib` | `codex-rs/core/src/context/world_state/environment_render_tests.rs:431`<br>`context::world_state::environment::render_tests::current_date_diff_clears_once_and_recovers` | "local", "utc" | Two declared local/UTC cases preserve unavailable tag and recovery. |
| 10 | `codex-core --lib` | `codex-rs/core/src/context/world_state/environment_tests.rs:224`<br>`context::world_state::environment::tests::legacy_single_environment_snapshot_preserves_environment_and_refreshes_date` | 1 declaration | Legacy same-date local snapshot emits UTC-basis change once, retains environment. |
| 10 | `codex-core --test all` | `codex-rs/core/tests/suite/current_time_reminder.rs:150`<br>`suite::current_time_reminder::environment_context_time_follows_config` | "default", "time_omitted" | Actual three request prefixes cross UTC midnight then same day, both configured time variants. Body prerequisites/skip paths: skip_if_no_network!. |
| 10 | `codex-core --test all` | `codex-rs/core/tests/suite/scenarios_world_state_prefix_tests.rs:61`<br>`suite::scenarios::world_state_prefix::extension_window_prefix_survives_compaction` | "local", "remote" | Explicit UTC opt-in survives local/remote compaction prefix snapshots. Body prerequisites/skip paths: skip_if_no_network!. |
| 11 | `codex-linux-sandbox --test all` | `codex-rs/linux-sandbox/tests/suite/managed_proxy_bridge_lifecycle_tests.rs:134`<br>`suite::managed_proxy::bridge_lifecycle::reaps_bridges_after_exit_and_cancellation` | 1 declaration | Live subprocess/subreaper checks both profiles and exit/group kill/launcher kill/caller-thread exit, inherited SIGCHLD status. Linux + actual bwrap/namespace/proxy prerequisites; helper may skip. This Mac source inventory is not execution. Body prerequisites/skip paths: managed_proxy_skip_reason. |
| 11 | `codex-linux-sandbox --test all` | `codex-rs/linux-sandbox/tests/suite/managed_proxy_bridge_lifecycle_tests.rs:232`<br>`suite::managed_proxy::bridge_lifecycle::proxy_fd_pressure_preserves_socks_and_mount_cleanup` | 1 declaration | Live FD pressure retains SOCKS and synthetic mount cleanup. Linux + actual bwrap/namespace/proxy prerequisites; helper may skip. This Mac source inventory is not execution. Body prerequisites/skip paths: managed_proxy_skip_reason. |
| 11 | `codex-linux-sandbox --test all` | `codex-rs/linux-sandbox/tests/suite/managed_proxy.rs:1030`<br>`suite::managed_proxy::handoff_isolates_concurrent_endpoints_and_closes_privileged_descriptors` | 1 declaration | Existing real handoff retains endpoint/descriptor isolation. Linux + actual bwrap/namespace/proxy prerequisites; helper may skip. This Mac source inventory is not execution. Body prerequisites/skip paths: managed_proxy_skip_reason. |
| 12 | `codex-linux-sandbox --lib` | `codex-rs/linux-sandbox/src/linux_run_main_tests.rs:30`<br>`linux_run_main::tests::detects_proc_mount_failures` | "older bubblewrap", "bubblewrap 0.12" | Two declared bwrap destination variants; each loops supported exact errno strings. Linux-gated pure string-parser unit; no bwrap/runtime helper or helper skip path. UNRUN. |
| 12 | `codex-linux-sandbox --lib` | `codex-rs/linux-sandbox/src/linux_run_main_tests.rs:45`<br>`linux_run_main::tests::ignores_unrelated_mount_errors` | "non proc mount", "different destination", "unhandled mount error", "unrelated error on a different line" | Four quote-aware macro variants reject unrelated destination/error/line. Linux-gated pure string-parser unit; no bwrap/runtime helper or helper skip path. UNRUN. |
| 12 | `codex-linux-sandbox --test all` | `codex-rs/linux-sandbox/tests/suite/managed_proxy.rs:345`<br>`suite::managed_proxy::proc_mount_denial_preserves_legacy_fallback_and_explicit_pid_inheritance` | "older bubblewrap", "bubblewrap 0.12" | Two real launcher variants retain explicit PID inheritance and legacy fallback. Linux + actual bwrap/namespace/proxy prerequisites; helper may skip. This Mac source inventory is not execution. Body prerequisites/skip paths: cargo_bin(. |

## Remaining validation and source limits

- Run actual targets only after normal integrated-source admission, with exact source/compiler/pin identity and native owner coordination. A source declaration, shell/Python validation or conceptual combination of controls is not runtime evidence. Linux helpers can skip; skipped bodies do not qualify namespace, FD, cancellation or sandbox behavior.
- Before claiming row 7’s source cap or Windows exclusion has executed, add/run actual boundary controls. Do not turn the display-side source cap into a promise about Markdown, RPC body size or server-side containment. Existing preview tests intentionally exercise large text.
- Before claiming shared root/child trace-budget or cap-plus-cancellation race coverage, bind actual controls for those exact schedules. Existing compressed-budget, prefix, cancellation and reducer controls supply their stated component coverage, not full arbitrary trace/receipt custody. Partial traces must remain visibly partial.
- The optional UTC date and previously reviewed transparent-TCP/attribution paths stay opt-in. Source adoption of bug fixes does not activate a feature or bypass unsupported executor capability. No policy permission follows from a preview link, rank, message acceptance text, trace reference, proxy correlation ID or clock tag.
- Bounded fixed-comparison owned searches found no callers/literals for deleted `width::char_width`, changed sidebar/layout/link enums, `SkillPopup::with_frequencies`, `ThreadTraceContext::start_root` or new trace-limit structs. Existing config-layer capture/restore uses the real constructor. Later-main composition must repeat this census; no compatibility shims or speculative changes are introduced here.
- Preserve ALab human-input/overlay authority, final box/turn decision checks, backend FIFO and parked owner bookkeeping, startup/child-registration boxing, accepted input/receipts, account epoch and restore restrictions. None of the preview or trace changes is a replacement for those owned semantics.

## Static record

The external audit packet under `~/.ubd/codex-upstream-next12-afterb8cb-cancellation-20261117` retains canonical comparisons, quote-aware source selectors and literal macro attributes, source Git blobs/SHA256 values, primary anchors, module/include edges, fixed-comparison overlap/caller census and maintained-validator outputs. Those local packet paths are reproducibility aids, not shipped runtime evidence or a final binding. Maintained exact-twelve validation passed. The authoritative full review census passed with 214 / 214 reviewed commits across twelve records, no missing decisions or errors. The unchanged feature catalog passed with 94 hook topics / 14 features. These are source-data checks, not execution or gate qualification; the metadata-only change waives no source/gate failure.
