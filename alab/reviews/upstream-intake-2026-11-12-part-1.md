# Upstream intake, part 1

Reviewed 13 actual commit diffs, `e58bf8f5233b` through `14e039906c92`, in the
linear range whose ALab base is `7c144752ea20afde9ded3cf97f6d800c89aa95d8`
and upstream target is `ce3031018ef130e653ed4d9c5bb971652de97161`.
Read the current `alab/features.toml`, packaged defaults, relevant ALab hooks,
and existing tests. Full per-commit decisions and evidence are in
`reviewed-1.json`; immutable commit SHA, subject and changed paths match
`part-1.json`.

This is semantic source review. No merge, repository edit, build, test,
credential use, provider request or live activation was performed. Test names
below are validation requirements, not pass claims. SourceV1 remains held.

## Required adaptations

1. **`9e6d1c5278b6`, retained delivery metadata.** Add the new
   `retained_source` field to the explicit `DeliveredAssistantMessage` in
   `alab-core/glue/exec_memory_records.rs::retained_delivery_remains_a_host_fact`.
   Strengthen that existing test with a real non-None retained source containing
   allocation metadata, so the private handoff codec proves the new revision is
   preserved. Also seed a non-None `next_item_ordinal` in the existing
   `compaction_keeps_nested_tool_outcomes_and_review_provenance` test.
   `MemoryCompacted` already clones/serializes the complete resume metadata, so
   no new production codec field is necessary. The inherited-message restriction
   and destination-owned ID generator remain necessary. The separate
   `control_tests.rs` expression uses `..parent_resume_metadata`; its explicit
   parent constructor is changed upstream, so it needs no extra ALab field edit.
2. **`bc3ccba6e1cb`, immediate HTTP fallback.** Reconcile the changed
   `core/src/responses_retry.rs` condition and return with ALab's `wait_after`
   and `fallback_notice` calls. Retain exact provider advice before the retry,
   typed fallback status, and existing permanent-refusal/capacity ordering.
   Prewarm and mailbox draining must use the new typed fallback without losing
   source-bound client/auth ownership. Add a focused test, proposed name
   `immediate_size_fallback_preserves_provider_deadline`, using
   `WebsocketMessageTooLarge`, a positive `RetryAfter`, and a retry budget larger
   than zero. Assert immediate transport switch, no early retry before the
   deadline, and the typed status event. Existing exhausted-budget coverage
   alone does not exercise the new early branch.

Neither adaptation authorizes native activation or removal of an ALab
ownership adapter.

## Per-commit assessment and validation

### `e58bf8f5233b` — adopt: shared Guardian reviewer selection

`select_synchronous_reviewer` returns the existing thread's reviewer plus an
unpolled configuration future. Both callers still await configuration inside
their own deadline/cancellation scope. No new reviewer or model is selected.
The exec-handoff contract still requires retained review delegates and actual
completion/drain; this refactor is not equivalent to that contract.

Exercise existing `isolated_review_returns_assessment_for_prepared_action`,
`guardian_review_uses_model_catalog_override_when_preferred_review_model_exists`,
`cancelled_guardian_review_emits_terminal_abort_without_warning`, and
`wait_for_guardian_review_timeout_drains_expected_turn_after_stale_terminal_event`.

### `9e6d1c5278b6` — adapt: retained revision continuity

`record_and_capture` fills the delivered source before persistence under the
existing checkpoint lock. Rollout metadata now keeps that source through an
older reader's rewrite. The new ordinal scanner counts local item/revision
provenance and skips quoted Guardian source/review metadata. The optional
checkpoint ordinal is data; it grants no authority and does not replace ALab's
fresh destination generator/admission.

Exercise `delivered_assistant_rollout_survives_an_earlier_read_and_rewrite`,
`model_id_ordinals_resume_at_the_checkpoint_and_advance_for_local_items`,
`inter_agent_communication_waits_for_confirmed_delivery_persistence`, and
ALab's strengthened `retained_delivery_remains_a_host_fact` and
`compaction_keeps_nested_tool_outcomes_and_review_provenance`. Also run
`memory_context_retains_item_allocations_across_actual_install` and
`restricted_checkpoint_keeps_local_evidence_and_existing_uncertainty` to check
that preserved revision data cannot revive inherited authorization.

### `bc3ccba6e1cb` — adapt: early size failures switch to HTTP

