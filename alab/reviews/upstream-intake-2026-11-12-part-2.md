# Upstream intake semantic review, part 2

Reviewed on 2026-11-12 against ALab base `7c144752ea20afde9ded3cf97f6d800c89aa95d8`, target `ce3031018ef130e653ed4d9c5bb971652de97161`.

This ordered subset contains 13 upstream commits (`9248df5978` through `f988ada4bf`). Its JSON preserves the supplied SHA, subject and ordered path lists. Actual diffs are retained in `part2-diffs/`; the assessment also reads current ALab hooks and `alab/features.toml`. Generated protocol changes were checked against their producing types/generator and consumer tests; no binary export was executed. This is semantic intake evidence only: no merge, Rust build, runtime tests, account reads or network calls were performed. Planned checks are not passing-test claims. The parent must assemble and validate the complete 40-commit record.

## Required adaptation and activation decisions

- Adapt `f988ada4bf` before shipping Cloud idle recovery. Its `Login` retains a `NetworkPermit` for the whole worker lifetime. In ALab that permit owns counted catalog/native resources (`http_native_source::Binding::acquire` -> `OriginUse`/`NetworkUse`), even while no recovery runs. Retain frozen non-counting source/policy observation in the idle owner and acquire activity only for each bounded recovery episode. Preserve sticky revoke-then-allow rejection and never recapture a newer account into the old worker.
- Add explicit `ThreadStreamReconnected` handling to ALab `backend_bookkeeping.rs` and both native upgrade dispatch helpers. A reconnect is neither a new turn nor a completed turn. Adapt the ALab `handle_polled_backend_event` test helper to its new backend-owned `parked_inputs` argument. Preserve the parked lifecycle hook before routing returns and the per-channel overflow FIFO when resolving `thread_events.rs`.
- Keep `multi_environment_mcp` disabled by default. Exact keys improve isolation, but additional exposed installations and qualified names remain opt-in. Preserve assignment router replacement, selected-owner EMA inspection and native source retirement.
- Keep `transcript_selected_exec_results` explicitly configured; an omitted section reserves zero tokens. No ALab default reservation is proposed.
- SourceV1 activation remains held. None of these commits proves the joined native source seal, consumer and successor-process contract. No unverified equivalence or hook-removal decision is made.

## Minimal Cloud source-custody seam to implement after integration

Keep the upstream recovery state machine, three-attempt limit and eight-second episode budget. Use an ALab-owned adapter for the `Login.permit` role. It retains the exact frozen `NetworkPolicy` and an original sticky application-revocation observation without retaining native/catalog activity. Its `run` acquires a source-bound permit from that frozen policy only for the episode, joins both revocation conditions, and releases on completion/cancellation. Its final `check` must reject retired authority. Choose the concrete thin HTTP/Cloud hook after integration; do not clear native binding or replace source identity with token/account equality.

Required deterministic tests: retaining an idle owner leaves activity zero; pending auth read/attach owns the original source; cancellation releases it; source A-to-B and revoke-then-allow cannot revive A; identical token/account text cannot substitute for original source identity. Retain the existing `auth.alab_bind_http_client_factory` hook before raw-token extraction and the `AuthConfig::load_auth_with_modes` direct-reader admission hook. This is a proposed adaptation, not verified behavior.

## Per-commit review

### `9248df5978` — Include bundled bubblewrap in the root metadata test fixture (#60898)

Decision: **adopt**. Classes: fix, packaging. Features: qualification-fixtures, build-and-adoption.

Repair only the Linux root-metadata security fixture: stage the bundled bubblewrap beside the copied sandbox executable, requiring the bundle for a verified build and allowing system bwrap only for an unverified build. This makes the existing denial assertion exercise the intended sandbox rather than fail at resource discovery; it does not relax the production sandbox or ALab Trust admission.

- Read actual diff: linux-sandbox/tests/suite/root_metadata_tests.rs discovers find_bundled_bwrap_for_exe, checks CODEX_BWRAP_SHA256, and copies into codex-resources.
- Relevant contract: features.toml qualification-fixtures requires truthful upstream test baselines; build-and-adoption retains Trust/compiler and packaged-artifact identity.
- Planned Linux check, not executed in this review: root_metadata_symlinks_cannot_reopen_approved_denials with bundled verified and unverified fixture paths.

Revisit: If verified Linux packaging changes bubblewrap placement or hash semantics, rerun the denial fixture against that exact package.

### `8a4b226ad5` — Version answer display instructions and restrict unsupported payloads (#60904)

Decision: **adopt**. Classes: protocol, security, fix. Features: native-session.

