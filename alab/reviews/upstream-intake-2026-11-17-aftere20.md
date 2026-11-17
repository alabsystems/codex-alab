<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Source-only intake after e20

Proposal base: `286fa693597a0403c5af58ef9e2a83c6bbf171dd` (published reviewed262). Historical fixed comparison is `e85ee16eec9f07b17b431c6514fca017aa06d72a`; a separate 25-path current-code addendum binds `9c005ceecd69ed2443e49e8feccc92c590b4cada`. Published `286fa693` adds only its two reviewed 262-decision metadata files to `9c005cee`. Canonical range `e20ccdfa80361deb81266d3ed6baea9c2cdd5ee1..49d657aad0e878fb7ab156b91126828c21a0005f` is the exact next five single-parent commits: 147,255 bytes / 3,241 canonical lines. Proposal: **5 adopt**. Only this review and `alab/upstream/49d657aad0-from-e20ccdfa80-intake.json` change. This intake performs no source implementation/import, Rust/compiler/formatter/Node/TS/generator/sample/helper/native/controller/gate, Cargo-metadata/version/V8/probe, SDK/default/permission/held-route change or publication. Statements concern this intake; separate lab diagnostics are not credited here.

All selected controls are **not_run /UNRUN /final_binding null** in this intake. Static review and metadata/ownership checks are not runtime, Send, containment, performance, actual source imports or application qualification. Actual integrated ancestry is distinct from review counts.

The author read all five whole canonical textual production/test/snapshot changes; root independently read 1–4 and source API fragments. All five canonical bytes/parents/trees/paths were independently rederived. No manifest changes occur in the canonical five; actual package/target manifests are still bound. The 37 per-commit assignments name 37 distinct Rust declarations / 40 declared cases before platform/skip filtering, in four packages. One tool-audio declaration has four actual literal test_case variants; ordinary loops add no cases. There are 28 TUI library, three attachment-store library, one OTEL library and five Core `--test all` declarations and 34 actual module edges/cfg/path attributes are bound. One image control inherits a non-Windows guard, all five Core controls have network skips. Filters are actual declaration substrings, not generated exact case names or runtime listings. The OTEL active-exporter control is inherited, not introduced by this cohort.

Frozen source audit1566eb771c40023bf8bc86197d4df31f091b109f69e3e4ebee22d8adc2b50653 and root source reviewd5ac4122669606cafccec7e3c88da2f0a8b2fbb0fb343dde7a3edd2aacd16807 retain precise per-control scope. Latest 262-decision publication receipt98cb794add4d07d10f6fb68730be7340e21c5a99265445da996e4cb6689b3045 binds the proposal base. Root-managed fetch receipt985fd82476972f391b7cf0965147114c1a1a377a858ff91247e1986c4d9d2636 binds exact cached tip49d; the author did no fetch. No later tip or source is silently included.

ALab analytics=false and metrics_exporter=none stay byte-exact. Explicit custom OTLP remains deliberately supported under native policy even if analytics=false; built-in Statsig alone requires current consent. Existing statusline opt-in/defaultfalse remains; keyboard help is a separate explicit '?' and available view-key action. FileBackedToolAudio flags remain unchanged. Metrics, typed uploads, file IDs and keyboard display grant no native account/request/Goal/Empty/input/TTY permission.

Inherited 81/92/f226/2875 terminal Err/EOF/settlement-steer custody; shared/remoteexec and actual root/input/FD leases; original request/attempt/Task/member/Empty/auth chronology; retained usage/current native Goal budget; and held SourceV1/full-capsule/all-root receiver obligations remain OPEN. Do not use telemetry attempt counts, current UI state, declared MIME or absence as an authority fallback.

| Row | Canonical commit | Decision | Scope |
|---|---|---|---|
| 1 | `f06700bc5d28` | adopt | Count aggregate statusline selection events |
| 2 | `efc5d35455f1` | adopt | Count the first qualifying prompt attempt per visit |
| 3 | `d75ea7b0fa9b` | adopt | Count accepted reader starts and typed failures |
| 4 | `db9778b73cf0` | adopt | Use typed image and tool-audio upload content |
| 5 | `49d657aad0e8` | adopt | Show runtime shortcut bindings on a keyboard diagram |

