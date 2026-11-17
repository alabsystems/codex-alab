<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream intake after 336b19ab26e0 — source review, 2026-11-16

This review covers exactly `336b19ab26e0d4e36b16d8a21d668632cd800207` through `c8e4cbcd9476795a08080f02e3bc09a64f6da80d`: 12 contiguous commits. The ALab comparison source is `0b4f0adc00282180af11544eefa125c415510250`. An initial later local `origin/main` observation at `322c991e98ad16e817fce4a570aee9a3519c0439` had three websocket-cancellation review/fixture changes. The assembly observation at `1bd04ff247e9c2c0f15f1f81dcf3295c33fa9b78` also includes reset-preservation and version-metadata changes (nine paths total). The feature catalog/defaults, identified adaptation paths and named declaration source paths remain unchanged. All owned candidates remain explicitly bound to the comparison source; eventual runtime evidence must bind the actual integrated source. Cached upstream is `090be4df0724cdef5758e6a0934ba557af8b6bbe`. The reviewed target remains fixed.

Decision: **7 adopt, 5 adapt**. This commit contains review records only. It does not merge upstream or implement the required adaptations.

All 87 selector rows (87 declarations, 11 packages) below are **not_run**. Function/test annotations, immutable source hashes, package manifests and target roots were checked. Filters are source-declared substrings, not fabricated generated libtest identifiers; macro labels are source annotations and require actual compiled discovery/body execution. No Rust/compiler/native operation, merge, install, receipt, runtime RED/GREEN or final evidence binding is claimed.

## Policy and custody

- Preserve apps/plugins/tool_suggest and other disabled or opt-in features, explicit model/provider/account selection, and current ALab defaults. No Cloud service, per-environment context, buffered response processing or held receiver path is enabled for evidence.
- Preserve the explicit SourceV1/all-root receiver hold, native source/account/root/permission admission, accepted input/history ownership, actual ResultWrite and goal mutation custody. Prompt text, country metadata, frames, observation flags and billing markers are not native receipts.
- Keep Cloud task/draft/source ownership until a ready local handoff. Local request abandonment, trace completion and stream readiness are not proof of remote rollback or future stream health.
- The separately reviewed unqualified ALab policy follow-up has a final one-shot native API issuance contract; this intake does not import it or promise retroactive cancellation of accepted Core queue work.

## Canonical patches

Digests use `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA`, including commit headers. Every stored patch was compared again to cached Git bytes.