Only a close-code 1009 before *any text frame* receives the new typed error.
Metadata, rate-limit or response-created frames already count as text, so those
paths retain ordinary stream retry behavior. Prewarm recognizes the same error;
mailbox-draining code treats it as transport completion while preserving its
pending input. Keep ALab provider pacing/status hooks when merging the new
`switched || requires_http_fallback` branch.

Exercise `websocket_fallback::close_code::retry_transport` cases
`size_rejection_before_any_text`, `size_rejection_during_prewarm`,
`size_close_after_model_metadata`, `size_close_after_rate_limits`,
`size_close_after_response_created`, and `ordinary_close`. Run ALab's
`protecting_an_exhausted_verdict_still_allows_real_fallback_and_its_deadline`,
`completed_errors_propagate_while_cancelled_waits_and_success_keep_follow_up`,
and `ordinary_retry_recovers_sleep_during_notification_after_a_rejected_receipt_sample`,
plus the proposed new early-fallback/deadline test above. The new enum falls
through ALab's nonexhaustive classifications without forcing a new retry wall.

### `ed7ca22bb60a` — adopt: advertised WebSocket byte limits

The limit belongs to the actual `WsStream` and is compared to serialized UTF-8
byte length before upload. Equality is allowed; missing or invalid values leave
prior behavior. Reuse the preceding commit's typed fallback and preserved
pacing rather than inventing a second recovery path.

Exercise `retry_transport::advertised_limit_skips_websocket_upload` (zero
WebSocket generations, one HTTP post) together with the six close-code controls.
Additional parser-boundary coverage for malformed/missing/equal-byte limits
would be useful but is not an identified ALab merge adaptation.

### `54b90f98a572` — adopt: bounded provisional/fallback titles

The current helper call already exists. This change gives it a 60-character,
Markdown-stripped provisional title, persists a fallback on invalid/failed
response, keeps manual name precedence and distinguishes hidden title/recap
threads. Title start and turn parameters explicitly request read-only/empty
roots; `never` remains subject to managed policy. Blocking input stays on the
hidden title until timeout. A timeout interrupts a known turn ID before detach;
this is not proof that a timed-out `turn/start` without an ID has drained.
ALab `auto_recap=false` and its startup/account ownership hooks stay in force.
No ALab-owned caller uses the renamed temporary/title interfaces.

Exercise `automatic_thread_title_replaces_the_provisional_name`,
`automatic_thread_title_saves_the_first_message_if_generation_fails`,
`manual_rename_cancels_running_thread_title`,
`overview_rename_cancels_running_thread_title`,
`canceled_thread_title_ignores_late_start_and_completion`,
`cloud_conversations_skip_tui_generated_titles`,
`managed_workspace_default_respects_read_only_availability`,
`preserves_custom_permissions_and_disables_required_mcp_servers`,
`temporary_recap_threads_disable_memories_and_remote_mcp_servers`, and the Unix
PTY test `automatic_title_after_continue_anyway`.

### `ec51be0772c9` — adopt: deduplicate overlapping file search roots

Repeated roots are removed from the walker while nested walks remain, retaining
root-specific ignore visibility. `get_file_path` selects the deepest matching
root; duplicate path insertion into its corresponding nested-root set is
suppressed before matcher accounting. There is no affected ALab adapter.

Exercise `overlapping_roots_share_result_slots_by_path` and
`matching_filenames_survive_the_path_result_limit`. The change handles lexical
paths and does not promise symlink-alias equivalence.

### `367c67560251` — adopt: whitespace-containing search snippets

Match the original text, choose Unicode-safe context there, then normalize only
the display snippet. Ellipsis checks use the same original string boundaries.
No stored transcript, ALab owner state or search permission changes.

Exercise `app-server::v2::thread_list::thread_search_matches_json_escaped_content`,
which now covers repeated spaces, newline, tab, quotes and backslash.

### `b117b2d6fe44` — adopt: explicit acceptance of GCP incremental default

This review accepts the new stable GCP-only default. It changes the encoding of
existing requests for a provider the user already selected; it does not select
GCP, start background requests, or enable a tool. Bedrock's flag remains false
by default and independent, responses-lite keeps its separate flag, and an
explicit GCP opt-out wins. That bounded benefit supports adoption without an
extra ALab default override. Ultra and all existing privacy/background defaults
remain unchanged. SourceV1 is unrelated and remains held.