## 1. Count aggregate statusline selection events — adopt

Count fixed-label selection events using native current telemetry policy while preserving analytics=false/metrics_exporter=none defaults and explicit custom OTLP behavior.

Integration requirements:

- Preserve packaged ALab analytics=false and metrics_exporter=none exactly. Explicitly configured OTLP collectors remain deliberately supported even when analytics=false; built-in Statsig alone requires current consent. No blanket telemetry-off claim for explicit collector configuration.
- Capture eligibility exactly: Statsig uses default environment; bounded_originator_tag_value(originator)==originator; all three custom OTEL_RESOURCE_ATTRIBUTES/OTEL_EXPORTER_OTLP_METRICS_HEADERS/OTEL_EXPORTER_OTLP_HEADERS absent or empty. The equality accepts listed known values AND literal sentinel other, so it is not an absolute allowlist-only assertion.
- Record uses entire stored/current exporter enum equality plus native current analytics check. MetricsClient may follow shared active inner installed by native global provider replacement; preserve that live exporter semantics, not an immutable original-collector/credential/process lease.
- Metric names/labels at new call sites are fixed static labels. Do not export raw provider URI/name/resource content. Collector field is live runtime state; preserve original owner or supported refusal rather than reconstruct from labels/IDs.

Control limits:

- Collector tests inspect scope-metric datapoint labels/counts, not complete Resource attributes/headers/transport. In-memory exporters and injected env getters do not exercise real Statsig/OTLP network.
- Runtime ManualReader explicitly uses Delta; snapshots are not cumulative counts. Failed saves/unchanged selection retain behavior; no efficiency/zero-overhead measurement.

Selected source declarations (final-cohort source, including later refinements, not prefix-only qualification):

- `codex-tui --lib statusline_metrics::tests::custom_resources_and_otlp_headers_suppress_builtin_reporting` — In-memory collector plus injected env getter checks three nonempty custom resource/header cases suppress StatuslineMetrics capture; it does not contact Statsig or inspect full Resource/header transport. cfg: cfg(test). UNRUN.
- `codex-tui --lib statusline_metrics::tests::metrics_skip_changed_destinations_and_custom_builtin_resources` — In-memory collector checks configured exporter inequality and custom environment capture suppression; native MetricsClient can still follow shared active exporter state, not an incarnation lease. cfg: cfg(test). UNRUN.
- `codex-tui --lib app::status_line_settings::tests::failed_write_preserves_the_file_and_the_live_selection` — Actual temporary config writes count successful enabled_custom/switched_custom only; repeated selection/failed writes preserve bytes/live selection and emit no extra change. cfg: cfg(test). UNRUN.
- `codex-tui --lib app::statusline_picker::tests::discovery_batch_keeps_the_highlight` — Paused native picker fixture retains highlight and counts picker open/return-to-default with fixed change label; injected discovery batches are not real server discovery. cfg: cfg(test). UNRUN.
- `codex-otel --lib metrics::client::tests::counter_consent_uses_the_active_exporter` — INHERITED native OTEL unit explicitly marks in-memory inner as consent-requiring, switches actual active pointer and proves current built-in consent vs custom collector behavior without Statsig network. cfg: cfg(test). UNRUN.

## 2. Count the first qualifying prompt attempt per visit — adopt

Count the first qualifying primary-thread user-prompt attempt per successful different-primary visit, without treating telemetry as submission admission or execution evidence.

Integration requirements:

- Preserve actual user trigger plus StartOrSteer/StartIfIdle and matching primary/active/widget thread checks; async-question replies remain excluded. Thread IDs are native UI association data, not model/Goal/account permission.
- Recording occurs BEFORE actual submission admission/result. Do not call this an accepted/executed-prompt receipt; rejected qualifying attempts may count. first_prompt_recorded advances even if telemetry suppressed.
- Reset only after successful different-primary attach at native begin_visit frontier. Preserve live flag/collector ownership through supported retention, not reinitialized defaults that count again or copied IDs that recreate actor authority.

Control limits:

