<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream intake 2026-11-12b, part 3

Reviewed the final seven pending commits, `9908717ddb27` through `1f175fa3c4af`,
against ALab base `71f328417b3165cb4aede018d056ff9f17d9b352` and upstream target
`1f175fa3c4af6a3f2e8d51971c3734b4a0aaca6b`. Read their complete retained patches,
confirmed their identities and changed paths with Git, and compared the relevant
ALab hooks, `alab/features.toml`, and native source-use contract. The matching
per-commit record is external `review-part3.json` in the intake evidence directory.

This records semantic source review and required validation, not test results or
native qualification. No build, provider request, account mutation, merge, or
activation was performed. SourceV1 remains held. Permission boundaries, explicit
model/effort selections, packaged Ultra defaults and goal/account UI contracts
remain in force.

## Decisions

| Commit | Decision | Source finding and consequence |
| --- | --- | --- |
| `9908717ddb27` | adopt | Cwd-only updates stop rebuilding a permission profile through legacy `SandboxPolicy`. The active profile, explicit roots and exclusions survive while runtime workspace roots follow cwd. Retain ALab's saved restrictions and fresh-owner environment validation. |
| `ea1b6ad1cf2c` | adopt | Retained image placements are cleared before history insertion can scroll them. Direct Kitty preservation requires unchanged safe viewport geometry; Sixel and file-backed Kitty are redrawn. Keep ALab pet cleanup and structured draft ownership. |
| `cdfcc18dbaf3` | adopt | Comments/schema descriptions clarify the remote board and configured clock; a fixture token is renamed. No runtime acceptance, consumption, clock or activation behavior changes. |
| `fae87361f447` | adapt | Add durable local-tree suspension, acknowledged result persistence and ancestor-aware reloading, retaining ALab's richer capture and native admission. The changed task-start and history APIs require explicit adaptation below. |
| `94fc72960265` | adopt | Raw rollout traces are reduced only by their recording executable. Require code-cell thread IDs and structured agent-message metadata; remove legacy diagnostic guessing. ALab transfer/history codecs are separate and remain necessary. |
| `16e27626e654` | adapt | `StatusLine` owns native/extension content and geometry, and callers supply an already-padded content rectangle. Reapply ALab's allocation/measurement/render hooks without double padding, preserving the native draft/hint budget. |
| `1f175fa3c4af` | adopt | Box an image-backend error, normalize terminating `bail!` expressions, and name the unchanged OTLP diagnostic prefix bound. Error predicates, retry semantics and telemetry defaults do not change. |

## Required adaptations

For `fae87361f447`, preserve upstream's registration-time suspension fence.
`core/src/tasks/mod.rs::spawn_task` now returns `TurnInputSubmission`; the ALab
`task_activity` refusal must return an explicit non-admitted outcome rather than
bare `return`. Move the existing ALab `exec_input_state::startup_mailbox` call
with upstream's mailbox drain into the active-turn registration lock, after the
second `turn_starts_closed` check. Keep its restored-input `InputsInstalled`
behavior: a rejected new start must not consume mailbox contents, and resumed
input must retain its original delivery phase. Preserve the accounting-baseline
hook independently.

`alab-core/glue/exec_child_restore.rs` must stop applying `ok_or` after
`load_agent_model_context`: the upstream function now returns a model context
or an error, rather than an optional context. Retain bounded history loading and
error propagation. Preserve the ALab runtime start gate, tree seal, joined-reader
closure, managed-review/ephemeral capture and destination-owned authority. The
new upstream descriptor contains durable regular-turn identities and wake state;
it does not establish ALab's native End acknowledgement, parser/input state,
review delegate state or successor-process authority. It therefore does not
justify removing these adapters or activating SourceV1.

Also adapt `alab-core/glue/exec_message_publication.rs` to the now-optional
`MessageDelivery::publish` and `MessagePublication::result_write` field.
`restore_exec` must construct no live result-write receipt. `capture_exec` must
refuse a pending receipt even if no lifecycle observer exists: serializing only
sender/scope would silently discard the durable-write obligation. Add a focused
regression proving refusal leaves the outstanding receipt/count intact. Existing
observer-free portable fixtures should construct the typed pending snapshot with
`restore_exec`, since native publication now correctly returns `None` when it has
neither an observer nor a persistence receipt.

For `16e27626e654`, retain the existing one-line hooks at:

- `chat_composer/composer_layout.rs`: `alab_border::footer_layout`, before native
  status measurement/allocation;
- `chat_composer/status_surface.rs`: ALab status measurement, now using the
  `Size` returned by `FooterLayout::status_bounds`;
- the same file's status renderer: ALab rendering into the final status rectangle.

