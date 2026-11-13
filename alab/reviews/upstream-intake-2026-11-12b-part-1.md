# Upstream intake 2026-11-12b, part 1

Reviewed the first seven actual diffs, `672e99fed8da` through `c37f817556df`,
against ALab base `71f328417b3165cb4aede018d056ff9f17d9b352` and integrated
frontier `ce3031018ef130e653ed4d9c5bb971652de97161`. The full batch target is
`1f175fa3c4af6a3f2e8d51971c3734b4a0aaca6b`. Per-commit entries are retained in
the external `review-part1.json`; their original SHA, subject and paths are unchanged.

This is source review only. No builds, tests, provider requests, account changes,
merge or activation were performed. Named tests below are required verification,
not results. SourceV1 remains held. Voice and Cloud remain explicit user choices;
this review authorizes no new startup activity or activation default.

## Integration requirements

No definite ALab-only struct-literal or removed-method adaptation was found in
these seven commits. The new Cloud request field is filled by the upstream
constructor changes, and searches found no ALab-owned callers of that constructor,
the removed `review_isolated`, renamed sidebar geometry, or moved realtime prompt.
Preserve the existing ALab hooks when resolving the touched App/ChatWidget,
core/lib, codex-api/lib and manifest seams. Keep upstream's lock resolution rules;
do not hand-edit its lock entries.

Two behavioral boundaries need targeted evidence after integration: the new
TUI-owned voice HTTP client must retain source/destination admission through
negotiation and cancellation; Cloud pending speed/model choices must remain
conversation/backend scoped and must not change local defaults or revive stale
account authority. These are verification obligations, not assertions that the
current source already violates them. Any adaptation should follow a reproducer.

## `672e99fed8da` — adopt bounded proxy-503 admission recovery

The marker is attached only to `OpenSession`. Its transport strips response data
on HTTP 503 while retaining headers/trailers, allowing tonic to honor an explicit
gRPC status or infer `Unavailable`. Other RPC bodies and the existing five-attempt
bound remain unchanged. The transport still uses the existing route-aware client.

Run `session_admission_retries_only_transient_failures_and_is_bounded`, including
plain-text exhaustion, HTML recovery, and explicit `Internal` preventing retry.
No additional ALab retry loop or broader retry classification should be added.

## `d2f76324729d` — adopt scoped DNSSEC advisory exceptions

The same five advisory IDs are added to audit and deny configuration, with an
explicit DNSSEC-disabled rationale and removal trigger at Hickory >=0.26.2.
This changes advisory policy, not dependencies or runtime DNS settings. The
network-proxy manifests pin Rama; the claim that its resolved graph has no DNSSEC
must be rechecked against the integrated graph before treating the exemptions as
qualified. Do not interpret the exception as permission to enable DNSSEC.

Proposed check: `hickory_dnssec_features_remain_disabled`, inspecting the actual
resolved all-workspace feature graph and matching exception sets. Revisit before
any DNSSEC enablement or Rust/Rama/Hickory upgrade; remove obsolete exceptions.

## `e61a08f9ecb9` — adopt per-parent Guardian review history

Direct review now enters the ordinary reusable reviewer pool; actor and reviewer
histories remain separate. The pool keys its reusable session to context and
retains its bounded recovery path. `recorded_tool_call::review_tool_call` still
wraps the entire operation in cancellation and deadline selection. A stored
review result must remain evidence, never an inherited authorization to execute.

Run `guardian_review_reuses_private_recorded_call_history_per_parent`,
`isolated_review_returns_assessment_for_prepared_action`,
`websocket_disconnect_cancels_guardian_review_and_releases_thread`, and
`wait_for_guardian_review_cancel_drains_expected_turn_after_stale_terminal_event`.
The expanded direct-review scenario must continue proving recorded commands are
not executed. Proposed ALab check:
`direct_guardian_history_does_not_cross_account_or_restore_authority`.

## `ae50f973f516` — adopt panel-oriented sidebar refactor

