<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream source intake after 0184

This records 12 contiguous cached upstream commits after `0184dfa6a48f0110dd9c6835522492908e5abcf3` through `e5839cbc591df02408d5009e287887c7d57a9167`, compared with exact main `58b0a50f7d77de907876be64b94cb44a3d7bac35`. Decisions: **8 adopt, 4 keep-opt-in**. This review imports no upstream code and performs no Rust, build, test, native admission, compiler/pin/profile/cache/default change or publication. Tests below are statically located candidates, **not_run / UNRUN**, with no execution binding. The staged 61-commit candidate and its qualification are separate.

The latest supplied managed-fetch observation for this review is upstream cap `34c7cbb454b359ce7dd9f30f728de60618322755` around 02:23 UTC Nov17. The reviewed target remains e583. This document does not claim that cap is the current remote tip. A later local origin observation `f42fc6b92cec1664ae1e5340b10916608b6f8cb6` adds 14 paths beyond the fixed comparison, chiefly smoke/review work plus owned audio fixtures/Cargo and startup-account rendering. None is either new review/intake path; no production merge or review-base relabeling occurred.

All 12 canonical patches were independently reserialized with `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA` and matched the retained bytes/digests (966,621 bytes). All meaningful production/test changes were read; generated binary scope is described separately. Source references are immutable, not evidence that a scenario ran.

## 01. 98e1584a082234ae716a3cc69d4c6f2d3631cf2d — adopt