- Actual embedded/mock test covers feature_off and builtin across fresh/resumed loop; custom branch and rejected qualifying prompt are source paths, not directly tested there.
- Counter/visit boundaries are telemetry, not error/Empty/source/admission authority. No post-terminal, billing or native Goal proof.

Selected source declarations (final-cohort source, including later refinements, not prefix-only qualification):

- `codex-tui --lib app::tests::session_lifecycle_requests::first_prompt_counts_once_before_and_after_resume` — Actual embedded app-server/local Responses fixture checks first/follow-up prompts in fresh/resumed visits for feature_off and builtin. Two loop visits remain one declaration; custom mode/rejected prompt coverage not supplied. cfg: cfg(test). UNRUN.
- `codex-tui --lib app::tests::session_lifecycle_requests::cloud_async_question_reply_uses_cloud_admission` — Existing fake Cloud command-channel accepted/rejected question-reply branches assert no first-prompt metric; not live Cloud or held-route admission proof. cfg: cfg(test). UNRUN.

## 3. Count accepted reader starts and typed failures — adopt

Count only current accepted reader Start/failure events with bounded labels while preserving stale-result rejection and existing statusline opt-in.

Integration requirements:

- provider_started is emitted on accepted current ReadStep::Start BEFORE footer.start lookup/read. It does not prove MCP connection or server/process start.
- provider_failed uses only kind=timeout/error after existing token/thread/cwd/provider/backend checks; stale Finished injections must not count. Lookup timeout/read timeout classify timeout; unavailable/snapshot/other Read classify error.
- Preserve statusline opt-in/default false, original current telemetry checks, independent usage/current-budget and all held-route constraints.

Control limits:

- Reader/picker controls use paused time/fake channel and in-memory metrics, not real server failures/transport. Polling one start is distinct from per-poll events.
- No raw resource/RPC text enters metric labels; existing safe typed diagnostics remain. Current native exporter is not an incarnation proof.

Selected source declarations (final-cohort source, including later refinements, not prefix-only qualification):

- `codex-tui --lib app::statusline_picker::tests::discovery_only_starts_a_reader_on_explicit_selection` — Paused fake-channel picker control counts explicit accepted reader Start and change, with URI change/reselection loop inside one declaration; not MCP process start. cfg: cfg(test). UNRUN.
- `codex-tui --lib chatwidget::statusline_reader::tests::statusline_reader_waits_for_the_displayed_thread_and_keeps_its_result` — Actual widget lifecycle plus injected Finished/UiTree events rejects stale token/thread/cwd and leaves metric snapshot empty for rejected events; no real process ownership proof. cfg: cfg(test). UNRUN.
- `codex-tui --lib chatwidget::statusline_reader::tests::statusline_polls_one_second_after_completion_including_empty_content` — Paused fake-channel reads preserve one-second-after-completion polling and count one provider_started, not one per poll. cfg: cfg(test). UNRUN.
- `codex-tui --lib chatwidget::statusline_reader::tests::statusline_invalid_content_stops_polling_immediately` — Paused fake-channel invalid response stops polling and records provider_failed kind=error plus one started. cfg: cfg(test). UNRUN.
- `codex-tui --lib chatwidget::statusline_reader::tests::statusline_hung_read_stops_polling_after_one_second` — Paused fake-channel one-second resource wait reports timeout and stops; does not bound UI snapshot await or remote operation cancellation. cfg: cfg(test). UNRUN.
- `codex-tui --lib chatwidget::statusline_reader::tests::statusline_startup_timeout_stops_before_the_first_read` — Paused lookup/startup expiry before first read records timeout and one accepted Start; started is not successful connection. cfg: cfg(test). UNRUN.
- `codex-tui --lib chatwidget::statusline_reader::tests::statusline_changing_provider_or_reconnecting_discards_the_previous_read` — Paused fake-channel replacement/reconnect rejects old Finished success/failure and counts only accepted reader Start instances. cfg: cfg(test). UNRUN.
- `codex-tui --lib chatwidget::statusline_reader::tests::statusline_explicit_reselection_restarts_a_failed_reader` — Paused fake-channel explicit reselection restarts timed-out reader and records two starts/one timeout; not automatic activation. cfg: cfg(test). UNRUN.