Exercise `incremental_tools_append_changed_catalog_without_rewriting_history`
with `gcp_default`, `gcp_opt_out`, `mantle_default`, `mantle_enabled`,
`runtime_default`, `runtime_enabled`, and `responses_lite`;
`vertex_requests_use_command_auth_with_namespaces_and_tool_search` with both
incremental choices; and
`provider_automatic_compaction_uses_v2_responses_endpoint`.

### `392ced7d3a91` — adopt: stale MCP OAuth writes fail closed

The persistor obtains the refresh lock, rereads its pinned store, and refuses
replacement or deletion before saving changed manager credentials. The file
save performs another expected-credential comparison under its aggregate lock
and updates the matched row in place, keeping legacy name/key/ownership.
Comparison intentionally strips fields the file cannot persist. Legacy cleanup
matches only eligible ordinary identities; enterprise-scoped cleanup preserves
the corresponding local entry. This complements, rather than replaces, ALab's
ChatGPT refresh and native source/transport locks. Added load telemetry does not
select a new telemetry sink; ALab analytics remains disabled.

Exercise `legacy_persistence_preserves_new_login_and_logout`,
`legacy_file_refreshes_preserve_the_matched_credential`,
`mutations_require_and_retain_the_transaction_guard`,
`file_store_load_and_delete_observe_aggregate_lock`,
`legacy_refresh_persistence_records_each_write_once`, and
`enterprise_file_cleanup_preserves_legacy_local_credentials`.

### `3a45bbba3cae` — adopt: in-process ready acknowledgment

`start` now waits until the serial processor has handled `initialized`. Send or
ack failure shuts down the started runtime, while ordinary notifications stay
fire-and-forget. Preserve the separate ALab `AlabExecSuspend` enum/forward/process
arms and metadata-only test configuration. Startup readiness is not End,
persistence closure or native capability publication.

Exercise `in_process_start_accepts_immediate_request` with
`zero_capacity_is_clamped` and `one_slot_capacity`, plus
`exec_suspension_keeps_original_turn_and_closes_admission` and
`ephemeral_exec_suspension_retains_history_without_a_rollout`.

### `f096185f3a23` — adopt with explicit unresolved advisory debt

The atomic write helper keeps a cleanup-owned temporary path until `rename`
succeeds. Windows test children publish markers into the writable workspace,
clear inherited capture handles, and use async waits for the current-thread
ConPTY runtime. These are actual lifecycle checks, not relaxed assertions.
Reapply `.bazel`/`.bzl`/`patches` removal, so upstream zlib snapshot changes are
reviewed but not activated in ALab's Cargo/Trust build.

The four added `hickory-proto` ignores (`RUSTSEC-2026-0487`, `0489`, `0490`,
`0491`) do not fix code. The dependency remains 0.25.2 via Rama 0.3.0-alpha.4;
the target's stock Rust pin is 1.95.0. Record the upstream removal trigger
(Rust/Rama update supplying hickory-proto >=0.26.2); this review has not assessed
the advisory bodies or claimed exploitability is absent. ALab's gate does not
become an advisory proof because the upstream audit files suppress reports.

Exercise `blocking_set_model_writes_through_symlink_chain` for the atomic-write
caller. On Windows exercise
`legacy_capture_emits_output_and_preserves_descendant_after_normal_exit`,
`legacy_capture_cancellation_terminates_descendants_without_timeout`, and
`legacy_tty_job_terminates_and_preserves_descendants`. Mac validation cannot
prove Windows overlapping-reader replacement or sandbox lifecycle semantics.

### `d27e7c1a8d1b` — adopt: stable cancellation-test tracing

A local `NoSubscriber` dispatch keeps concurrent callsite registration from
invalidating the test capture. Production abort attribution and tracing policy
are untouched. Run
`tools::parallel::tests::cancellation_before_dispatch_admission_logs_dispatch_only_timing`
with its parameterized cases, including as part of parallel core tests.

### `14e039906c92` — superseded by existing `.github` removal

The source was read: the new Python labeler uses a tools-disabled strict-schema
Responses request, validates known labels, rejects redirects and handles bounded
retries; workflow application errors are no longer swallowed. It is entirely
under `.github`, which ALab removes. Keep it removed rather than activating
model/GitHub automation or supplying credentials. Verify the composed candidate
has no tracked `.github` paths and passes the existing removed-path check. No
runtime execution of removed code is required or authorized.