Adopt the version-1 typed recovered-answer display contract and fail-restricted handling of unknown version/mode: malformed display is discarded while moderated becomes true. Rust targeted moderation, history conversion and Python generated-model validation take the same path. This is presentation metadata, not authorization or native session evidence; no ALab behavior is superseded.

- Read actual Rust display.rs, AgentMessageModeration/TargetedAnswerModeration deserializers, thread_history.rs and v2/item.rs diff; unsupported non-null display cannot preserve moderated=false.
- Reviewed generated schema/TypeScript contract changes and Python artifact-generator fallback plus its history/notification regression; source uses typed show/hide/block and literal version 1.
- No ALab AgentMessageDisplay field consumers found; the affected protocol.rs ALab review-source hooks are separate from moderation.
- Planned checks: answer_moderation_preserves_the_display_instruction; unsupported_answer_display_leaves_the_answer_restricted; core model_request moderation regression; sdk/python/tests/test_contract_generation.py::test_answer_display_is_read_in_thread_history_and_notifications. No tests run.

Revisit: Re-review when a new display version is supported; never treat unknown presentation metadata as native permission.

### `561f0f5cf0` — Keep same-named MCP servers separate across environments (#60905)

Decision: **keep-opt-in**. Classes: feature, protocol, security, refactor. Features: account-isolation, exec-handoff, product-policy.

Carry exact McpServerKey registration identity through connection/binding/resource/call/approval ownership, retaining host-owned Apps and enterprise policy boundaries. Keep multi_environment_mcp disabled by default as upstream declares. Environment-qualified namespaces/resource selectors and multiple plugin installations change model-visible tool exposure and approval scope; local source retirement/router hooks remain necessary and this is not SourceV1 qualification.

- Read actual catalog/server_key/binding/connection/runtime/tool metadata, core resource resolution, approval and hook-selector, and extension discovery diffs. Legacy exposure groups by bare name without rewriting the winning exact key; qualified exposure uses environment-scoped keys.
- features/src/lib.rs adds MultiEnvironmentMcp at UnderDevelopment with default_enabled=false; ext/mcp discovery exposes additional installation servers only when enabled.
- core/tools/handlers/mcp_resource.rs requires environment_id for environment-owned servers when multiple environments are selected; tools/registry.rs preserves that selector across hooks; mcp_tool_call.rs keeps scoped session approvals separate from bare-name persisted policy.
- Compared current ALab glue/assignment_mcp.rs and runtime hooks: replaceable RouterOwner, shutdown, enterprise-state refusal and invalidated caches must survive merge; mcp_processor.rs retains selected-owner EMA status lookup.
- Planned checks: scenarios_multi_environment_mcp::same_named_executor_mcps_keep_calls_and_approvals_separate; hooks_executor::mcp_hooks_preserve_ownership_with_same_named_servers; persistent_mcp_tool_approval_remembers_scoped_session_key; explicit_environment_is_validated_before_waiting_for_ready_connections; assignment_retirement_replaces_closed_router_without_inheriting_form_permission; mcp_status_owner_tests. No tests run.

Revisit: Enable only after the exact ALab build passes scoped tool/resource/approval and account-retirement coverage; retain SourceV1 activation hold independently.

### `c95b72ed6f` — Reserve Guardian context budget for selected Code Mode exec results (#60914)

Decision: **keep-opt-in**. Classes: feature, model. Features: exec-handoff, product-policy.

Accept the Guardian selected-exec-result mechanism but leave the new reservation inactive unless explicitly configured. Selection requires the host-recorded completed wrapper inventory, not output text or a model-supplied tool label; a missing section reserves zero tokens. Keep ALab native handoff/review evidence preservation, which this budgeting change does not replace.

- Read actual config parsing, ExecutedToolCalls::exec_output_matches, Guardian collector/review_session_prompt, transcript tagging and elastic allocation/enforcement diffs. Fixed budgeting retains the ordinary tool cap.
- GuardianElasticSectionConfigToml accepts nested_tool_names only for transcript_selected_exec_results; absent section defaults to zero and no selection predicate is installed.
- Reviewed current ALab exec_model_context.rs exhaustive state capture and destination-generator restore; new selected_exec_tool_names is configuration data, not a replacement for captured review history or fresh authorization revision.
- Planned checks: selected_exec_result_uses_only_the_retained_wrapper_inventory; missing_exec_call_preserves_recorder_verified_output; elastic_section_config_validates_entry_limits; guardian_elastic_budgeting scenario; memory_context_retains_accounting_and_renews_process_evidence. No tests run.

Revisit: Any ALab default reservation or new nested tool selection needs a separate model-budget decision and handoff validation.