| Commit | Decision | Subject | Patch SHA256 |
| --- | --- | --- | --- |
| `5943d7acdf01` | adapt | Persist timestamped realtime frames before acknowledging completion (#61473) | `42c9608df7a5151ec700b3174f2414d2bb5c2082ce78d59015a6393b166a964d` |
| `05229d1874d4` | adopt | Add snapshot pagination for persisted realtime history (#61480) | `a074511e02e053d934afa91fee9b6d88754dfbe70b01e612c683501d38e119ab` |
| `6ddb87e70053` | adopt | Allow Ctrl+C to quit during Cloud session switches (#61481) | `5bf73d998571f05102d342e90d858f083e5ddf5d83581fe66218c3463fa2cdad` |
| `7924f95421a3` | adapt | Keep archived threads read-only and support unarchiving in the TUI (#61486) | `3428ca8286d25a1ead0d5f194e20ad12e9341da96b10ddbc8dd537fbe5d2bc82` |
| `048431dcbecc` | adopt | Harden Git remote collection and limit remote URL metadata (#61488) | `b443099f96207be6e1c227e507c63b6ce900be22dc06d982bb989848c3bd210c` |
| `d659419fe7f7` | adopt | Preserve the current Cloud task until navigation is ready (#61490) | `c032d4098f43f25810dee0dd663347f700dca3f6e4940015a70bc0057f0db2f2` |
| `b1573f102413` | adopt | Describe context and permissions per environment behind a feature flag (#61494) | `6b3fcf12ff39df9ab373c8cb6b8924119a880c28428d073474649e5b9cfe75bb` |
| `65a5410f72da` | adapt | Add trusted request-country metadata to Responses requests (#61535) | `8bbff76eee5dfb9496601488fb08060fb103c37d0473113c62a8a1e8d564ec65` |
| `81b995a6c9a4` | adapt | Unify model request hook lifecycles across sampling and compaction (#61562) | `995d6542354d137b16dcbb652ba91cf35a551dfbf4c3c56119e9fa98e1996377` |
| `92c1a4dc2e1f` | adopt | Add response processing after completion and usage accounting (#61577) | `c2a8dfa6137af3fe8549119eea20a1af4929c34432e40dc86b78523d10690bf3` |
| `f226c9ccfb97` | adapt | Recheck rollout budgets before starting tool work (#61578) | `d5ab290bfa8ae3a3c2866fc137c39e27779b3f4fe8ffd3d59da9ababad9a422b` |
| `c8e4cbcd9476` | adopt | Support fractional rollout budgets and configurable context reminders (#61579) | `8dc8b598b519d3e85345a7ae49bd500ece7645cc00ad1d449c686c35f582b439` |

## 5943d7acdf01 — adapt

Accept enrolled canonical realtime frames with first-observed timestamps, distinct call identities and a flush barrier before completed history events. Retain legacy unenrolled transcript rendering and carry observation floors through checkpoints. Adapt the owned exhaustive metadata fixture with a nonempty floor; no input acceptance or effect receipt follows from a frame or schema variant alone.

Affected contracts: `history-pagination`, `exec-handoff`, `execution-continuity`, `qualification-fixtures`.

Required integration behavior:

- Add observation_clock_floor_ms: Some(nonzero) to alab-core/glue/exec_memory_records.rs336 and retain its full compaction roundtrip equality oracle; explicitly assert the floor if useful. Production codecs already carry the full RealtimeItem and CompactionResumeMetadata without projecting fields away.
- Preserve append→flush→HistoryItemCompleted ordering, including disk sync and first directory entry synchronization on Unix. A failed barrier must not acknowledge completion; legacy unenrolled and ephemeral histories retain their contracts.
- Preserve stable first-observation timestamp when sealing a split transcript and restore maximum durable observation floor after owner restart/compaction. Keep distinct generated call ID versus thread ID and explicit ExistingCall identity.
- Retain ALab input/delivery/native goal custody and active context metadata. New TurnInputAccepted and SessionEndInputSaved variants describe acceptance records; do not infer that a frame acknowledges an accepted input or that declaring a variant proves a producer/receipt path.
- Retain shared rollout flush/shutdown error boundaries: any caller may discharge an outstanding realtime barrier, and background append now writes pending records without invoking shared flush. Preserve actual ALab history, ResultWrite ACK and goal persistence ordering; do not infer zero I/O overhead for other callers from an unenrolled frame flag.

ALab interactions:

- Only owned exhaustive CompactionResumeMetadata literal found is the real record-codec fixture. MemoryRecord stores typed RealtimeItem and CompactionResumeMetadata directly, so no production field projection adaptation is demonstrated.
- Upstream adds one Box at Session initialization, one per send_event_raw and one around each regular run_turn await. Lazy operation/cancellation order is retained; this source review does not measure stack/layout performance or claim these changes fix current independent native failures.
- Existing live/mixed timeline conversion presents frame speech as transcriptSegment, while the new persisted reader preserves canonical Frame shape. Hidden acceptance variants are omitted from ordinary live history notifications and mixed timeline queries.

Limits and missing controls:

- Actual persistent completion evidence requires the filesystem/barrier and live Core notification/read tests to execute. Parser/schema declarations and handoff enum names are not receipts.
- Owned codec roundtrip proves metadata preservation only when run. Enabled native exec/restart paths must preserve restored observation floors; no new supported voice-handoff or older-reader compatibility claim is made, and the SourceV1/all-root hold stays intact.
- sync_data/directory synchronization are source-defined operations, not measured latency, zero-I/O, power-loss or hardware-durability guarantees. Shared flush and shutdown callers need actual integrated regression evidence; no benchmark or extra Rust run is performed here.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `observation_time_survives_clock_rollback_and_owner_restart` | `codex-rs/core/src/observation_clock_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-core --lib` | `compaction_retains_the_floor_when_older_events_are_omitted` | `codex-rs/core/src/observation_clock_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-protocol --lib` | `frame_roundtrips_and_requires_a_voice_payload` | `codex-rs/protocol/src/realtime_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-rollout --lib` | `first_realtime_seal_requires_directory_sync_after_ordinary_writes` | `codex-rs/rollout/src/recorder_tests.rs` @ `c8e4cbcd9476` | Unix filesystem barrier; require actual body execution. |
| `codex-core --test all` | `conversation_records_history_without_an_event_observer` | `codex-rs/core/tests/suite/realtime_conversation.rs` @ `c8e4cbcd9476` | Source cases: transport_close, error_close |
| `codex-core --test all` | `conversation_user_text_turn_is_not_sent_to_realtime` | `codex-rs/core/tests/suite/realtime_conversation.rs` @ `c8e4cbcd9476` | Source cases: durable, ephemeral; Durable case reads completed seal without an extra flush. |
| `codex-app-server --test all` | `existing_call_attaches_without_reinitializing_the_client_session` | `codex-rs/app-server/tests/suite/v2/realtime_conversation.rs` @ `c8e4cbcd9476` | Source cases: defaults to v1, supports v3, rejects v2 |
| `codex-app-server --test all` | `realtime_starts_on_forked_and_resumed_threads` | `codex-rs/app-server/tests/suite/v2/realtime_conversation.rs` @ `c8e4cbcd9476` | Source cases: fork without opt in, cold resume without opt in, fork with legacy disabled flag, cold resume with legacy disabled flag, fork with legacy enabled flag |
| `codex-core --lib` | `compaction_keeps_nested_tool_outcomes_and_review_provenance` | `codex-rs/alab-core/glue/exec_memory_records.rs` @ `0b4f0adc0028` | Owned host include, extend actual roundtrip to nonempty new floor.; Owned include: compiled by listed host package, not physical glue directory |

## 05229d1874d4 — adopt

Accept a runtime-independent persisted realtime reader with bounded immutable snapshots, observation/position ordering, exact-event projection coverage and fork-local boundaries. Pending or unsupported coverage and filtered empty pages must never become deletion, input acceptance or native admission evidence.

Affected contracts: `history-pagination`, `account-isolation`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Keep page limit1..500, nonempty time range, exact query/rollout/snapshot cursor binding and committed-position catch-up so late seals cannot leak into an older snapshot.
- Use current selected rollout resolution including archive and hide copied parent frames using proven fork/subagent boundary; a copied fork with no boundary remains Unsupported.
- Preserve existing account-scoped discovery/authenticated app-server source ownership. This explicit thread-ID read does not acquire a runtime or change source/account selection; do not reinterpret archived history as writable.
- Keep experimental thread/realtime/list activation policy and canonical generated protocol/schema artifacts. Unsupported stores must explicitly return Unsupported, not fabricated ready data.

ALab interactions:

- No owned new ThreadRealtimeListParams or ListRealtimeParams literal/caller was found. New reader uses existing paginated validation, per-thread live-writer lock, selected-rollout materialization and SQLite projection.
- LocalHistoryAccount is a discovery-scoping contract; do not invent a new account-read authorization assertion from its name. Preserve current host and source boundaries and test account isolation on eventual integrated source.

Limits and missing controls:

- Supplied SQL controls cover fixed snapshot, ties/late catch-up, switched session cursor, projection lag and exact IDs; actual app-server controls cover canonical frame shape and child-only fork data.
- Focused explicit invalid page-size/range, mismatched snapshot/query and archived/legacy Unsupported controls are still necessary if existing integrated suites do not exercise them. Source declaration presence alone does not establish these branches.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-thread-store --lib` | `snapshot_stays_fixed_while_position_catchup_finds_a_late_seal` | `codex-rs/thread-store/src/local/thread_history/realtime_stream_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-thread-store --lib` | `projection_lag_is_explicit_and_descendants_hide_parent_positions` | `codex-rs/thread-store/src/local/thread_history/realtime_stream_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-thread-store --lib` | `exact_event_id_waits_for_the_acknowledged_seal_to_be_projected` | `codex-rs/thread-store/src/local/thread_history/realtime_stream_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-app-server --test all` | `timeline_pages_mix_items_and_resolve_the_opening_realtime_session` | `codex-rs/app-server/tests/suite/v2/thread_timeline.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-app-server --test all` | `realtime_list_reads_only_frames_sealed_on_the_paginated_fork` | `codex-rs/app-server/tests/suite/v2/thread_timeline.rs` @ `c8e4cbcd9476` | Source declaration only. |

## 6ddb87e70053 — adopt

Allow a real Ctrl+C press to clear buffered draft/modal work first and then request ShutdownFirst during Cloud preparation, dropping the pending operation before adoption. Cancel unowned queued or in-flight start requests before their handshake can replace the active attachment; do not claim remote prewarm rollback.

Affected contracts: `native-ui`, `execution-continuity`, `account-isolation`, `qualification-fixtures`.

Required integration behavior:

- Preserve draft/paste/search/modal cancellation precedence, shutdown ordering, trust confirmation and explicit destination settings. Other navigation stays serialized while preparation owns the pending Cloud stream.
- Preserve biased closed-response cancellation for start/list work and existing ordinary request semantics. Dropping a local request is not a receipt proving the remote service did not prewarm or commit anything.
- Retain source/native task custody and actual old-view ownership; later d659 adds scoped resume preparation so a failed/cancelled navigation cannot displace the current attachment.

ALab interactions:

- No owned wait_for_cloud_switch/load_new_session_config_from callsite or exhaustive CloudOperation constructor adaptation was found. ALab clipboard/interface/account hooks in the touched paths must remain thin and preserve source ownership.

Limits and missing controls:

- TUI controls use actual event routing and worker controls hold a real mock RPC. They are not live Cloud service cancellation or signed native exec qualification.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-tui --lib` | `cloud_switch_wait_quits_on_ctrl_c_before_adopting_the_thread` | `codex-rs/tui/src/app/tests/new_session_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `cloud_switch_wait_ctrl_c_clears_buffered_draft_before_quitting` | `codex-rs/tui/src/app/tests/new_session_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `cloud_switch_wait_keeps_draft_editable_without_submitting_to_local` | `codex-rs/tui/src/app/tests/new_session_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-app-server-client --lib` | `dropping_an_in_flight_start_unblocks_the_native_worker` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-app-server-client --lib` | `dropping_an_in_flight_list_unblocks_the_native_worker` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |

## 7924f95421a3 — adapt

Keep archived cached threads read-only, retain drafts/initial prompts/questions/pending steers, and require real unarchive plus successful reattachment before editing resumes. Adapt the owned pet dock to the generalized read-only API so both archived and external-writer notices retain their layout.

Affected contracts: `native-ui`, `history-pagination`, `execution-continuity`, `qualification-fixtures`.

Required integration behavior:

- Replace removed chat.external_writer_view access in alab-tui/glue/pet_layout.rs69 with chat.is_read_only_view(); extend its actual hidden-pet/no-frame scheduling control to both read-only reasons. Do not let a new archived notice expose the dock or overwrite its text.
- Keep archive metadata distinct from live attachment; cached archive notifications mark replay-only, read-only input never autosends, and writer conflict must retain archive state/draft.
- External unarchive recovery is bound to the still-displayed same thread and channel archive flag. A stale event cannot reattach an unrelated current view; local unarchive failure keeps the original recovery surface.
- Preserve ALab TUI point/box/clipboard/title hooks and actual Core admission/native goal/source custody. Archive or an empty transcript is not deletion, authority or approval.

ALab interactions:

- Only owned removed external_writer_view field read found is pet_layout.rs. No owned ThreadEventChannel literal or read-only enum constructor needs adaptation.
- Upstream disables composer input while cancelling transient search, suppresses initial-message/queue autosend, freezes tool cells, and retains accepted steer records rather than retrying them from a read-only view.

Limits and missing controls:

- Actual embedded archive/unarchive fixtures must execute including writer conflict and failed recovery; pure read-only rendering alone does not prove live reattachment.
- The existing pet test covers hidden geometry/fork scheduling; it does not yet assert the new archived/external notice cases. Required extension is a proposal, not implemented evidence.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-tui --lib` | `archived_thread_notice_blocks_input_and_unarchives_with_draft` | `codex-rs/tui/src/app/tests/archived_thread_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `archived_thread_notice_retains_draft_when_recovery_fails` | `codex-rs/tui/src/app/tests/archived_thread_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `archived_view_preserves_pending_steer_when_interrupt_arrives` | `codex-rs/tui/src/chatwidget/tests/composer_submission.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `archived_view_preserves_initial_prompt_and_dismisses_search` | `codex-rs/tui/src/chatwidget/tests/composer_submission.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `archived_view_collapses_questions_and_retains_answers` | `codex-rs/tui/src/chatwidget/tests/questions_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `externally_archived_thread_keeps_advertised_agents_navigation` | `codex-rs/tui/src/app/tests/agents_navigation_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `reconnect_allows_slow_hydration_but_bounds_a_stalled_server` | `codex-rs/tui/src/app/tests/active_reconnect_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `hidden_pet_does_not_schedule_frames_and_visible_pet_resumes` | `codex-rs/alab-tui/glue/pet_layout.rs` @ `0b4f0adc0028` | Owned host include; extend with both read-only notice reasons.; Owned include: compiled by listed host package, not physical glue directory |

## 048431dcbecc — adopt

Collect sanitized Git fetch URLs from NUL-delimited local configuration and resolve ambiguous rewrite/reset semantics through bounded concurrent local Git lookups under one deadline. Fit complete remote identities into8096-byte ASCII turn metadata when the nonremote payload fits; never truncate a URL into another repository.

Affected contracts: `provider-recovery`, `account-isolation`, `product-policy`, `qualification-fixtures`.

Required integration behavior:

- Preserve local-only --get-url behavior, hooks/fsmonitor disabling, configuration override isolation, native insteadOf precedence/version-dependent resets and credentials sanitization before publishing progress.
- Keep up to8 concurrent remote resolutions under the shared command deadline. Do not claim a global count/byte cap on Git configuration or invent a remote network probe.
- Preserve nonremote metadata and short complete URLs, prioritize origin and deterministic small entries, and keep compatibility-header tool inventory exclusion. If nonremote metadata alone exceeds8096, existing full payload is retained; this is not a universal arbitrary metadata size guarantee.
- Git repository URL metadata is disclosure, not source/native execution authority. Preserve existing ALab provider/header privacy, account/native root identity and analytics opt-out policy.

ALab interactions:

- No owned associated_remote_urls/CodexResponsesMetadata/private remote parser caller was found. Existing metadata and origin hooks must remain unchanged; dependency feature addition is upstream sync data, not an owned lock edit.

Limits and missing controls:

- Source tests cover malicious filter text/nonUTF8 config/rewrite credentials/empty resets and exact boundary/large URL shedding. Timeout/concurrency cleanup and actual HTTP/WebSocket request metadata still require admitted integrated execution; local process tests do not establish provider acceptance or global privacy.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-git-utils --lib` | `git_remote_urls_keep_filtered_fetch_remotes` | `codex-rs/git-utils/src/info/remotes_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-git-utils --lib` | `git_remote_urls_ignore_git_config_file_override` | `codex-rs/git-utils/src/info/remotes_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-git-utils --lib` | `git_metadata_collection_sanitizes_rewritten_remote_credentials` | `codex-rs/git-utils/src/info.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-core --lib` | `turn_metadata_keeps_the_exact_boundary_and_omits_an_overlong_remote` | `codex-rs/core/src/responses_metadata_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-core --lib` | `oversized_remote_preserves_other_metadata_and_short_remote_urls` | `codex-rs/core/src/responses_metadata_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-git-utils --lib` | `timed_out_git_wrapper_does_not_leave_child_process_running` | `codex-rs/git-utils/src/git_process_tests.rs` @ `c8e4cbcd9476` | Existing shared child process-tree timeout cleanup; not a direct new collection-deadline control. |
| `codex-git-utils --lib` | `timed_out_exited_git_wrapper_does_not_leave_child_process_running` | `codex-rs/git-utils/src/git_process_tests.rs` @ `c8e4cbcd9476` | Existing shared child process-tree timeout cleanup; not a direct new collection-deadline control. |

## d659419fe7f7 — adopt

Prepare Cloud resume attachment and initial history on a scoped channel while the old task remains loaded and its stream continues. Commit only the ready candidate with current generation/healthy stream and no await between acknowledged local commit and replacement; abandonment/history failure keeps the old loaded task and preserves unknown-mutation attribution.

Affected contracts: `execution-continuity`, `account-isolation`, `native-ui`, `history-pagination`, `qualification-fixtures`.

Required integration behavior:

- Keep scoped handle restricted to the candidate thread history and commit; ordinary main-handle commands must not commit it. Dropping candidate/caller, EOF, wrong command, stale reused generation, unload or shutdown must discard the candidate safely.
- Preserve old request loaded settings, partial-history warning and stream until successful commit; carry any canceled recovery loss conservatively, never treat cancellation as remote rollback or retry permission for unknown mutation.
- Preserve bounded8-event prefix probe/order and unread stream tail; real candidate hydration must finish without buffering an entire live burst or blocking unrelated history RPC progress.
- Bind Command Center search rows to their actual source BackendId and route selection to the registered owner, including search-only rows and collision refusal. Current thread draft and source/native task state must survive failed navigation.
- Keep ordinary embedded/remote resume policy and explicit model/permission ownership; no provider/Cloud activation, source authority grant or held receiver route is introduced.

ALab interactions:

- No owned new CloudOperation exhaustive match or changed search-page/resume-navigation callsite was found; existing owned CloudOperation fixtures match Request/Shutdown selectively and are outside the new navigation path.
- New Cloud navigation constructs destination settings and hydrates scoped history before commit. The final local worker handoff is not native executable admission or a guarantee of remote transactional rollback.

Limits and missing controls:

- The actual worker test injects real candidate stream/settings into run_worker_with_prepared and checks old loaded/status/warnings across EOF/burst/shutdown/abandon/reused/stale cases. It does not itself prove signed Cloud service admission.
- Require actual TUI failure+draft, backend routing and streamed transport bodies on integrated source. Source branches and prepared data are not proof that a displayed task has durable input acceptance.
- The bounded immediate stream probe establishes only the local readiness boundary it observes; it does not promise future stream health or a remote durable-commit receipt.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-app-server-client --lib` | `prepared_resume_keeps_the_previous_stream_until_handoff` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-app-server-client --lib` | `failed_replacement_keeps_old_thread_until_new_attachment_succeeds` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-cloud-client --lib` | `attach_preserves_opaque_events_and_native_error_details` | `codex-rs/cloud-client/src/client_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `cloud_resume_picker_reconciles_admitted_steers` | `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `overview_opens_a_search_only_task_on_the_search_backend` | `codex-rs/tui/src/app/agents_overview_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |
| `codex-tui --lib` | `overview_keeps_rows_and_routes_actions_to_the_owner` | `codex-rs/tui/src/app/agents_overview_tests.rs` @ `c8e4cbcd9476` | Source declaration only. |

## b1573f102413 — adopt

Accept per-environment prompt and permission grouping with stable snapshots behind the default-off feature. Ready local/remote captures describe their own profile; unknown readiness withdraws the old scope and uses explicit thread-default fallback. Flag-off legacy descriptions remain unchanged. Model-visible descriptions grant no native permission.

Affected contracts: `product-policy`, `account-isolation`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Keep flag-off ALab defaults and SourceV1/all-root receiver hold.
- Model-visible permission text is data; keep existing captured native filesystem/environment/grant checks as authority.

ALab interactions:

- No changed ALab marker in native environment/permissions construction; no owned direct PermissionDescription/PermissionsState caller found.
- Feature-specific scoped snapshots persist enough format state to withdraw old scopes across model/flag/readiness changes.

Limits and missing controls:

- Snapshot/render tests alone are not native enforcement evidence; actual Astra scenario covers context selection only.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `context::world_state::permissions::tests::snapshots` | `codex-rs/core/src/context/world_state/permissions_tests.rs` @ `c8e4cbcd9476` | Scoped A/B, removed B, same scope, fallback defaults and scoped→legacy snapshot/diff cases. |
| `codex-core --lib` | `renders_only_newly_approved_prefixes` | `codex-rs/core/src/context/world_state/permissions_tests.rs` @ `c8e4cbcd9476` | Saved command approval delta applies to all named environments without re-injecting unchanged full rules. |
| `codex-core --lib` | `legacy_snapshot_deserializes_and_only_suppresses_matching_full_permissions` | `codex-rs/core/src/context/world_state/permissions_tests.rs` @ `c8e4cbcd9476` | Legacy persisted hashes remain compatible and stale hashes cause current text. |
| `codex-core --lib` | `legacy_single_environment_snapshot_does_not_change` | `codex-rs/core/src/context/world_state/environment_tests.rs` @ `c8e4cbcd9476` | Feature-off legacy single environment comparison remains stable. |
| `codex-core --lib` | `failure_context_limits_total_detail_bytes_at_utf8_boundaries` | `codex-rs/core/src/context/world_state/environment_tests.rs` @ `c8e4cbcd9476` | Readiness failure context keeps existing actual UTF8 byte bound. |
| `codex-core --test all` | `astra_switches_environments_for_the_rest_of_the_active_turn` | `codex-rs/core/tests/suite/scenarios.rs` @ `c8e4cbcd9476` | Actual named remote/local environment configurations and model request snapshot; active-turn switch does not alter future defaults.; Real ExecServerProcess fixture required; skip_if_no_network must not be counted as body execution. |

## 65a5410f72da — adapt

Accept omitted, unknown or normalized trusted runtime request-country metadata captured through steps, turns, subagents, reviewers, prewarm and compaction, including WebSocket continuation/fallback. Send it only to the Codex backend. Adapt owned recovery to discard the old host value and use the fresh trusted destination binding; the datum is not native execution authority.

Affected contracts: `provider-recovery`, `account-isolation`, `product-policy`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Preserve actual ALab source/auth-factory admission/diagnostic privacy hooks around request signing/socket use.
- Do not replay old trusted runtime country from serialized tree/CLI settings; actual fresh trusted host must reapply it on new runtime/cold resume.
- Required adaptation in codex-rs/alab-core/glue/exec_tree_settings.rs: Exhaustive CodexThreadSettingsOverrides match and full restore literal must handle request_country. Explicitly exclude source country from wire capture; reconstruct from fresh.request_country alongside fresh turn_extension_init. Review fresh-binding semantics; do not silently serialize source host value. (source locations 46, 119)
- Required adaptation in codex-rs/alab-api-tests/src/websocket_nested_error_ids_tests.rs:191; codex-rs/alab-api-tests/src/sse_diagnostic_context_tests.rs:290; codex-rs/alab-api-tests/src/websocket_invalid_request_tests.rs:28; codex-rs/alab-api-tests/src/websocket_deflate_tests.rs:242; codex-rs/alab-api-tests/src/websocket_error_status_tests.rs:151; codex-rs/alab-api-tests/src/websocket_semantic_error_ids_tests.rs:248; codex-rs/alab-core/glue/api_request_diagnostics.rs:751; codex-rs/alab-core/glue/websocket_request_diagnostics_tests.rs:70: Add request_country: Default::default() to actual ResponsesApiRequest fixture literals, preserving old omission and exact payload/byte/privacy expectations.
- Required control (not implemented evidence) in codex-rs/alab-core/glue/exec_tree_wire_tests.rs: Extend real existing settings roundtrip with source DE versus fresh US/unknown/omitted; assert fresh binding and no source country wire field. All runtime remains unrun.
- At 65, explicitly discard source request_country in the exhaustive owned wire capture, restore fresh.request_country.clone() beside fresh turn_extension_init, and preserve exhaustive field handling. In the real settings roundtrip use source DE/fresh US (plus unknown/omitted controls), assert fresh wins and no source DE value enters serialized wire. Eight default fixture fields alone do not establish this authority boundary.

ALab interactions:

- Responses property comparison intentionally excludes country so same WebSocket connection is reused while current per-response metadata still changes.
- App-server public settings translation assigns None; config file/persisted settings cannot author this new datum. Native permission/account/source/goal gates are unchanged.

Limits and missing controls:

- from_iso_code validates two ASCII letters/normalization; it does not prove real ISO assignment or host location.
- Country must not leak to custom provider: production code explicitly guards uses_codex_backend; new source tests primarily exercise Codex mock path.
- Require an actual custom-provider request omission control before claiming country privacy across providers; the production uses_codex_backend guard and Codex mock tests are source evidence only.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-protocol --lib` | `request_country_rejects_malformed_values` | `codex-rs/protocol/src/request_country_tests.rs` @ `c8e4cbcd9476` | Rejects malformed two-letter input; normalization is format validation, not ISO country-membership proof. |
| `codex-api --lib` | `direct_serialization_preserves_websocket_request_payload` | `codex-rs/codex-api/src/endpoint/responses_websocket.rs` @ `c8e4cbcd9476` | Actual API→WS serialization preserves omitted/DE/null country without changing ordinary envelope. |
| `codex-core --lib` | `prewarm_turn_keeps_its_captured_settings_after_defaults_change` | `codex-rs/core/src/session/mailbox_prewarm_tests.rs` @ `c8e4cbcd9476` | Actual captured warmup setting remains old while new defaults affect future turn. |
| `codex-core --lib` | `country_updates_preserve_captured_steps_without_model_switching` | `codex-rs/core/src/session/step_activation_tests.rs` @ `c8e4cbcd9476` | Actual live country-only update, captured step stability, absent model-switch requirement. |
| `codex-core --test all` | `country_settings_follow_step_and_turn_lifetimes` | `codex-rs/core/tests/suite/model_request.rs` @ `c8e4cbcd9476` | Source cases: future settings only, live country update without model switching; Actual model requests distinguish future-thread DE→US, optional liveJP, and explicit unknown null. |
| `codex-core --test all` | `websocket_country_changes_on_one_connection` | `codex-rs/core/tests/suite/agent_websocket.rs` @ `c8e4cbcd9476` | Actual shared WS continuation/warmup request country changes DE/US/null/omitted on same connection. |
| `codex-core --test all` | `request_country_survives_websocket_to_http_fallback` | `codex-rs/core/tests/suite/agent_websocket.rs` @ `c8e4cbcd9476` | Actual fallback retains request datum on HTTP. |
| `codex-core --test all` | `remote_compact_v2_retains_metadata_from_resumed_history` | `codex-rs/core/tests/suite/compact_remote.rs` @ `c8e4cbcd9476` | Actual resume/remote-compaction requests carry hostDE, explicitunknown and retain ordinary harness-metadata exclusion. |
| `codex-core --test all` | `review_resolves_inherited_summary_preferences` | `codex-rs/core/tests/suite/review.rs` @ `c8e4cbcd9476` | Actual reviewer requests inherit hostDE then explicitunknown while summary/model defaults are preserved. |
| `codex-core --lib` | `settings_round_trip_requires_fresh_bindings_and_preserves_explicit_clears` | `codex-rs/alab-core/glue/exec_tree_wire_tests.rs` @ `0b4f0adc0028` | Actual owned tree settings serializer+fresh restore; must be extended to prove fresh country binding rather than old source replay.; Current baseline has no new request_country field; adaptation is required before final compilation.; Owned include: compiled by listed host package, not physical glue directory |

## 81b995a6c9a4 — adapt

Accept attempt-owned pre/post model-request lifecycle sharing for sampling and local/remote compaction only after preserving ALab terminal-failure custody across steering. This first breaking commit moves a decoded terminal error behind a synchronous Post hook before the consumer receives it, enabling the source-derived follow-up schedule described below. No behavioral RED has been executed.

Affected contracts: `provider-recovery`, `execution-continuity`, `exec-handoff`, `goal-storage-recovery`, `product-policy`, `qualification-fixtures`.

Required integration behavior:

- Retain actual terminal_steering_tests marker, decoded failed status, real steer, original terminal/retry-exhaustion verdict, one actual request and true task cancellation wins.
- Use dependent92 existing per-stream carrier only as observed-outcome await custody; never publish usage/charge/commit/rollback/completion early.
- Preserve ALab weekly capacity/quota retries and real foreground/local+remote compaction hooks while rebasing native client signatures.
- Required adaptation in codex-rs/core/src/client/response_delivery.rs: Minimal plausible source-only extensionpoint: tiny marked call into ALab-owned helper before await of actually observed Err, reusing existing92 per-stream carrier solely to retain observed-outcome await custody. Consider locally observed EOF→originalStreamerror in finish_eof too. No allocation claim before implementation; existing carrier reuse is a candidate; no implementationauthorized. (source locations 81:38-48, final:77-98, final:143-159)
- Required adaptation in codex-rs/alab-core/glue/terminal_steering_tests.rs: hooks_for_model_request becomes private; remove/replace only now-private redundant configuration assertion without widening production API or weakening actual held-hook/steer/verdict/requestcount oracles. (source locations 235)
- Required control (not implemented evidence): Existing real terminal-steering controls provide genuine candidateRED; rerun actualintegratedsource after mechanicalfixture adaptation. Extend actual EOF/post hook retryability control only if proposed carrier hook includesEOF. Do not relabel source inference or setup/compilefailure as runtimeRED.
- Preserve already observed EOF and its original stream-failure/retry-exhaustion classification across the synchronous failed post-hook as well as an explicit Err. True task cancellation remains independently decisive. Never mark usage posted or goal/database completion merely to retain terminal observation.

ALab interactions:

- 81 ResponseDelivery38 classifiesErr then awaits synchronous Post42-48;81 turn3051 HTTP preempt exitsOk(needs_follow_up)3066 before Err delivery. Before81 consumer ownedErr before post hookwait.
- 92/final c8 still has gap: ResponseDelivery77/86 awaits failedhook; settlement_started store118 applies onlyCompletedafterhooks; finalturn3063false→followup. Outer ALab retry hooks cannot preserve an error not delivered.
- True turn cancellation remains independent/priority via consumer accept_for_turn and producer owner/tx close hook cancellation.

Limits and missing controls:

- Synchronous Post hooks delay terminal delivery but no longer defer default streaming tools; configured AfterCompleteResponse in92 is a distinct output policy.
- Background hook tasks own captured session/step; source-retirement/taskcancel cleanup remains an integrated runtime obligation.
- The suggested existing per-stream carrier is present only at92/final c8. Any stepwise prefix ending at 81 must supply a source-specific retained terminal-observation contract before admission; the later record is not an implementation or fix. EOF coverage is a proposed actual producer/consumer control, not an executed test.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --test all` | `model_request_hooks_report_failures_and_retries_but_cannot_veto` | `codex-rs/core/tests/suite/hooks.rs` @ `c8e4cbcd9476` | Actual request retry pre/post pairing and informational hook errors cannot veto native request. |
| `codex-core --test all` | `model_request_observer_reports_cancellation` | `codex-rs/core/tests/suite/hooks.rs` @ `c8e4cbcd9476` | Actual observer receives cancellation and lifecycle identity. |
| `codex-core --test all` | `model_request_post_hook_orders_tool_dispatch` | `codex-rs/core/tests/suite/hooks.rs` @ `c8e4cbcd9476` | Source cases: default runs the tool before the response ends, an observer does not hold the tool, buffered output waits for completion without hooks, streaming output and tools precede a synchronous hook, interrupt discards the held response and its charge, the response bill blocks its own tool; Six final actual Core scenarios distinguish streaming eager/observer/sync, configured after-complete, real interrupt/nocharge, and bill refusing own tool.; Final labels include dependent92 behavior; supplied tests do not cover ALab held failed-hook→steer verdict custody. |
| `codex-core --test all` | `foreground_compaction_only_installs_and_bills_after_its_post_hook` | `codex-rs/core/tests/suite/compact.rs` @ `c8e4cbcd9476` | Real local compaction post-hook gate; interrupt installs no checkpoint/posts no bill, later accepted compaction bills once. |
| `codex-core --test all` | `async_compaction_reuses_pending_work` | `codex-rs/core/tests/suite/scenarios.rs` @ `c8e4cbcd9476` | Source cases: auto uses pending result, auto falls back after failure, auto falls back after failure before checkpoint, auto falls back after account changes, auto can be interrupted, manual without background, manual uses pending result, manual falls back after failure, manual falls back after account changes, manual can be interrupted, pending adoption still over budget; Eleven real candidate reuse/fallback/account-change/interrupt/adoption modes; observed background hook pair/bill runs once on adopted candidate.; Uses actual mock model/loopback MCP hook and existing executor prerequisite; body execution required. |
| `codex-core --lib` | `collect_queued_incomplete_usage_on_cancellation` | `codex-rs/core/src/compact_remote_v2.rs` @ `c8e4cbcd9476` | Source cases: turn interrupt discards queued incomplete usage, session shutdown preserves queued incomplete usage without foreground updates; Two direct native collector controls: foreground cancel discards queued incompleteusage; background shutdown preserves bill without foreground history.; Collector queue control, not transport/DB/native-goal persistence test. |
| `codex-core --lib` | `collect_queued_complete_response` | `codex-rs/core/src/compact_remote_v2.rs` @ `c8e4cbcd9476` | Source cases: background bills a complete response even without a summary, turn interrupt discards a queued complete response, session shutdown bills a queued complete response even without a summary; Three native collector cases preserve background billing even no summary and discard queued foreground result on turn interrupt.; Collector queue control; real transport/hook compaction coverage is separate above. |
| `codex-core --lib` | `steering_cannot_erase_a_received_cyber_policy_refusal` | `codex-rs/alab-core/glue/terminal_steering_tests.rs` @ `0b4f0adc0028` | ALab actual decoded provider failure held at synchronous Post, real steer, original terminal reason, one actual model request.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-core --lib` | `steering_cannot_erase_a_received_session_budget_refusal` | `codex-rs/alab-core/glue/terminal_steering_tests.rs` @ `0b4f0adc0028` | Same actual gate/steer contract for provider SessionBudgetExceeded; distinct from local rollout and native goal budgets.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-core --lib` | `steering_cannot_hide_an_exhausted_ordinary_retry_budget` | `codex-rs/alab-core/glue/terminal_steering_tests.rs` @ `0b4f0adc0028` | Actual ordinary retry exhaustion remains failure under steer, does not invent backend reason.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-core --lib` | `an_actual_interrupt_keeps_priority_over_a_received_refusal` | `codex-rs/alab-core/glue/terminal_steering_tests.rs` @ `0b4f0adc0028` | Real turn interruption wins while decoded failure awaits its post hook.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-core --lib` | `weekly_reset_retries_original_local_and_remote_compaction` | `codex-rs/alab-core/glue/weekly_compaction_tests.rs` @ `0b4f0adc0028` | Actual local/remote foreground earned-reset retry keeps original request and compaction state.; Owned include: compiled by listed host package, not physical glue directory |

## 92c1a4dc2e1f — adopt

Accept optional processing after completion, an event-count-bounded ordered buffer, and producer-owned completed rollout billing before output release. SamplingCharge.posted suppresses duplicate rollout billing while ordinary token history/contributors still run. Adoption depends on carrying the required 81 terminal/EOF custody adaptation; this commit does not resolve that gap.

Affected contracts: `execution-continuity`, `goal-storage-recovery`, `exec-handoff`, `provider-recovery`, `qualification-fixtures`.

Required integration behavior:

- 81 received-Err/steering adaptation is a required dependency; added generic hook tests cannot mask existing ALab oracle.
- Do not equate SamplingCharge.posted or settlement_started with native goal mutation receipts/database durability/accepted-input authority.
- Preserve installed compaction checkpoint/analytics and one completed bill before refusing later work; do not retry host accounting failure or invalid charge.
- Required control (not implemented evidence): MAX_BUFFERED_EVENTS=65_535 is an event count bound, not byte bound. No supplied specific overflow boundary test found; add actual producer/receiver or Core stream control at the limit/overflow with no held-item/handler release and deterministic cleanup before claiming that boundary.

ALab interactions:

- Raw low-level client/prewarm ResponseStream has no sampling settlement; SamplingRequest ordinary/foreground/background passes real session/turn.
- record_token_usage_info still invokes TokenUsageContributor after budget result even when error; posted marker skips only shared rollout/host billing. GoalExtension callback permit/retirement/final writer integer accounting remains separate.
- Completed producer bill survives receiver loss only after actual settlement begins; pre-hook/interruption before billing deliberately discards output/charge. No blanket claim that arbitrary response usage always survives cancellation.
- Existing per-stream carrier reads only steering and mailbox branches before waiting; Err classification occurs before wait_for_bill_only and uses original reason.

Limits and missing controls:

- Native goal retained tokens and committed tool completion require separate actual integrated controls; upstream bill marker does not prove them.
- Feature default streaming unchanged; ALab recovery configured response mode must follow actual captured layers/fresh host choices, not silently force buffered mode.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `load_config_resolves_response_process_mode` | `codex-rs/core/src/config/config_tests.rs` @ `c8e4cbcd9476` | Actual default streaming/configured after_complete_response TOML parse. |
| `codex-core --test all` | `steer_interrupts_and_drains_websocket` | `codex-rs/core/tests/suite/pending_input.rs` @ `c8e4cbcd9476` | Source cases: completed_reasoning, discarded_reasoning, rejected_interrupt, missing_acknowledgement; Four actual websocket steering/ACK cases: buffered stale patch not applied, actual follow-up contains aborted output, provider units17 billed exactly once. |
| `codex-core --test all` | `invalid_provider_rollout_budget_units_fail_without_retry` | `codex-rs/core/tests/suite/rollout_budget.rs` @ `c8e4cbcd9476` | Real negative provider charge rejects buffered tool/history, one request/no filesystem effect. |
| `codex-core --test all` | `host_accounting_reports_raw_usage_and_preserves_error_semantics` | `codex-rs/core/tests/suite/rollout_budget.rs` @ `c8e4cbcd9476` | Source cases: host exhausts budget, accounting failure never retries inference, incomplete response keeps original error; Actual host Exhausted/Fail/Incomplete variants preserve raw usage and original error; no rerun inference. |
| `codex-alab-autopilot --lib` | `terminal_finalizer_accounts_exact_post_dispatch_callback_for_every_stopped_status` | `codex-rs/alab-autopilot/src/goal_retained_usage_tests.rs` @ `0b4f0adc0028` | Actual native goal terminal statuses account exact retained callback to original durable goal.; Tests native goal accounting, not the new upstream rollout producer; preserve as integrated regression, not proof of new billing alone.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-alab-autopilot --lib` | `terminal_paths_flush_retained_tokens_before_stopping_a_later_manual_turn` | `codex-rs/alab-autopilot/src/goal_retained_usage_tests.rs` @ `0b4f0adc0028` | Actual native DB failure/retained tokens drain before next manual terminal stop.; Owned include: compiled by listed host package, not physical glue directory |

## f226c9ccfb97 — adapt

Accept budget rechecks after parallel admission/start observers and immediately before MCP handoff. Already running or committed operations can finish after a peer stops. OnceLock tracks each exact terminal outcome and prevents duplicate Aborted callbacks. Reapply the owned invocation, retained-completion and transfer-attribution hooks across the rewritten abort branches.

Affected contracts: `goal-storage-recovery`, `execution-continuity`, `exec-handoff`, `provider-recovery`, `qualification-fixtures`.

Required integration behavior:

- Keep native goal completion register before eager dispatch/id reservation, child cancellation token, retained completion lease and shutdown/turn-owner drains.
- Exactly one Invocation per actual dispatch must scope normal handler+finish and only its own outer abort callback, including BeforeDispatch refusal; no provider call ID reuse as identity.
- Retain original transfer reason for real account handoff; budget stop must not become user cancellation or committed goal success.
- Required adaptation in codex-rs/core/src/tools/parallel.rs: Reapply existing ALAB(goal-completion) prepare/retain around full new completion future; preserve .with_model_goal_invocation on actual dispatch and single terminal_outcome.set(Aborted)-won notify path. Move existing transfer::attribute_abort to actual stopped_response(User) result, leaving BeforeDispatch truthful. Removed aborted_response helper must not be restored as stale compatibility. (source locations 171, 182, 373, 399, 416, 439, 442)
- Required control (not implemented evidence): Run existing actual native partial-finish/forced waiter/failure/API pause controls against rebased OnceLock path; generic PausedToolLifecycle test alone does not prove durable GoalService mutation or owned observer completion.

ALab interactions:

- Registry owns terminal_outcome.set before callbacks. Once Completed is claimed, cancellation awaits dispatch/finish; Aborted output is typed via was_aborted and handler-less work never gets Completed.
- Canonical global rollout stop is separate from GoalService integer/database budget; no change to held native mutation chronology/rollback/admission.
- MCP start-gated stop suppresses actual peer operation; once peer result received new stop does not erase it. Earlier native permission/approval and network policy callbacks remain.

Limits and missing controls:

- Terminal outcome claimed is not proof every observer finished; ALab retained-future owner must still await the transferred abort/join/notify chain.
- Last checks admit new work; they deliberately do not retroactively cancel issued MCP/native operations.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `mcp_budget_stop_at_handoff` | `codex-rs/core/src/session/turn_start_mcp_tests.rs` @ `c8e4cbcd9476` | Source cases: stop_before_send, stop_after_send; Two actual MCP/reader gates: beforehandoffstop→zero peer calls+Aborted; aftersend→onepeer+Completed despite peerbudget stop. |
| `codex-core --lib` | `completed_and_budget_stopped_tools_report_their_own_lifecycle` | `codex-rs/core/src/tools/parallel.rs` @ `c8e4cbcd9476` | Actual native tool runtime/registry gates: completed handler survives cancel at finish, queued and onstart-held work neverinvokes handler after peerstop, one own outcomeeach.; Uses fixture lifecycle contributor, not actual durable goal extension. |
| `codex-alab-autopilot --lib` | `a_claimed_tool_finish_survives_forced_waiter_teardown_without_replaying_its_outcome` | `codex-rs/alab-autopilot/src/model_blocked_partial_finish_tests.rs` @ `0b4f0adc0028` | Actual native goal mutation, held ToolFinish, forced waiter destruction; retained completion publishes only its own invocation once.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-alab-autopilot --lib` | `a_claimed_native_failure_survives_waiter_teardown_without_becoming_a_model_stop` | `codex-rs/alab-autopilot/src/model_blocked_partial_finish_tests.rs` @ `0b4f0adc0028` | Real DB failure after handlerdispatch remains nativefailure under teardown, never fabricated modelstop.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-alab-autopilot --lib` | `an_api_pause_during_claimed_tool_finish_defeats_its_later_source_delivery` | `codex-rs/alab-autopilot/src/model_blocked_partial_finish_tests.rs` @ `0b4f0adc0028` | Real newer API pause defeats old claimed finish publication.; Owned include: compiled by listed host package, not physical glue directory |
| `codex-core --lib` | `cancelled_wait_closes_once_with_the_transfer_reason` | `codex-rs/alab-core/glue/exec_transfer.rs` @ `0b4f0adc0028` | Actual account-transfer cancellation emits one interruption with account-handoff reason, not user blame.; Owned include: compiled by listed host package, not physical glue directory |

## c8e4cbcd9476 — adopt

Accept finite positive fractional rollout allowance, mutually exclusive legacy integer/fractional limits, compensated nonnegative accumulation, the specified equality/one-ULP comparison and optional initial-context reminders. This upstream shared rollout budget does not migrate the separate native GoalService integer budget or database chronology.

Affected contracts: `goal-storage-recovery`, `product-policy`, `provider-recovery`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Keep rollout budget opt-in/default configuration and actual explicit model/catalog settings; no global native goalbudget migration.
- Keep exact persisted integer limit/threshold data; choose onlyone limit field. Source validation rejects both even when the feature is disabled.
- Provider opaque units/host accountant remain distinct from local weighted token formula; host success must not also charge local tree.
- Required control (not implemented evidence): Existing actual suite covers0.1+0.2 equality/material overspend and fractional reminders. No dedicated supplied numerical oracles found for small charges after a large posted total, near f64::MAX compensation/saturation, and runtime threshold above 2^53; add focused real RolloutBudget/BudgetUnitsSum controls before making those stronger numericguarantees.

ALab interactions:

- Config now resolves limit_budget f64 and reminder_at_context_start default true; source legacy limit_tokens preserved as config data, runtime effective units may be fractional.
- BudgetUnitsSum does not manufacture usage/receiptidentity; record_usage validates explicit provider units finite/nonnegative and weights finite from config.
- Thresholdcomparison uses ceil to i128 for i64 exact comparison; new context reminder canbesuppressed independently of crossed-threshold notifications.

Limits and missing controls:

- Fractional upstream shared session budget is not native durable goal budget; no old non-replay gap is cleared.
- Eventcountbuffer and message byte budgets remain separate; changing balance/display wording does not relax existing HTTP/WS metadata limits.

Source-declared candidates, all **not_run**. Upstream declarations use final target `c8e4cbcd9476`; owned declarations use comparison source `0b4f0adc0028`. Preserve and adapt the owned controls on the admitted integrated source.

| Package / target | Filter | Declaration source | Variant or scope limit |
| --- | --- | --- | --- |
| `codex-core --lib` | `load_config_resolves_rollout_budget` | `codex-rs/core/src/config/config_tests.rs` @ `c8e4cbcd9476` | Real fractional100000.5 TOML plus legacy exact integerthreshold serialization beyond2^53. |
| `codex-core --lib` | `load_config_rejects_invalid_rollout_budget_limits` | `codex-rs/core/src/config/config_tests.rs` @ `c8e4cbcd9476` | Missing/bothlimits (even disabled)/zero/infinite rejected byactual configconstructor. |
| `codex-core --test all` | `adds_weighted_initial_and_threshold_reminders` | `codex-rs/core/tests/suite/rollout_budget.rs` @ `c8e4cbcd9476` | Source cases: weighted tokens and initial reminder, opaque units and threshold reminders only; Actual model inputs preserve fractional60.25 balance and optional initial reminder/thresholdonly mode. |
| `codex-core --test all` | `exhausted_budget_respects_enforcement_on_current_and_later_turns` | `codex-rs/core/tests/suite/rollout_budget.rs` @ `c8e4cbcd9476` | Source cases: enforced, advisory, host without accounting; Actual model requests0.1+0.2 equality allowed then material overspend stops Controller; advisory and Hostwithoutaccounting remain distinct. |
| `codex-core --test all` | `subagent_usage_draws_from_the_shared_budget` | `codex-rs/core/tests/suite/rollout_budget.rs` @ `c8e4cbcd9476` | Source cases: local accounting, host accounting; Actual subagent charges shared Controller or host-accounting budget without secondlocalcharge. |
| `codex-core --test all` | `compaction_accounting_errors_fail_without_retry` | `codex-rs/core/tests/suite/rollout_budget.rs` @ `c8e4cbcd9476` | Source cases: local token usage, local provider units, remote v2 token usage, remote v2 provider units, local compaction host accounting failure; Five actual local/remote compaction token/providerunits/hostfailure cases do not retrycompletedbill. |
| `codex-core --test all` | `restates_the_current_remainder_after_compaction` | `codex-rs/core/tests/suite/rollout_budget.rs` @ `c8e4cbcd9476` | Actual installed compaction new context restates actualcurrentbalance under reminder policy. |

## Required terminal-custody adaptation at 81

The first breaking commit is 81b995a6c9a4. Its producer receives a decoded Err, starts a synchronous failed Post hook and withholds the error. The HTTP consumer can receive a real steer during that wait; with no interrupt channel it returns needs_follow_up instead of retaining the terminal verdict. At final c8, settlement_started is still set only for Completed after hooks, so92 has not closed the error schedule. These are source-derived interleavings, not executed behavioral failures.

A narrow candidate is an ALab-owned helper reached by a tiny marked producer hook after an actual terminal Err/EOF is observed and before awaiting its post-hook, retaining the per-stream observation through the existing consumer drain path. The later 92/final c8 carrier is a potential existing allocation to reuse, not a component already present at 81 or an implemented fix. True task cancellation must retain priority; no usage/charge/native completion may be published early. Any prefix must meet the same contract with its actual source.

Keep the existing actual terminal_steering_tests hook marker, decoded status, real steer, original terminal/retry-exhaustion verdict and request-count oracles. Only the newly private helper assertion is redundant; do not weaken the behavioral controls or label compilation/setup failure as RED. Add an actual EOF/post-hook control for that variant. The separate f226 hook rebase must retain native invocation and claimed completion/transfer ownership through terminal_outcome; a generic observer predicate is not durable native completion evidence.

## Required runtime boundaries

The shared rollout flush change affects all callers that encounter pending realtime synchronization. Preserve actual append/flush/error and ResultWrite/goal persistence boundaries. Box additions and sync_data/directory operations have concrete allocation/I/O effects; no zero-overhead, latency, power-loss, hardware-durability or stack-fix claim follows from this source review.

Country recovery must discard the old source runtime datum and bind the fresh trusted host value. The real settings roundtrip needs differing source/fresh values plus unknown/omitted cases and no old country on serialized wire. Default fields in API fixtures alone do not prove this boundary; country syntax normalization does not prove geographic truth.

Buffered response output has an event-count limit, not a byte limit. Fractional upstream rollout accounting remains separate from native GoalService integer/chronology semantics. New boundary/numerical tests explicitly listed above are proposals; no missing test name is invented.

Tests with network, native executor, platform, helper or service prerequisites must execute their actual bodies. A skip, timeout, setup error or declaration match cannot qualify a behavior. Shared child-process timeout tests are existing helper controls; they do not by themselves exercise the new entire remote-collection deadline.

Maintained upstream_intake snapshot/validate checks ordered source coverage, subjects, paths, decision classes and catalog identities. This and generated JSON parsing are source/data checks only; they do not establish runtime parity, generator equivalence, native admission, application certification or final feature evidence.