## 4. Use typed image and tool-audio upload content — adopt

Replace image/audio tag classification with typed content while preserving actual thread/ephemeral/payload and policy data; inline MIME verification is not implied.

Integration requirements:

- Preserve thread_id and ephemeral, image file_name/tags/data including image_gen policy, and exact tool-audio declared MIME/data. Variant classification is not policy permission or a new attachment/file lease.
- Inline backend returns exact bytes WITHOUT interpreting MIME. Persistent backends MUST verify declared MIME against uploaded bytes by trait contract; that verification is not implemented/proven by this inline test.
- Preserve original image/model preview distinction, actual native audio/image origin, feature-disabled no-upload branch, response/history/resume metadata and current native budget/usage. No SourceV1/all-root or FileBackedToolAudio default activation.
- No owned named UploadRequest/UploadContent/AttachmentStore consumer found at fixed e85 or later9c; do not restore removed obsolete request fields as compatibility fallback.

Control limits:

- Debug redacts payload bytes only; thread ID, file_name, tags and declared MIME remain visible. No all-metadata/secret-redaction guarantee.
- Literal RIFF unit vector is byte preservation, not complete valid-audio/MIME validation. Native integration uses local mocked model plus recording backend; fake file IDs/expiry are not durable storage/credential proof.
- Audio testcase has four actual annotated variants, not extra loop cases. Core controls retain direct network skips/native helper requirements.

Selected source declarations (final-cohort source, including later refinements, not prefix-only qualification):

- `codex-attachment-store --lib tests::attachment_debug_output_redacts_bytes` — Pure Debug assertion redacts image/audio/result BYTE payloads. Thread ID, file name, image tags and declared MIME remain displayed metadata. cfg: cfg(test). UNRUN.
- `codex-attachment-store --lib tests::inline_store_preserves_image_bytes` — Supplies a typed Image input and checks the inline backend returns its exact bytes; no variant/metadata persistence, policy or MIME verification. cfg: cfg(test). UNRUN.
- `codex-attachment-store --lib tests::inline_store_preserves_tool_audio_bytes` — Inline backend returns literal RIFF byte vector unchanged without interpreting it; literal is not full valid-audio/MIME validation proof. cfg: cfg(test). UNRUN.
- `codex-core --test all suite::code_mode::code_mode_omits_short_audio_and_preserves_other_output` — Native CodeMode/local mock Responses fixture explicitly DISABLES FileBackedToolAudio, retains valid inline audio/text and asserts no recording-store upload. cfg: cfg(test). Direct body skip: skip_if_no_network!. UNRUN.
- `codex-core --test all suite::extension_sandbox::generated_image_ui_item_is_file_backed_in_rollout` — Native local mock/recording-store image generation fixture checks typed Image model/UI upload, actual thread/ephemeral and image_gen policy tags; not live image service/persistent backend. cfg: cfg(test); cfg(not(target_os = "windows")). Direct body skip: skip_if_no_network!. UNRUN.
- `codex-core --test all suite::image_rollout::resumed_history_only_emits_resize_notices_for_new_images` — Native mock/recording-store resume fixture checks typed image upload bytes/dimensions and actual resumed thread; no real storage authorization claim. cfg: cfg(test). Direct body skip: skip_if_no_network!. UNRUN.
- `codex-core --test all suite::scenarios::gif_generation_preserves_animation_and_replays_a_still_preview` — Native mock/recording-store GIF fixture checks animation bytes and separate PNG preview through typed Image variants; no real external generation/backend proof. cfg: cfg(test). Direct body skip: skip_if_no_network!. UNRUN.
- `codex-core --test all suite::scenarios::audio_file::tool_audio_store_id_survives_rollout_and_next_request` — FOUR actual test_case variants: direct/CodeMode x Legacy/Paginated. Native mock+recording-store checks exact typed ToolAudio MIME/data/thread/ephemeral upload across rollout/resume/expiry fixture; fake store IDs are not permission. cfg: cfg(test). Direct body skip: skip_if_no_network!. UNRUN.

## 5. Show runtime shortcut bindings on a keyboard diagram — adopt