Geometry and focus helpers are renamed from chooser to panel, and focus changes
reuse visible content instead of forcing the chooser. Chooser-specific checks
remain. No ALab glue directly uses the removed geometry names. Preserve native
session observation, pet cleanup, account identity and parked-backend hooks in
the surrounding App code; this refactor does not supersede those contracts.

Run `sidebar_chooser_resize_survives_startup_thread_assignment`,
`sidebar_chooser_preserves_the_parent_draft_and_starts_a_side_chat`, and
`parked_thread_routing_cannot_replace_the_selected_account_quota_or_workspace`.
Proposed check: `panel_focus_preserves_stashed_draft_and_native_account_row`.

## `72532ecae5ef` — keep voice negotiation explicitly opted in

The TUI now creates the media call with local provider credentials, then attaches
the backend using `ExistingCall`. The new provider helper uses the existing
`AuthManager::shared_from_config`; request preparation observes auth generations,
and `RealtimeCallClient` still reaches ALab's `EndpointSession` admission hook.
The detached negotiation is abortable, and results are accepted only for the
matching thread/startup attempt. SDP remains redacted in command diagnostics.
The prompt/header extraction preserves existing precedence and thread-source
bounds. Do not add ambient credential fallbacks or enable SourceV1.

Run `voice_cancel_restart_only_attaches_the_new_attempt`,
`remote_voice_start_routes_v3_offer_and_effective_preference`,
`shutdown_stops_backend_voice_once_through_app_server`,
`background_voice_closes_with_its_owner_not_the_visible_thread`, and
`realtime_offer_is_redacted_in_diagnostics_and_preserved_for_signaling`.
Also run `source_change_after_destination_admission_refuses_before_new_credential_materialization`.
Proposed checks: `tui_voice_negotiation_rejects_retired_local_source` and
`unrequested_voice_startup_sends_no_call`. Revisit activation only after the
new route's admission, cancellation and redaction evidence passes.

## `b279cfb170df` — keep Cloud speed selection conversation scoped

Pending model/effort/tier settings move into a per-backend map keyed by thread.
User admission consumes them; stale catalogs refuse selection. The wire retains
omitted/inherit versus explicit-null/clear, converts `default` to clear, and
rejects tier overrides on queued messages. This must not overwrite local config,
the packaged Ultra reasoning default, or another conversation's selection.
ALab Fast/Ultrafast labels remain distinct from reasoning effort.

Run `cloud_ultrafast_toggle_updates_the_conversation_on_admission`,
`cloud_speed_tracks_the_model_staged_for_the_next_turn`,
`cloud_model_selection_waits_for_native_admission`,
`user_input_uses_stable_id_and_the_existing_cloud_configuration`,
`submission_preserves_admission_variants_and_wire_identity`, and
`invalid_input_never_sends_and_diagnostics_redact`.
Retain `alab_ultra_default_survives_same_model_selection`,
`supported_speed_tiers_reach_model_status_title_and_header_surfaces`, and
`cloud_idle_recovery_reads_and_attaches_only_under_its_original_source`.
Proposed check: `cloud_pending_speed_does_not_cross_account_or_conversation`.
Revisit after source-bound Cloud admission and pending-choice tests; no default
Cloud connection or service-tier change is authorized.

## `c37f817556df` — adopt owned trusted-Git cleanup

Trusted temporary Git repositories now use `MarketplaceStagingDir` under the
canonical Codex home's `.tmp/.staging`. Lifetime locks protect live owners;
creation invokes existing bounded cleanup and rejects a symlinked staging root.
The automatic/manual Git environment distinction is unchanged. Plugin defaults
remain disabled, and this is not permission to prune ordinary worktrees or caches.

Run `cleanup_preserves_live_process_and_reclaims_its_abandoned_checkout` with its
new trusted-Git path and concurrent-owner assertions. Proposed check:
`trusted_git_repository_rejects_symlinked_staging_root_without_touching_target`.