Simplify history and notes tool registration (#61764)

Generate one ordered history/notes executor vector from the registration macro, preserving per-step catalog messages, backend gating and namespace/tool order.

Classes: refactor. Feature owners: history-pagination, model-discovery, skill-discovery.

No owned direct caller of the removed per-namespace helpers or new builder was found. Retain actual account/source gating and packaged opt-in history/notes behavior.

Preserve the prior178 step-catalog builder requirement: Some(step) message override and explicit empty catalog coverage are still required; the existing extension fixture mostly observes the installed no-step builder, so do not claim those branches executed.

Canonical patch SHA256 `59854aaeda89d853aefe8d16d7ca67a5c7b119be3eee56c24dc20f1e3710266c`, 3,539 bytes.

## 02. 75fc67e8f4284a669eebd90ab6bc9ef7697e876e — adopt

Remove numeric positions from project responses (#61771)

Remove numeric position from Project response/store projections and generated clients; retain database ordering and beforeProjectId move semantics, including append for omitted/null.

Classes: protocol, refactor. Feature owners: history-pagination, execution-continuity, qualification-fixtures.

No owned Project/StoredProject literal or numeric response-position consumer was found in the bounded main census; no compatibility alias or ALab adaptation is justified by that census.

Read all text/schema/SDK changes; verify both compressed precomputed exports by bounded decompression and Project-property inspection separately. This is generated source conformance, not regeneration or app execution.

Preserve actual project membership, metadata timestamps, pagination/order and atomic import/notification controls; absence of a response position is not deletion or permission authority.

Canonical patch SHA256 `fac06477146bacb2ad47ead9e1bbf4772577f6c4b4609fb7150f70608b3c8643`, 754,083 bytes.

## 03. 776bd46f03a30df0caf3c37cd2fbbc70d4b7d7b4 — adopt

Centralize permission profile compatibility helpers in protocol (#61784)

Centralize the existing legacy compatibility policy and writable-root projection in protocol, retaining Windows fallback cwd/output/tmp/network and protected metadata carveouts.

Classes: refactor, security. Feature owners: product-policy, execution-continuity.

The sandboxing public re-export remains; no owned direct helper call or signature adaptation was found. Keep actual native executor/admission policy authoritative.

Protocol projection tests can validate the pure helper on this host; Windows restricted-token/elevated runtime behavior requires the supported Windows lane and remains UNRUN. No permission boundary is bypassed.

Canonical patch SHA256 `59dd5588756486e5a8cdf21d149a3b4f3faac68cd237b8602e48a1674ffffa1f`, 11,007 bytes.

## 04. 95946faafcd8f068e28610e8ae6a4c03060c1448 — adopt

Strengthen workspace-write configuration tests (#61785)

Strengthen explicit permission-profile and memories-root assertions while retaining Windows legacy fallback coverage and non-Windows working-directory writes.

Classes: fix. Feature owners: product-policy, execution-continuity, qualification-fixtures.

Test-only source change; preserve actual provider/config defaults. The host platform selects different branches, so one platform cannot qualify both Windows and non-Windows behavior.

Canonical patch SHA256 `453e1fa9555f7d53d083fa78e57983b6d984ccbba6c6bd735bb447a0743345f3`, 2,170 bytes.

## 05. cd1805370610e71eae7819b4ed28b90679a53f0e — keep-opt-in

Add lifecycle-managed Guardian housekeeping (#61795)

Retain default-off lifecycle-managed Guardian housekeeping: one coalesced root worker, flush before ordered evidence capture, cancellable per-batch work and joined accepted appends; do not activate it in ALab.

Classes: feature, background, security. Feature owners: product-policy, history-pagination, account-isolation, provider-recovery, execution-continuity.

GuardianV3 remains UnderDevelopment/default false and is absent from both ALab default files. No held SourceV1/all-root route is enabled.

The initial private Core review thread is superseded inside this cohort by08 dedicated sampler; do not preserve obsolete private-thread code or claim intermediate and final runtime equivalence without qualification.

Actual board append acceptance and completion precede cursor acknowledgement; errors/cancelled partial batches do not imply rollback or safe replay. Restart/replay and concurrent board writers remain explicitly unsupported.

Keep evidence as untrusted data, redact response contents in diagnostics, and retain exact root/history/account/native goal custody. One mpsc capacity-one wake and worker per eligible root is source architecture, not a measured cost reduction.

Canonical patch SHA256 `77a01daf0292bbfddd5aeb40b2c2946cfec0524b36fd79560c5132bbffbf1b5b`, 39,104 bytes.

Revisit: Before any ALab GuardianV3 activation, qualify the final05–08 cohort with actual account/route/network/cancellation/write-custody and nonforeground Goal/budget behavior; preserve default false and all native/SourceV1 holds.

## 06. 9544f09e7b880f373532b4fb5ca98313d2450d4f — keep-opt-in

Wire Guardian V3 housekeeping into the app server (#61797)

Wire housekeeping only from the server GuardianV3 flag; thread overrides cannot activate or disable server ownership, and actor instructions do not become housekeeper instructions.

Classes: feature, background, security. Feature owners: product-policy, account-isolation, history-pagination, execution-continuity.

Keep the server feature default false and ALab packaged defaults unchanged; thread config is not server authorization.

Use actual new/resumed actor-versus-housekeeper controls and disabled-server board absence. Final08 removes the temporary without_parent_instructions/private-thread path entirely; preserve the final isolated tool-free request semantics rather than obsolete compatibility code.

The new local board host disables remote notifications and stays within the existing agent tree; no remote/all-root/SourceV1 receiver authority is inferred.

Canonical patch SHA256 `165985e7c6bc4fa3b896ad9bf2ef6f2d71decc7d35f4b2c6f52c150250033bd6`, 11,598 bytes.

Revisit: Before any ALab GuardianV3 activation, qualify the final05–08 cohort with actual account/route/network/cancellation/write-custody and nonforeground Goal/budget behavior; preserve default false and all native/SourceV1 holds.

## 07. 5630d58680af22b71dc0d435049d2a16ec9dace4 — keep-opt-in

Add a dedicated Guardian v3 housekeeper inference client (#61798)

Retain the dormant tool-free dedicated housekeeper sampler/transport behind GuardianV3 opt-in: independent conversation/window, bounded transport retries, exact account/route resolution and unfinished-response cancellation.

Classes: feature, background, model, security. Feature owners: product-policy, provider-recovery, account-isolation, history-pagination, model-discovery.

This row introduces modules but08 performs wiring; qualify the cohesive final path. The existing Responses transport is not the ordinary Core ModelClient request/Goal policy pipeline.

Preserve per-batch immutable turn, per-attempt auth resolution, runtime account-bound HTTP factory, network-policy/socket key checks, owner revision checks, retry advice and typed fatal policy/build/auth behavior. A parent label is not actor/native Goal authority.

No actor instructions/tools, foreground usage receipt, native Goal admission or request-origin proof may be inferred from the new direct sampler. Before activation require actual account replacement/runtime credential, proxy/TLS/network-policy, cancellation and current-budget/usage behavior; request-country remains default-omitted, not imported trusted metadata.

One retained conversation/socket plus bounded whole-batch eviction changes cost; no latency, memory, power or traffic benchmark was run.

Canonical patch SHA256 `bbb23fdb155844c3af553e603537191c009ba55bfd4419d984fc8e576fe9fc3c`, 33,313 bytes.

Revisit: Before any ALab GuardianV3 activation, qualify the final05–08 cohort with actual account/route/network/cancellation/write-custody and nonforeground Goal/budget behavior; preserve default false and all native/SourceV1 holds.

## 08. 0370e34fe89585a3eb898e2dc21460b48abd1f83 — keep-opt-in

Route Guardian housekeeping through its dedicated sampler (#61799)

Route Guardian evidence through the dedicated sampler, remove obsolete private review-thread support, commit retained history only after successful board writes/cursor acknowledgement, and stop after error or dropped partial routing.

Classes: feature, background, refactor, security. Feature owners: product-policy, provider-recovery, account-isolation, history-pagination, execution-continuity.

Accepted appends remain joined through account change and root shutdown; do not drop write custody to make cancellation immediate. Account changes cancel future work but do not undo committed writes.

Retain whole completed-batch eviction, exact original-order routing and response-content redaction. Source tests cover HTTP fallback, bounded retries/advice, command-token rotation, managed401 renewal and accepted append shutdown; actual runtime is not_run.

Before enabling ALab require runtime-bound account replacement and credential/network isolation plus no unintended foreground Goal/actor effects. Partial-error/restart/replay/concurrent-writer semantics remain unsupported; default-off is not execution qualification.

Canonical patch SHA256 `60e43698bd69354327654739d5cc7258b04c0bdb54b792d3761b139f5b4285b5`, 50,425 bytes.

Revisit: Before any ALab GuardianV3 activation, qualify the final05–08 cohort with actual account/route/network/cancellation/write-custody and nonforeground Goal/budget behavior; preserve default false and all native/SourceV1 holds.

## 09. 4fc37acc87fd9eacbc7fd6bf099a4acbdd2252cc — adopt

Clarify managed human command checkpoint stability (#61800)

Clarify documented fresh human-command checkpoints for interactive CLI while app-server capability integration remains experimental; automatic/session/persistent approval cannot satisfy a prompt-user checkpoint.

Classes: other, security. Feature owners: product-policy, native-ui, execution-continuity.

Documentation only: no default, entitlement or enforcement implementation change. Preserve actual human decision and capable-connection rejection, ALab approval/policy fences and all human holds.

Canonical patch SHA256 `aeab80ae6233968bb9a464eb0b81f17bd77282694beabe7c462aa3f448f77426`, 2,937 bytes.

## 10. 662a634e88fcc809d77da393d75572c3d814cb27 — adopt

Preserve active turn approvals when an earlier turn finishes late (#61801)

Bind terminal approval cancellation and thread watch status to the latest started turn so late A completion does not cancel or mark B finished; preserve terminal event delivery for A.

Classes: fix, security. Feature owners: product-policy, native-session, execution-continuity, account-isolation.

Retain exact live pending-approval identity and command outcome controls. This is app-server event bookkeeping, not a substitute for ALab native policy observations, EOF invalidation or final decision authority.

The pending interrupt queue still stores request IDs without turn IDs and every terminal event drains it. Enqueue validates the target but outgoing request trace metadata does not veto response. A late A may source-reach acknowledging B interrupt before B terminal; this inherited remaining scope requires a deterministic interrupt-target control and is not repaired or runtime-reproduced by this row.

Canonical patch SHA256 `5ea02df6259a79b3bbd93bb6dc0e9b466720aa11b1f52320d398358b9a3b4e1a`, 14,382 bytes.

## 11. 2309bae1ae7a6dcab6dd0ade0fd37a7491b1de01 — adopt

Isolate app-server turn metadata by turn ID (#61817)

Index summaries/errors/final messages and command timestamps by actual turn, take only completing turn state, initialize inline-review metadata and clear bookkeeping on listener replacement/removal.

Classes: fix, refactor. Feature owners: native-session, history-pagination, execution-continuity, account-isolation.

No owned direct ThreadState/TurnSummary field consumer or literal was found. Preserve original native thread/turn identity, accepted-input/result receipt ownership and ALab real goal chronology; display/API metadata is not mutation authority.

Retain actual overlapping approval command start/completion count, per-turn error/start timestamp and default missing-summary controls. Per-turn map cleanup is source bounded by terminal/listener lifecycle, not a proved global memory bound under lost terminals.

Canonical patch SHA256 `f3b950b2f4f1624e8b3bd639e6361824aec748cbbc6445379a2f89d55de370b5`, 23,383 bytes.

## 12. e5839cbc591df02408d5009e287887c7d57a9167 — adopt

Preserve aggregate turn diffs across steering (#61820)

Retain aggregate TurnDiffTracker in the actual turn store through steering, keep each environment first display root, and emit the final aggregate before TurnComplete.

Classes: fix, refactor. Feature owners: execution-continuity, history-pagination, exec-handoff, native-session.

No owned removed with_environment_display_roots caller exists. Owned terminal-steering fixtures call outer run_sampling_request, whose signature remains; preserve their actual error/steer/interrupt oracles when integrating inherited81/92.

Keep execution roots and trust/native admission identity separate from display roots; a cwd change cannot rebind an earlier diff. Retain incremental patch updates and exactly one final aggregate, and discard terminal-abort tracker without inventing a successful final diff.

Turn-store cache is volatile display state; this does not make history/input/goal/metadata handoff portable or settle native result receipts. Preserve existing final flush, accepted-input checkpoint, child completion, retained context and actual native Goal completion controls.

Canonical patch SHA256 `3f998c8c5b3467c613a1e5fceea99c512ba4113b684b1815abc5011fe8e34eb9`, 20,680 bytes.

## Source interactions and limits

The bounded owned-path search covers all tracked `codex-rs/alab-*` and `alab/` source at the fixed main, excluding historical review/intake text. It finds no direct Guardian housekeeper, removed history helper, response Project numeric position, compatibility helper, or app-server TurnSummary consumer. Matches requiring contextual inspection are owned `terminal_steering_tests.rs` (outer sampling API retained) and `exec_memory_records.rs`'s **different** CompactionResumeMetadata.last_started_turn_id (unchanged upstream type). No demonstrated new ALab code adaptation is required by this batch. This is a source census, not proof of arbitrary downstream consumers.

Guardian05–08 is a single default-off cohort. The feature remains UnderDevelopment/default false in upstream features, and both packaged ALab defaults omit guardianv3. The final path removes the temporary private CodexThread/without-parent-instructions helper instead of retaining stale compatibility code. It uses a dedicated model conversation with no tools, store=false, separate conversation ID and fixed housekeeper instructions. Evidence/model output/channel descriptions remain data, never approval or Goal authority. Account-owned request resolution keeps the factory's proxy/TLS/network policy and runtime binding; the transport itself is a new direct Responses caller and does not inherit ordinary Core lifecycle accounting, terminal refusal delivery or the proposed ALab request-origin design. Before activation those boundaries require actual supported-lane controls; no cross-account/native budget safety conclusion is inferred merely from default-off or InternalSessionSource labels.

One root worker and capacity-one coalesced wake avoid unbounded wake queues, but no measured memory/CPU/network/latency benefit is claimed. Each routing pass lists channels, estimates input, may evict whole committed batches and can make up to the original attempt plus two transport retries, with a separate managed401 sequence. Accepted board appends run to completion under shutdown/account cancellation. Cursor acknowledgement and sampler-history commit follow all successful writes; partial writes can remain after failure. Stopped latch prevents a dropped routing future from replaying in this instance. Restart/replay and concurrent board writers remain unsupported, and these source controls are not crash/power-loss durability proof.

Row10 repairs latest-turn approvals/watch state, not every pending protocol request. PendingInterruptQueue is still `Vec<ConnectionRequestId>`; turn_interrupt_inner validates the requested active turn but stores no turn in the queue. Both late TurnComplete and TurnAborted call an unconditional whole-queue drain. Outgoing record_request_turn_id attaches trace context; ordinary send_response constructs a response without a current-turn check. A schedule with pending B interrupt then late A terminal is therefore a source-derived remaining premature-ACK candidate, not a demonstrated runtime result or a new regression introduced by this row. A deterministic late-A/B-interrupt control is still required before claiming interrupt response isolation.

## Generated project exports

Both changed zstd exports were independently hashed, decompressed with managed Python's standard compression.zstd and parsed as export JSON; all nested JSON-schema strings also parsed. Experimental Project appears in seven nested schema definitions and stable Project in two; none requires or exposes numeric position. Embedded v2/Project.ts in both exactly matches the target TypeScript file without a position property. Decoded sizes are 6,136,802 and 5,380,369 bytes; compressed sizes 181,304 and 172,469. This validates these field-level changes and embedded TypeScript, **not** a generator run or complete byte equivalence of every generated definition/SDK. No Rust or source schema regeneration occurred.

## Inherited obligations remain open

- `81b995a6c9a4c978f399775cfa5ce50702368749` / `92c1a4dc2e1ff83dd0d2f06f15f03df21b8cb2f2`: original decoded terminal Err/EOF must survive synchronous PostModelRequest hook awaits and real steering; actual TurnAborted retains priority. The later completed-billing marker is not alone terminal-error custody. The previously proposed thin owned carrier is UNIMPLEMENTED, not a main-branch fix. Preserve actual refusal marker/status/verdict/request-count tests.
- `f226c9ccfb9762eb5b8fa5d45b43262304f5a360`: reapply ALab invocation, goal-completion and exec-transfer custody through canonical exactly-once terminal outcome handling, including completed dispatch despite cancellation. Do not restore obsolete equivalent atomics or claim fake lifecycle callbacks prove native committed Goal completion. Real native budget/retained usage and actual completion/partial-finish controls remain required.
- `a394ff9cf66e9c17258077e8f8295178ca0c2025`, `598df2d0158544c3b9ce199d3d7ca9db5621af55`, `e51c551fb406486afb7ef5c3d43387f1be3c09e8`: preserve terminal restoration and real local/remote exec root, parent, accepted input, mailbox, history, stdio/hook and native Goal ownership. Shared app-server transport is not equivalence proof; remote/held SourceV1 all-root paths remain disabled/held.
- The prior history/notes Some(step) catalog-message/explicit-empty builder requirement remains open. Existing installed-extension/source declarations do not execute that missing branch.
- Generic/auth/quota error origin, natural tool/execution late-origin authority, stale GoalTool success reset versus valid retained reset, policy/EOF final decisions and ordinary/Compact/local-upgrade issuance remain separately owned, source-bound and unqualified where pending. This bookkeeping cohort does not close those bugs or replace actual native permission/current-budget/usage checks.
- No generated text, helper predicate, test name, cached source label, server receipt invented by a fixture, or intake decision establishes signed admission, installed adoption, actual result receipt ownership, full native application certification or a safe receiver for held full capsules.

## Candidate tests and runtime boundaries

There are 34 statically verified declarations across six package manifests, representing 44 declared cases when explicit test_case labels are counted. Candidate arguments below are function substring filters, not asserted exact generated libtest names. Each macro filter must run all listed variants; actual listing, nonempty suite counts, skip conditions and execution results must be captured later. Guardian's library has test=false; its actual housekeeping integration target is required. App-server/Core aggregate integration target is `--test all`. Module registration/manifest facts are retained in the external source packet. No test was run for this review.

| Row(s) | Package / target / function filter | Declared variants | Source and intended scope |
|---|---|---|---|
| 1 | `codex-history-notes-extension --test history_notes_extension installed_extension_exposes_and_invokes_history_notes_tools` | single declaration | `e5839cbc591d:codex-rs/ext/history-notes/tests/history_notes_extension.rs:53` — Installed tool order/invocation and provider/auth gating; does not close missing Some(step)/explicit-empty builder branch controls. **not_run** |
| 1 | `codex-history-notes-extension --test history_notes_extension history_notes_require_an_openai_provider_and_codex_backend_auth` | single declaration | `e5839cbc591d:codex-rs/ext/history-notes/tests/history_notes_extension.rs:429` — Installed tool order/invocation and provider/auth gating; does not close missing Some(step)/explicit-empty builder branch controls. **not_run** |
| 2 | `codex-app-server --test all projects_list_by_recency_and_preserve_metadata_timestamps` | single declaration | `e5839cbc591d:codex-rs/app-server/tests/suite/v2/projects.rs:56` — Actual project API order/metadata/persistence/filter/atomicity. **not_run** |
| 2 | `codex-app-server --test all projects_persist_and_assign_threads` | single declaration | `e5839cbc591d:codex-rs/app-server/tests/suite/v2/projects.rs:211` — Actual project API order/metadata/persistence/filter/atomicity. **not_run** |
| 2 | `codex-app-server --test all project_import_is_atomic_and_notifies_after_commit_in_order` | single declaration | `e5839cbc591d:codex-rs/app-server/tests/suite/v2/projects.rs:764` — Actual project API order/metadata/persistence/filter/atomicity. **not_run** |
| 2 | `codex-app-server --test all projects_validate_filters_cursors_and_sqlite_less_assignment` | single declaration | `e5839cbc591d:codex-rs/app-server/tests/suite/v2/projects.rs:916` — Actual project API order/metadata/persistence/filter/atomicity. **not_run** |
| 2 | `codex-state --lib project_lifecycle_preserves_order_and_clears_assignments` | single declaration | `e5839cbc591d:codex-rs/state/src/runtime/projects_tests.rs:15` — Native SQL ordering/moves/pagination, independent of removed response field. **not_run** |
| 2 | `codex-state --lib project_move_reorders_projects_and_preserves_no_op_timestamp` | single declaration | `e5839cbc591d:codex-rs/state/src/runtime/projects_tests.rs:587` — Native SQL ordering/moves/pagination, independent of removed response field. **not_run** |
| 2 | `codex-state --lib project_list_cursor_round_trips_across_pages` | single declaration | `e5839cbc591d:codex-rs/state/src/runtime/projects_tests.rs:364` — Native SQL ordering/moves/pagination, independent of removed response field. **not_run** |
| 3 | `codex-protocol --lib compatibility_writable_roots_preserve_the_output_only_fallback` | single declaration | `e5839cbc591d:codex-rs/protocol/src/permissions_tests.rs:1456` — Pure compatibility roots/protected metadata projection; not Windows runtime certification. **not_run** |
| 3 | `codex-protocol --lib preserving_writable_roots_cannot_be_rebound_during_projection` | single declaration | `e5839cbc591d:codex-rs/protocol/src/permissions_tests.rs:633` — Pure compatibility roots/protected metadata projection; not Windows runtime certification. **not_run** |
| 3 | `codex-protocol --lib legacy_workspace_write_projection_preserves_symbolic_project_root` | single declaration | `e5839cbc591d:codex-rs/protocol/src/permissions_tests.rs:700` — Pure compatibility roots/protected metadata projection; not Windows runtime certification. **not_run** |
| 4 | `codex-core --lib add_dir_override_extends_workspace_writable_roots` | single declaration | `e5839cbc591d:codex-rs/core/src/config/config_tests.rs:6493` — Actual platform-conditional config assertions; Windows and non-Windows branches differ. **not_run** |
| 4 | `codex-core --lib memory_tool_makes_memories_root_readable_without_creating_or_widening_writes` | single declaration | `e5839cbc591d:codex-rs/core/src/config/config_tests.rs:6701` — Actual platform-conditional config assertions; Windows and non-Windows branches differ. **not_run** |
| 5,7,8 | `codex-guardian-v3 --test housekeeping routes_captured_originals_and_updates_descriptions` | transient_failures_route_each_original_once, exhausted_retries_leave_the_batch_pending, evicts_only_completed_batches, write_failure, new_destination_needs_description, only_routed_channels_can_be_edited, stalled_channel_creation, redacted_model_error | `e5839cbc591d:codex-rs/ext/guardian-v3/tests/housekeeping.rs:75` — Final08 sampler path, all eight declared routing variants; skip_if_no_network may skip execution. **not_run** |
| 5,7,8 | `codex-guardian-v3 --test housekeeping root_shutdown_waits_for_the_accepted_evidence_append` | single declaration | `e5839cbc591d:codex-rs/ext/guardian-v3/tests/housekeeping.rs:531` — Actual managed401 renewal with unchanged retry body, accepted append joined through root shutdown, and no persisted housekeeper rollout. Not command-provider rotation or arbitrary runtime account replacement. **not_run** |
| 5,7,8 | `codex-guardian-v3 --test housekeeping command_token_rotation_between_batches_reauthenticates_the_housekeeper` | single declaration | `e5839cbc591d:codex-rs/ext/guardian-v3/tests/housekeeping.rs:746` — Actual command-provider token rotation across two routing batches forces authenticated socket replacement. Not the managed401 or accepted-append shutdown schedule. **not_run** |
| 6,8 | `codex-app-server --test all v3_routes_the_first_live_message_while_the_actor_is_still_sampling` | new_root, resumed_root | `e5839cbc591d:codex-rs/app-server/tests/suite/v2/guardian_sync_session_tests.rs:814` — Server-owned opt-in, actor instruction isolation, first new/resumed live input; skip_if_no_network. **not_run** |
| 6 | `codex-app-server --test all thread_delete_removes_persisted_boards_even_with_agent_feature_disabled` | guardian_disabled, guardian_enabled | `e5839cbc591d:codex-rs/app-server/tests/suite/v2/thread_delete.rs:363` — Server disabled cannot be enabled by thread override; board cleanup and prior-state persistence. **not_run** |
| 9 | `codex-core --test all managed_checkpoint_requires_fresh_human_decisions` | single declaration | `e5839cbc591d:codex-rs/core/tests/suite/scenarios_managed_human_approval_tests.rs:26` — Actual managed checkpoint requires fresh human decisions even automatic/session approvals. **not_run** |
| 10,11 | `codex-app-server --lib late_turn_completion_keeps_the_next_command_approval_answerable` | single declaration | `e5839cbc591d:codex-rs/app-server/src/request_processors/thread_processor_tests.rs:1203` — Real late A stop gate/B approval; final actual command result, started timestamp and one start/one completion. **not_run** |
| 11 | `codex-app-server --lib test_handle_error_records_message` | single declaration | `e5839cbc591d:codex-rs/app-server/src/bespoke_event_handling.rs:3141` — Per-turn metadata and ordinary/missing-summary command bookkeeping. **not_run** |
| 11 | `codex-app-server --lib test_handle_turn_complete_emits_error_multiple_turns` | single declaration | `e5839cbc591d:codex-rs/app-server/src/bespoke_event_handling.rs:3922` — Per-turn metadata and ordinary/missing-summary command bookkeeping. **not_run** |
| 11 | `codex-app-server --lib test_handle_turn_interrupted_emits_interrupted_without_error` | single declaration | `e5839cbc591d:codex-rs/app-server/src/bespoke_event_handling.rs:3615` — Per-turn metadata and ordinary/missing-summary command bookkeeping. **not_run** |
| 11 | `codex-app-server --lib command_execution_started_helper_emits_once` | single declaration | `e5839cbc591d:codex-rs/app-server/src/bespoke_event_handling.rs:2572` — Per-turn metadata and ordinary/missing-summary command bookkeeping. **not_run** |
| 12 | `codex-core --test all apply_patch_aggregates_diff_across_multiple_tool_calls` | single declaration | `e5839cbc591d:codex-rs/core/tests/suite/apply_patch_cli.rs:2436` — Incremental patches then final aggregate after final answer/continued sampling. **not_run** |
| 12 | `codex-core --test all steer_during_final_flush_reaches_the_model_before_turn_completion` | read_only_steer, steer_with_patch | `e5839cbc591d:codex-rs/core/tests/suite/pending_input_persistence.rs:374` — Actual final flush steer with changed executor cwd, original roots, one start, accepted input once. **not_run** |
| 12 | `codex-core --lib tracks_same_absolute_path_across_multiple_environments` | single declaration | `e5839cbc591d:codex-rs/core/src/turn_diff_tracker_tests.rs:110` — Environment display-root isolation and invalidation, not native execution identity. **not_run** |
| 12 | `codex-core --lib displays_foreign_paths_relative_to_their_environment_root` | single declaration | `e5839cbc591d:codex-rs/core/src/turn_diff_tracker_tests.rs:147` — Environment display-root isolation and invalidation, not native execution identity. **not_run** |
| 12 | `codex-core --lib invalidated_tracker_suppresses_existing_diff` | single declaration | `e5839cbc591d:codex-rs/core/src/turn_diff_tracker_tests.rs:93` — Environment display-root isolation and invalidation, not native execution identity. **not_run** |
| 12 | `codex-core --lib steering_cannot_erase_a_received_cyber_policy_refusal` | single declaration | `58b0a50f7d77:codex-rs/alab-core/glue/terminal_steering_tests.rs:649` — Owned real terminal/steer/interrupt controls must survive inherited81/92 producer custody integration; fixtures/assumptions may need mechanical adaptation, never weakened. **not_run** |
| 12 | `codex-core --lib steering_cannot_erase_a_received_inference_hook_denial` | single declaration | `58b0a50f7d77:codex-rs/alab-core/glue/terminal_steering_tests.rs:654` — Owned real terminal/steer/interrupt controls must survive inherited81/92 producer custody integration; fixtures/assumptions may need mechanical adaptation, never weakened. **not_run** |
| 12 | `codex-core --lib steering_cannot_erase_a_received_invalid_prompt` | single declaration | `58b0a50f7d77:codex-rs/alab-core/glue/terminal_steering_tests.rs:659` — Owned real terminal/steer/interrupt controls must survive inherited81/92 producer custody integration; fixtures/assumptions may need mechanical adaptation, never weakened. **not_run** |
| 12 | `codex-core --lib an_actual_interrupt_keeps_priority_over_a_received_refusal` | single declaration | `58b0a50f7d77:codex-rs/alab-core/glue/terminal_steering_tests.rs:692` — Owned real terminal/steer/interrupt controls must survive inherited81/92 producer custody integration; fixtures/assumptions may need mechanical adaptation, never weakened. **not_run** |

Required missing controls are not replaced by this table: actual runtime-bound Guardian account replacement/network-policy/unfinished cancellation, arbitrary raw/direct transport Goal/budget isolation, supported Windows runtime behavior, late-A/B interrupt acknowledgement, history builder Some(step)/explicit-empty messages, and inherited native completion/error-origin/held receiver contracts. Existing test names and source assertions are candidates only.

## Provenance

Only this review and `alab/upstream/e5839cbc59-from-0184dfa6a4-intake.json` are source-record changes. The external packet at `~/.ubd/codex-upstream-next12-after0184-20261117/` retains canonical patches/index, source blob hashes, actual declaration/attribute/package/target/module facts, bounded owned caller census, generated-export checks and maintained validation/census output. Root supplied an initial pending snapshot and reading-progress note; this record's author independently read the canonical changes and made the final decisions. Intake schema/coverage validation is source bookkeeping, never runtime qualification.
