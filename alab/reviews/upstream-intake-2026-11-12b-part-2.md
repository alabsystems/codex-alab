# Upstream intake 2026-11-12b, part 2

Reviewed the seven actual diffs `bbd0d2b2fbc5` through `f242b49c8add`
against ALab base `71f328417b3165cb4aede018d056ff9f17d9b352` and integrated
frontier `ce3031018ef130e653ed4d9c5bb971652de97161`. The full batch target is
`1f175fa3c4af6a3f2e8d51971c3734b4a0aaca6b`. The external
`review-part2.json` retains each original SHA, subject and path list and passes
the intake validator for this exact seven-entry subset.

This is source review, not qualification. No build, test, provider request,
account change, merge or activation was performed. Tests below are required
verification, not results. SourceV1 remains held. No new default, background
activity, telemetry destination or credential authority is approved here.

## Required adaptation

`d0221bc94711` changes plan identity from a turn-derived string to a UUID retained
only while unfinished. Its `unfinished_plan_id` is local to `run_turn`. ALab's
`exec_child_suspension.rs` cancels/removes that task and restores a new one;
`exec_model_context.rs`, `exec_sampling_progress.rs` and `exec_input_batch.rs`
do not capture the new ID. A plan begun before suspension would receive another
ID on restoration. Capture the exact unfinished ID in ALab-owned turn/recovery
state before publishing its events, restore it, and retire it after completion.
Use minimal marked upstream hooks. Do not retain a completed ID or recapture a
new one during restoration. Two calls to `run_sampling_request` in
`terminal_steering_tests.rs` also need its new mutable argument.

Proposed integration regression: `exec_handoff_preserves_only_an_unfinished_plan_id`.
Emit a plan start and partial delta, transfer the same turn, require its continued
deltas/completion to retain the exact ID, then require a distinct ID for the next
plan. Keep snapshot validation and existing cancellation/refusal assertions.

## `bbd0d2b2fbc5` — adopt ascending-recency pagination repair

Only ascending recency with an available state DB scans an extra filesystem row;
the truncation step can then produce the next cursor for unindexed rollouts.
Other ordering paths retain their behavior. The ALab rollout-version hook is
disjoint. There is no account-authority or provider-filter expansion.

Run `thread_list_cold_ascending_recency_returns_next_cursor`: its two cold rows,
page size one, provider filter and final cursor exhaustion cover the changed
behavior directly.

## `c432cf0253ed` — adopt acknowledged Cloud rename

`ThreadSetName` trims the name and uses the existing bound Cloud `mutate` path.
The loaded name and `ThreadNameUpdated` notification change only after a server
acknowledgment. Client validation rejects empty thread/name values. Existing
origin restrictions, redirect policy, request deadline and credential/factory
binding remain in force; Cloud remains an explicit choice.

Run `rejected_rename_preserves_loaded_cloud_thread_name` and
`rename_sends_the_thread_and_title_and_requires_acknowledgment`. Retain the actual
source-custody controls
`cloud_idle_recovery_reads_and_attaches_only_under_its_original_source` and
`retained_cloud_transport_owns_its_actual_reader_and_destination_policy`.
Proposed direct coverage:
`cloud_rename_after_source_retirement_sends_nothing_and_preserves_loaded_name`.
The new RPC must not send through a retired source or mutate the cache on failure.

## `2bb2e20d6aa8` — adopt reconstructed fork metadata repair

ThreadStarted and thread/read reconstruction now install the target's fork
parent. `set_fork_parent` clears the cached parent title when that parent changes;
missing/invalid parent data clears inherited lineage. This repairs display state,
not runtime parent admission. Preserve ALab's surrounding startup-stack,
retry-status and queued-input hooks.

Run the renamed
`reconstructed_session_uses_target_lineage_and_active_permissions`, preserving
its active-permission checks. A focused setter check for the same-parent cached
title and invalid-parent clearing would complement it.

## `d4242793f70c` — adopt request-time Guardian limits

Original root/conversation evidence and typed verified user answers survive
collection. Fixed or elastic limits are applied when preparing the request for
both sync and async profiles. Fixed root preparation is cached; elastic
preparation can use the original source. New image details normalize before
deduplication, and committed request prefixes/IDs remain intact. Assistant text
is context, not an authorization grant. The explicit sync elastic configuration
gate remains: this does not turn elastic budgeting on by default.

Run `preparation_preserves_committed_prefix_and_selects_only_new_entries_and_images`,
`instructions_preserve_source_order_and_whole_records`,
`ordinary_exchanges_keep_roles_and_drop_assistant_context_before_restrictions`,
and `legacy_verified_answers_keep_distinct_source_order`. The existing
`guardian_root_handoff.rs::handoff_scenario` callers must prove original large
evidence survives while fixed request messages remain bounded.

For ALab restoration, run
`exec_review_tree_recovers_original_delegate_after_all_admissions`,
`exec_review_tree_refusal_stops_before_any_destination_request`,
`exec_review_tree_delivers_a_result_completed_while_frozen`,
`ephemeral_exec_review_tree_preserves_typed_result_without_storage`, and
`ephemeral_exec_review_tree_refusal_stops_before_sampling`. No ALab-owned caller
of the changed Guardian snapshot/context input APIs was found. This is not a
claim that stored review evidence grants destination authority.

## `d0221bc94711` — adapt unfinished plan identity for transfer

Keep upstream's UUID allocation and completed-ID retirement. Besides the ALab
adaptation above, run `plan_attempts_only_reuse_unfinished_item_ids`,
`plan_mode_uses_proposed_plan_block_for_plan_item`, and
`plan_mode_keeps_only_unfinished_ids_across_model_requests`. The terminal
steering call-site updates must preserve the existing policy/refusal, overload,
retry-budget and actual-interrupt checks. This affects the `exec-handoff` and
`provider-recovery` contracts and must be resolved before qualification.

## `4490060db60a` — superseded by whole-path removals

All 26 changed paths are covered by `alab/removed-paths`: 24 under `.github/`,
`.bazelrc`, and `scripts/check-module-bazel-lock.sh`. The reviewed upstream change
adds protected GCS/OIDC cache setup, conditional Bazel release selection and a
new wrapper; none reactivates these removed paths. Preserve their whole-path
absence during the merge. ALab keeps its source-bound native Cargo/atpkg lane.

Verification is removed-path enforcement, not running the removed wrapper tests
or claiming GitHub/Bazel/GCS/Windows CI results. Reconsider only with a separate
explicit build-lane, credentials and release-policy decision.

## `f242b49c8add` — adopt redundant permission-helper removal

The deleted method belongs to `PermissionProfile`; the distinct
`SandboxEnforcement` legacy conversion remains. Supported cwd/template
conversion retains symbolic workspace roots. Tests keep profile round trips
and protected workspace metadata behavior. ALab restoration uses current
permission snapshots, with no caller of the deleted helpers found.

Run `permission_profile_presets_match_legacy_defaults`,
`permission_profile_round_trip_preserves_disabled_sandbox`,
`workspace_write_with_extra_root_protects_metadata`, and the legacy-home-relative
denial/materialization cases in `permissions/target_tests.rs`, plus ALab's exec
configuration/recovery permission round trips. This deletion does not authorize
weaker destination permissions or a new sandbox default.