Add an explicitly toggled US QWERTY guide from actual runtime bindings while retaining real sidebar/transcript/input/editor ownership and display fallback.

Integration requirements:

- Preserve real routed-key activation, focused sidebar, owned transcript search/interaction, modal and draft priority. Guide hints never execute actions or grant terminal/input/Goal permission.
- Start in List; explicit available Tab or F1–F12 toggles Keyboard. Conflict check includes overlapping runtime bindings and chord prefixes (empty queue exception); remapped/modified/non-US/off-diagram keys remain in list.
- FooterMode becomes tuple ShortcutOverlay(ShortcutView); shortcut guide keys derive from actual RuntimeKeymap. Bounded fixed/current owned census has no direct enum/constructor caller requiring migration, but supported retention must preserve view/runtime-key/physical original ownership or refuse unsupported state, not silently mint closure/input ownership.
- Preserve original sparkle/deadline/status semantics and small-size list fallback. Static US QWERTY diagram and color/palette samples are display conventions, not actual keyboard layout or hardware probe.
- Later current9c editor Command/TempPath, original parser EditorFence, terminal modes/TTY handoff and partial-refusal restrictions must survive future composition. New view must not resume/steal original event source or substitute for those resources; held receiver routes remain held.

Control limits:

- Native test buffers/synthetic palettes/snapshots, not real 80x24 device/OS matrix, renderer performance or end-to-end retained editor transfer.
- Diagram layout builds HashSet/Vec/String/render data and may expand composer height budget for a fitting diagram while native viewport still constrains display. No latency/power/zero-allocation claim.
- All25later current paths are object-bound; only relevant native hooks and prepared-command/handoff/input interfaces were read, not a wholesale semantic qualification of concurrent editor implementation.

Selected source declarations (final-cohort source, including later refinements, not prefix-only qualification):