### `27294165dc` — Preserve Command Center task ownership across backend switches (#60929)

Decision: **adopt**. Classes: fix, refactor. Features: native-ui, account-isolation, exec-handoff, qualification-fixtures.

Adopt first-observed backend ownership for retained Command Center tasks, backend-scoped discovery/pins/usage and live-blank validation before draft reuse. Server-current collaboration settings win over stale retained settings. Preserve ALab parked-backend turn/goal bookkeeping and frame attribution separately: upstream UI ownership does not prove native turn completion or supersede the collision-safe seen book.

- Read actual BackendRegistry, backend selection/events, overview discovery/actions/usage, resume_preparation and input_restore/reconnect diffs. Conflicting owner registration refuses; parked removals retain displayed pagination budget.
- live_blank_session requires Idle and empty paginated/history evidence with explicit older-server fallback; populated tasks resume instead and retained drafts receive current server model/reasoning mode.
- Compared ALab backend_bookkeeping.rs, session_observe.rs, tasks.rs and upstream hooks in app_server_events/backend_events/backend_selection. ALab Tasks navigates through SelectAgentThread; all turn/goal bookkeeping hooks must retain authenticated backend IDs.
- Planned checks: overview_keeps_rows_and_routes_actions_to_the_owner; refreshing_one_backends_pins_preserves_cross_backend_rank; background_archive_and_delete_remove_only_owned_rows; backend_handoff_reads_saved_turns_instead_of_reusing_a_stale_blank; backend_handoff_keeps_resumed_settings_for_populated_task_draft; reused_native_thread_and_turn_ids_keep_the_other_backends_seen_owner. No tests run.

Revisit: Revisit if upstream ownership policy or backend-ID lifecycle changes; do not replace native bookkeeping based only on these UI tests.

### `c93732887e` — Preserve legacy sandbox templates in TUI permission conversions (#60936)

Decision: **adopt**. Classes: fix, security, protocol. Features: exec-handoff, product-policy.

Adopt lossless portable legacy sandbox-template conversion for TUI-created child threads. Template decoding avoids host .git probing and preserves duplicate writable roots, network flags and temporary-directory exclusions; conversion back requires exact PermissionProfile round-trip and rejects restrictions the legacy form cannot represent. Runtime enforcement continues to bind workspace metadata separately.

- Read actual PermissionProfile and FileSystemSandboxPolicy projection diff, dynamic_tools create_thread/start_turn and temporary_structured_request checks. Template mode does not collapse fixed roots into cwd or deduplicate them.
- create_thread retains named profiles when present and otherwise forwards the exact converted template; external sandboxes without a named profile remain rejected. Temporary structured requests still require Managed permissions with no writable entries.
- Planned checks: sandbox_policy_round_trips_workspace_write_access, task_creation_and_followup_start_background_turns, and app_server_session legacy permission-override rejection tests. No tests run; no ALab policy/default permission change proposed.

Revisit: If runtime filesystem policies cease to round-trip through legacy templates, refuse or add an explicit typed transport; do not broaden permissions.

### `77cbcdb4a6` — Preserve thread-local model ID state across replay and compaction (#60942)

Decision: **adopt**. Classes: fix, model. Features: exec-handoff, qualification-fixtures.

Adopt per-thread item/window generators and append-order durable ordinal accounting. Reconstruct retained history with a separate generator; resume from the validated history window without a provisional allocation; recover cursor maxima from checkpoint replacement, Guardian and retained source revisions. Preserve ALab handoff installation into the destination session generator and its independent runtime-authority renewal.

- Read actual model_ids.rs, Session initialization/reconstruction, LocalAgentRuntime generator removal, history/model_id_ordinals and retained_context changes, in-memory/local writer cursor changes and rollout migration lock type.
- Local writer updates its u64 cursor after recorder accepts a batch and before flush, preserving queued ownership on cancellation; checkpoint metadata stamps only allocations through its append position, not future items.
- Compared ALab exec_model_context.rs::PreparedExecContext::install and exec_ephemeral_history.rs::restore_context_if_pending: installation already receives session.item_id_generator, so retain that fresh-thread dependency; no ALab runtime-generator field reference found.
- Planned checks: model_id_ordinals_resume_at_the_checkpoint_and_advance_for_local_items; cold_root_resume_overlaps_child_reads_and_applies_identities_in_graph_order; admitted_prompt_id_reaches_responses; memory_context_retains_item_allocations_across_actual_install; memory_context_retains_accounting_and_renews_process_evidence. No tests run.

Revisit: Rerun durable resume/fork and native nonpersistent handoff after composition; model-ID repair is not a substitute for End ACK or fresh destination authority.