Upstream now insets `FooterRows::status_area`. Remove the second x+2/width-4
inset from owned `alab-tui/glue/composer_border.rs::render_status`, and align its
measurement helper with content-width semantics, including zero-width bounds.
The footer-cap preflight still starts with full composer width and must convert
that consistently. Keep upstream `StatusLine` and its cache invalidation; do not
restore `FooterContent`, the removed single-line extension conversion, or a
second geometry cache. Native hints and draft rows keep priority; passive panes
must retain their own native status and must not overwrite focused account hits.

## Validation required on the integrated source

These are actual source test names unless explicitly marked as an extension.
They have not been run for this intake.

- **Permissions (`9908717`)**: core
  `session_configuration_apply_preserves_profile_file_system_policy_on_cwd_only_update`;
  integration `settings_update_does_not_retarget_active_turn_environment`;
  ALab `captured_root_restrictions_survive_startup_without_relaxing_fresh_host_policy`
  and `environment_restore_requires_current_owner_workspace_and_configuration`.
- **Image lifecycle (`ea1b6ad`)**: `tui::inline_image_history_tests` contains
  `inline_history_erases_preview_before_scrolling_and_preserves_draft`,
  `direct_kitty_preview_survives_history_above_the_viewport`,
  `iterm2_file_preview_is_deleted_before_history_and_uploaded_on_the_next_draw`,
  and `direct_kitty_preview_is_repainted_when_history_can_move_the_viewport`.
  Also run `app::alab_pet_cleanup_tests`, including disabled-pet and full-screen
  overlay cleanup cases.
- **Documentation (`cdfcc18`)**: retain
  `remote_board_uses_the_existing_tools_and_session_identity`; no additional
  behavior test is needed for the comment/token-only change.
- **Tree suspension (`fae8736`)**: core unit
  `terminal_result_writes_require_confirmation_after_the_parent_claims_mail`
  and `saved_message_wake_checks_the_last_started_turn_during_admission`;
  integration `root_turn_suspension_preserves_unfinished_turn_history`,
  `graceful_suspend_preserves_a_finishing_child_result` (all eight parameterized
  cases), `suspended_tree_records_the_original_unfinished_turns`, and
  `tree_suspension_leaves_an_ephemeral_turn_running`.
  Keep ALab tree tests `captures_active_and_idle_children_after_all_writers_stop`
  (the existing root/child/grandchild case),
  `captures_review_parent_and_original_delegate_with_consumer_state`,
  `captures_review_result_when_delegate_completes_after_freeze`,
  `captures_an_active_ephemeral_quota_member_without_root_admission`,
  `upgrade_forest_joins_source_reader_closure_before_returning_its_capsule`,
  `a_sealed_tree_refuses_input_before_settings_or_reservation_change`,
  `a_sealed_tree_leaves_automatic_wake_mail_unconsumed`,
  `restores_public_v2_children_with_residency_pins`, and
  `idle_root_restore_rechecks_retirement_after_native_admission_wait`.
  Extend an existing admission/queue regression if necessary to prove the moved
  startup hook leaves mailbox input untouched on the new suspension-fence refusal.
- **Trace replay (`94fc729`)**: rollout-trace reducer
  `replay_rejects_a_mismatched_manifest_schema`,
  `code_cell_lifecycle_links_nested_tools_waits_and_outputs`,
  `runtime_code_cell_ids_can_repeat_across_threads_and_after_resume`,
  `spawn_runtime_payload_targets_delivered_child_message`,
  `send_message_runtime_payload_targets_delivered_child_message`, and
  `agent_result_edge_links_child_result_to_parent_notification`.
  Schema equality alone is not cross-binary identity proof.
- **StatusLine (`16e2762`)**: all tests in
  `bottom_pane::chat_composer::composer_layout::alab_border::tests`, particularly
  `active_goal_reserves_a_row_without_moving_the_draft_or_stealing_account_hits`,
  `goal_account_rows_follow_footer_layout_with_constrained_draft_and_focus`,
  `passive_footer_keeps_its_native_status_without_replacing_focused_account_hits`,
  and `edition_switch_preserves_native_composer_with_a_recovery_button`.
  Extend the existing constrained-layout test with narrow widths/nonzero origins
  and exact painted/account-hit columns to catch duplicate padding.
  Retain upstream status-surface, status-line and UI-tree suites, including
  `status_rows_respect_host_budget_and_yield_to_the_draft`, replacement-content
  invalidation, focus/theme/resize behavior and clipped hyperlinks.
- **Cleanup (`1f175fa`)**: retain Cloud request failure/ownership tests and
  `otel/tests/suite/otlp_http_loopback.rs`; the refactor needs no new ALab behavior
  test. Verify existing source-bound Cloud and reconnect hooks survive the merge.

No reviewed commit changes the package/default precedence contract or supplies
the independent producer/controller/consumer/successor evidence required to lift
the SourceV1 hold.