- `codex-tui --lib bottom_pane::shortcut_overlay::tests::shortcut_overlay_leaves_remapped_tab_and_function_keys_available` — Pure runtime-keymap control chooses F5 when Tab/function/chord prefixes conflict and preserves existing bindings. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::shortcut_overlay::tests::shortcut_overlay_draws_base_keyboard_and_falls_back_to_the_list` — Pure TestBackend snapshots show compact/roomy US QWERTY diagram and width/height list fallback; two size loops do not add cases. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::shortcut_overlay::tests::shortcut_keyboard_adapts_to_probed_and_unknown_terminal_colors` — Pure injected palette dark/light/unknown controls distinguish panel/key/highlight or reset fallback, not a real terminal probe/hardware matrix. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::shortcut_overlay::tests::shortcut_keyboard_keeps_diagram_for_keys_outside_us_layout` — Pure snapshots retain diagram for F13/accented key, signal off-diagram actions and retain their list representation. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::shortcut_overlay::tests::shortcut_overlay_uses_runtime_bindings_and_wsl_paste` — Existing pure runtime-hint/customized-WSL snapshot fixture; literal WSL flag is not actual OS/terminal integration. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::shortcut_overlay::tests::shortcut_overlay_clipping_preserves_customization_in_small_areas` — Existing pure TestBackend clipping/snapshot control keeps customization discoverable in small dimensions. cfg: cfg(test). UNRUN.
- `codex-tui --lib app::owned_transcript::tests::shortcut_action_list_leaves_room_for_the_owned_transcript` — Native test Tui/render buffer at60x24 retains transcript/action list and fallback when diagram does not fit; no physical terminal. cfg: cfg(test). UNRUN.
- `codex-tui --lib app::owned_transcript::tests::shortcut_keyboard_fits_in_owned_transcript_at_80x24` — Native test Tui/render snapshot at80x24 plus active transcript search/interaction refuses guide input until interaction ends. cfg: cfg(test). UNRUN.
- `codex-tui --lib app::tests::key_chords::focused_sidebar_chooser_keeps_tab_with_shortcut_guide_open` — Native test Tui plus config-server fixture routes remapped Tab to actual focused sidebar choice while guide remains visible; no hardware/held receiver grant. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::chat_composer::snapshot_tests::shortcut_footer_displays_configured_chords` — Pure native composer snapshot control retains runtime chord hints with tuple ShortcutOverlay mode. Actual module is chat_composer::snapshot_tests, not nested tests. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::chat_composer::sparkle::tests::shortcut_help_preserves_an_unused_sparkle_but_literal_question_marks_do_not` — Existing pure native sparkle state/snapshot control preserves unused sparkle through help, while literal question-mark input follows ordinary edit behavior. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::chat_composer::sparkle::tests::shortcut_help_hides_visible_sparkles_without_restarting_their_deadline` — Existing pure native sparkle state/snapshot control hides visible animation under help without resetting original deadline; no performance measurement. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::chat_composer::status_surface::tests::activity_shortcut_help_follows_runtime_binding` — Existing pure native status-surface snapshot follows actual runtime activity hint across remapping; no keyboard hardware evidence. cfg: cfg(test). UNRUN.
- `codex-tui --lib bottom_pane::chat_composer::status_surface::tests::status_survives_hints_feedback_and_queue` — Existing pure native status/composer snapshots retain status through help, flash and queue states; loops are one declaration. cfg: cfg(test). UNRUN.

## Cross-cutting source bounds

Metric eligibility is exactly the source predicate, not blanket allowlist-only logic: bounded_originator_tag_value(original)==original also accepts literal sentinel 'other'. Record checks complete current/stored exporter-enum equality and native current consent. The captured MetricsClient can follow shared active_inner/global provider replacement, so it is not an immutable exporter-incarnation/process/credential lease. Native ManualReader explicitly uses Delta; repeated snapshots are not cumulative-counter proof. In-memory tests inspect datapoint labels/counts, not all Resource attributes/headers or real Statsig/OTLP transport. Fixed label names/values exclude raw provider URI/resource/RPC content at these call sites, without claiming all exported metadata is redacted.

first_prompt is recorded BEFORE actual submission admission/result and its once flag advances even when telemetry is suppressed. It is the first qualifying ATTEMPT, not an accepted or executed prompt receipt. The accepted reader Start metric is emitted BEFORE lookup/read and proves no server/process start. Existing reader token/thread/cwd/provider/backend checks and safe typed diagnostics remain. No late callback/current ID can create permission.

UploadContent carries actual native source data: Image file_name/tags/data and ToolAudio declared MIME/data, plus unchanged thread_id/ephemeral. Inline preserves bytes without interpreting MIME. Persistent backend verification remains a documented requirement, not proof from literal RIFF bytes or fake recording-store file IDs. Debug hides payload bytes only and still exposes thread ID/file name/tags/MIME. Removed old request fields have no named owned callers in bounded fixed/current census; do not restore obsolete compatibility APIs.

The guide displays a static US QWERTY convention; synthetic palette/WSL/test-buffer snapshots are not a hardware/layout/OS matrix. Runtime bindings and chord prefixes choose the available view key; modified/off-diagram actions remain in the list. Focused sidebar, active transcript/modal/draft and original routed activation retain priority. Layout creates HashSet/Vec/String render data and may expand composer budget for a fitting diagram while native viewport constrains display; no zero-allocation/latency/performance claim follows.

The current `9c005cee` 25-path addendum is separate from historical `e85ee16e`. Exact before/after Git identities are bound; none of the five canonical patches directly overlaps those paths. Relevant native external-editor/Tui hooks and whole prepared-command, terminal-handoff and original-input interfaces were read, not every concurrent editor implementation body. Preserve SAME Command/TempPath, original EditorFence, real TTY/mode/event-broker handoff, partial-refusal hold and actual cleanup; view state/IDs cannot recreate them or resume another event source. Any supported retained footer/visit/collector state must preserve its actual original owner/data or maintain explicit refusal; no new live-owner/held receiver activation is granted here.

All 37 selectors remain source candidates only. Pure attachment/keyboard helpers have no inherited network/process caveat; metric tests use in-memory collectors, reader tests use paused/fake channels, and Core tests use local mocked model/recording stores with their direct network skips and real maintained package helper requirements. No helper flags, source compiler override, defaults or permission changes are requested. Exact execution, compile/Send/lint and all broader original-cause/native-budget/retention qualification remain separate.