### `5a529c2909` — Preserve file permissions during repeated Windows sandbox setup (#60943)

Decision: **adopt**. Classes: fix, security. Features: product-policy, qualification-fixtures.

Adopt Windows repeated-setup DACL verification: existing persistent directories use a no-reparse READ_CONTROL handle to check the expected explicit ACE prefix and inherited-only suffix, avoiding unnecessary inheritance propagation that could alter moved files. Any mismatch or verification failure reapplies the expected policy; protected executable-directory policy remains enforced.

- Read actual acl.rs handle-based DACL extraction, setup_provisioning DirectoryAcl/metadata_policy_matches and Owned<LocalAllocHandle> lifetime changes. VerifyInherited is limited to registered ProvisionOnly persistent directories.
- Read repeated_registered_setup_keeps_existing_file_permissions: moved preexisting file remains nonwritable after second setup while a newly created file receives inherited group write. No ALab hooks in these paths.
- Planned Windows checks: repeated_registered_setup_keeps_existing_file_permissions; lock_sandbox_dir_blocks_inherited_write_for_runner_files; existing reparse/restricted-DACL tests. Native Windows execution unavailable in this read-only Mac review.

Revisit: Validate on Windows before claiming this platform path qualified; no broad ACL relaxation is authorized.

### `d7840f9f22` — Reconcile Windows runtime registration after service privilege errors (#60944)

Decision: **adopt**. Classes: fix, security. Features: build-and-adoption, product-policy.

Adopt narrowly bounded reconciliation of Windows packaged-service privilege failure. Only terminal AsyncStatus::Error plus the exact service-privilege HRESULT may succeed after the intended user sees a reparse execution alias and FindUsers reports the exact package Installed for that SID; cancellation, unrelated errors and missing registration still fail. Command image/package verification remains downstream.

- Read complete registered_runtime.rs and registration_result.rs diff and regression. The postcondition callback is reached only after successful result/extended-error decoding and only for ERROR_PACKAGED_SERVICE_REQUIRES_ADMIN_PRIVILEGES.
- Registration probes impersonate the intended user for alias metadata, query the exact full package name and match user SID plus Installed state; ordinary registered_packages verification remains after reconciliation.
- Planned Windows check: only_reconciles_a_terminal_service_privilege_error_for_the_registered_target plus retains_different_async_and_deployment_errors_with_activity and diagnostic_getter_failures_do_not_mask_completion_or_leak_text. No tests run; this is unrelated to ALab atpkg admission.

Revisit: Reassess on Windows if deployment error semantics or alias identity checks change; never generalize to cancellation/access-denied success.

### `1b7c025258` — Report biometric type in user verification responses (#60948)

Decision: **adopt**. Classes: protocol, feature. Features: security-notices, native-session.

Adopt optional biometricType context separate from the signed proof. macOS reports Fingerprint only after successful signing protected by the existing biometric-only key policy and a TouchID method observation. Missing/unknown method remains None, and older servers omitting the field deserialize successfully. The new field is not signed authority or a native handoff/source attestation.

- Read complete protocol, native provider, app-server response mapping and RPC/cancellation test diffs plus experimental export list. Credential ID/signature wire fields and cancellation ordering are unchanged.
- Native provider sets method after successful signature; API docs explicitly identify it as context rather than signed proof. No ALab proof/admission hook is removed.
- Planned checks: verification_response_accepts_older_servers_without_biometric_method; user_verification_rpc_reports_native_method_separately_from_proof; signing_uses_decoded_bytes_without_a_pending_elicitation; user_verification_cancel_remains_effective_until_proof_enqueue_and_clears_context. No tests or interactive biometric operation run.

Revisit: Any decision relying on biometricType as authenticated proof requires a separate signed protocol change and review.

### `64acaba91c` — Refactor shortcut overlay binding construction (#60962)

Decision: **adopt**. Classes: refactor. Features: native-ui.

Adopt typed shortcut-entry construction while preserving conditional entries, repeat-label behavior, paired navigation keys and clipping/footer rendering. PageUp/PageDown now use the same KeyBinding display conversion as other keys. There is no key-dispatch or ALab interface activation change and no ALab hook in the modified file.

- Read complete shortcut_overlay.rs diff: bindings produces the same three groups; lines filters missing hints and preserves Esc repetition plus SSH-compatible jump alternatives.
- Compared current ALab native-ui contract; rendering helper changes do not replace ALab row budgeting, prompt ownership or pet lifecycle behavior.
- Planned checks: shortcut_overlay_uses_runtime_bindings_and_wsl_paste; shortcut_overlay_clipping_preserves_customization_in_small_areas. No tests run.

Revisit: Recheck snapshots if platform-specific KeyBinding labels change; retain assertions rather than normalizing visible text.

### `5c3ff1dabf` — Validate the issue labeler's API key before sending requests (#60964)

Decision: **superseded**. Classes: security, fix, removed-path. Features: build-and-adoption, product-policy.

The upstream change adds fail-fast API-key validation in its issue-labeler: trim boundary whitespace and reject empty or non-visible-ASCII values before building/sending Authorization. This is CI-only credential formatting, not an ALab runtime credential-source change or a reason to disclose secrets.

- Read complete .github/scripts/issue_labeler.py diff; suggest_labels validates api_key before constructing its Request and uses the validated local value.
- The path has no ALab hook or runtime feature contract; no GitHub request or credential inspection was performed.
- Root composition review: `alab/removed-paths` removes `.github` in its entirety, including this script. Retain that policy and verify the composed candidate has no tracked `.github` path; do not execute the removed automation or provide credentials. No tests run.

Revisit: If supported authentication formats change, review validation at this CI boundary independently of account registry/runtime credentials.

### `f988ada4bf` — Reconnect idle Cloud conversations after transient stream loss (#60969)

Decision: **adapt**. Classes: fix, background, feature, protocol. Features: account-isolation, provider-recovery, native-ui, exec-handoff, product-policy.

Adopt idle Cloud recovery only with ALab source-custody and event-dispatch adaptations. The new worker retains a NetworkPermit for its whole lifetime; in ALab that permit owns counted resource/native-use activity, so an idle cached recovery object can prevent source retirement. Retain frozen non-counting authority/revocation state and acquire activity for each bounded recovery episode without recapturing a newer owner. Add ThreadStreamReconnected handling without fabricating turn completion, preserve queued input/backend ownership, and retain SourceV1 activation hold.

- Read actual idle.rs/auth.rs/worker/CloudClient and TUI/backend/input/history diffs: recovery is offered only for observed-idle transient loss, accepted only by the displayed idle UI, bounded to three attempts/8 seconds, excludes PAT, checks exact token/account before and after attach, and retains sticky policy revocation. HTTP/2 keepalive is Cloud-only (10s interval, 60s timeout).
- Concrete composition gap: new AppServerEvent::ThreadStreamReconnected is absent from exhaustive matches in ALab backend_bookkeeping.rs::State::stream and both upgrade native-case deliver helpers; handle_polled_backend_event gains parked_inputs and its ALab test adapter must supply backend-owned state.
- Concrete custody mismatch: new auth::connect stores factory.network_policy().acquire(origin) in Login.permit; current network_policy.rs::acquire_destination invokes http_native_source::Binding::acquire, which retains OriginUse and NetworkUse/assignment resources. Source-use contract requires idle cached factories not count as activity.
- Current auth::connect ALab hook binds auth.alab_bind_http_client_factory before token extraction; preserve it and the AuthConfig::load_auth_with_modes direct-reader admission hook. New current() rereads stored credentials, so test that retirement prevents an old worker reading/adopting B even with the same token/account; exact-string comparison alone is not source identity.
- Planned existing checks: native_submission_uses_one_admission_and_keeps_preflight_guards; successful_reopen_supersedes_only_earlier_losses_for_that_thread; attachment_status_follows_its_server_notifications; reconnect_queues_enter_and_resumes_ordinary_next_turn_submission; retained_cloud_transport_owns_its_actual_reader_and_destination_policy; native_network_request_keeps_resource_until_last_body_use_not_cached_factory; both native upgrade completion-order tests.
- Required new deterministic tests: idle recovery owner contributes zero activity; accepted recovery holds its original source through auth read/attach/result; revoke-then-allow and source A-to-B cannot revive old recovery; cancellation releases episode activity; reconnect-only event cannot restore cleared seen turns/goals or release the upgrade completion fence. No tests/builds run in this review.

Revisit: Before shipping integrated recovery, implement and pass the source-bound observer/episode tests plus event-dispatch adaptations. SourceV1 stays unadvertised until its independent joined producer/controller/consumer/successor qualification completes.

## Verification limits

Subset review fields pass `upstream_intake.validate` using identical synthetic subset metadata and current feature IDs; immutable SHA/subject/path fields match the packet. This does not replace validation of the parent's full frontier record. Native Windows execution, Linux sandbox behavior, generated export reproducibility and composed ALab Rust builds remain for their gate lanes. The source review provides no basis to remove account-isolation, native-handoff, overflow-ordering or fixture-clock hooks.
