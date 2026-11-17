<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Source intake: twelve commits after 4f556, through d3b769

Exactly twelve contiguous upstream commits: `4f556fdd6db3f614207bacc037b1bc4452e301bf..d3b7695db912fd7fc8c30d814d172e75fefbadde`. Fixed ALab comparison `73dceee9d2f3a8398c3a7fa9db56a1a93d92018e`. Root supplied a fresh managed upstream observation `59ac39addbbcd3499319c805a916846ca8ac5fef`; the author used cached objects, performed no fetch, and reviewed only this twelve-commit cohort. Later local origin observation `73dbb5eae284dd0bfd15f99a9d8038baa12f6a87` changes only four request-diagnostics paths from the fixed basis and overlaps neither intake metadata nor bound owned code.

**10 adopt / 1 keep-opt-in / 1 adapt.** Canonical Git fuller/binary serialization was matched byte-for-byte: **565,110 bytes /  13,959 lines**. All production, tests, text snapshots and manifest hunks were reviewed. Row 11 stable generated Zstd was bounded-decompressed/parsed separately: only two UserInput description strings changed inside RolloutLine schema; no generator ran. Row 12 moved 875-line tests were read in the new path and matched the deleted path byte-for-byte after explicit recorded identifier/comment renames. This record changes only review/intake metadata, imports no upstream code and implements no adaptation.

Every test candidate is **not_run / UNRUN**, **final_binding=null**. No Rust/compiler/Cargo metadata/native/helper/build/test/lint/formatter/profile/lock/pin/cache/SDK/admission-FD/credential/default/permission or held-route change occurred. Source metadata, declaration and Python schema checks are not runtime, Send, stack, timing, hardware, durability, containment or application qualification.

Inherited required/unimplemented 81/92 terminal Err/EOF custody, f226 exactly-once native tool completion, a394/598/e51 exec/source/root/input/mail/history/stdio custody, typed generic/auth/quota and natural Empty original authority, retained actual usage/current native Goal budget, policy API issuance and cancellation/retirement obligations remain OPEN. See [after336](upstream-intake-2026-11-16-after336.md), [after5cb](upstream-intake-2026-11-16-after5cb.md), [aftera11d](upstream-intake-2026-11-17-aftera11d.md). Private repairs and review previews are not presumed present in fixed main or qualified. No ALab hook is declared obsolete/equivalent or authorized for removal by these decisions.

ALab packaged/aterm defaults keep apps/plugins off and omit the new Guardian flag. Cloud listing is not activation; Guardian evidence is not foreground Goal admission; root session ID is not native parent/root permission. Held SourceV1/all-root receiver routes remain held, never enabled to obtain tests. No SQL migration/index numbering changes occur in this cohort. Upstream manifest/lock changes require later normal maintained ALab re-resolution, not manual upstream lock entries.

## Decisions

| # | Commit | Decision | Subject |
|---|---|---|---|
| 1 | `e3b721754e1e` | adopt | Freeze Guardian review views at ordered evidence cutoffs (#61929) |
| 2 | `d32c2ba608b1` | adopt | Log Windows MXC deny-read glob expansion timings (#61931) |
| 3 | `9537934ed6d5` | adopt | Allow independent app-server thread resumes to overlap (#61934) |
| 4 | `cc356fd384b9` | adopt | Preserve partial Git metadata when worktree status is slow (#61939) |
| 5 | `9b2d2c12db7a` | adopt | Add saved environment listing to the Cloud client (#61940) |
| 6 | `1d9786941238` | adopt | Route turn-start client metadata through a dedicated helper (#61941) |
| 7 | `c65601417707` | adopt | Centralize exec-server launches in `WrappedSandboxCommand` (#61942) |
| 8 | `34c7cbb454b3` | adopt | Allow host-started forks to inherit the root session ID (#61945) |
| 9 | `0637230e89be` | keep-opt-in | Include enclosing tool context in nested Guardian approvals (#61946) |
| 10 | `ad161203cb8e` | adopt | Mask system SSH config drop-ins in the Linux sandbox (#61947) |
| 11 | `5b3d2c7fce11` | adopt | Extract user input preparation into `codex-user-input` (#61949) |
| 12 | `d3b7695db912` | adapt | Refactor composer undo and redo history (#61951) |

## 1. e3b721754e1e — adopt

Order actual Guardian evidence snapshots and review subscription views through one bounded writer queue so overlapping reviews cannot see later revocations. This repairs evidence visibility without enabling Guardian or turning transcript evidence into execution or Goal authority.

Canonical SHA256 `f86890bbf5058bb891c0dd473849a3c7815ff7b080e6d4793bd6aa3673cc8a90` (39,476 bytes /  1,039 lines).

Primary source at exact final cohort:

- `codex-rs/ext/guardian-v3/src/cutoff.rs:31` — `pub struct RootEvidence`.
- `codex-rs/ext/guardian-v3/src/host.rs:114` — `capture.finish`.

Integration requirements:

- Retain the real flush/read/provenance and sole housekeeper routing-before-view boundary, plus root cancellation/worker join. Cursor acknowledgement is queue admission, never a durable board-write or policy receipt.
- Keep existing Guardian defaults and apps/plugins gates; cutoffs and retained-source order do not create native Goal/request/tool permission.

Coverage limits:

- The three new real-history/board controls establish candidate cutoff semantics only and may skip without network. They do not exercise actual native approval consumption or certify durability.
- One-slot backpressure serializes flush/read/reservation and adds an activity worker; no latency, allocation-free or zero-IO claim.

## 2. d32c2ba608b1 — adopt

Bind Windows MXC runtime logging to the host-selected home rather than workload CODEX_HOME and report aggregate deny-glob expansion timings without changing policy expansion.

Canonical SHA256 `2074d2780d4d997d01b5c07cae74b31d598994aef9e62645a65af682fc789975` (7,875 bytes / 202 lines).

Primary source at exact final cohort:

- `codex-rs/mxc-sandbox/src/lib.rs:97` — `The workload environment can omit`.
- `codex-rs/windows-sandbox-rs/src/logging.rs:26` — `pub fn init_runtime_log`.

Integration requirements:

- Preserve optional host argument, absolute-path-only runtime initialization and existing platform/security policy handling. Do not use timing logs as containment or performance qualification.

Coverage limits:

- Existing wrapper selector verifies transport argv/cwd plus optional argument shape; it does not verify Windows log destination, missing home or host/workload override matrix. Actual Windows launcher/log controls remain required.
- No glob strings, file contents or credential values are added to the timing record; ordinary Windows logging retains its existing custody boundaries.

## 3. 9537934ed6d5 — adopt

Share list permits only for canonical-ID independent root resumes, with exact source/ID checks and FIFO-preserving exclusive upgrade for uncertain ownership. ALab archive/delete management still calls the unchanged exclusive API.

Canonical SHA256 `4b5cfeb9fca593b43b8d25fa6114a9e1dc2860574be4128e83c204ed1a12d749` (29,820 bytes / 737 lines).

Primary source at exact final cohort:

- `codex-rs/app-server/src/request_processors/thread_list_state.rs:48` — `pub(super) async fn acquire_resume`.
- `codex-rs/app-server/src/request_processors/thread_processor/resume_permit.rs:63` — `pub(super) fn source_can_share`.

Integration requirements:

- Preserve aliases/path/supplied-history/daemon/parent-linked/unknown-source exclusivity, actual per-ID request queue and retry source checks. Config/MCP work may overlap only after the native source probe completes.
- Keep ALab retained task management and delete helpers under the real exclusive permit through their original notification boundaries; no extra outer self-deadlocking permit. Retain Goal resume, actual account and source guards.

Coverage limits:

- Selected actual MCP/canonical-path/permit-order controls are UNRUN; no proof of all account/source race schedules or latency improvement.
- Read/list serialization is not native turn admission, a transferred source receipt, or permission to enable held receiver routes.

## 4. cc356fd384b9 — adopt

Publish partial verified Git metadata before a Memory deadline, tie status to its own HEAD, and use completed positive-only recent-directory probes without blocking model dispatch on them.

Canonical SHA256 `99b2c55a25ebfd19c806e823b00f720c667333687ccadeb1545a203c17b62e29` (60,598 bytes /  1,581 lines).

Primary source at exact final cohort:

- `codex-rs/core/src/turn_metadata.rs:617` — `async fn collect_workspace_git_metadata`.
- `codex-rs/git-utils/src/status.rs:85` — `pub async fn probe_worktree_changes`.
- `codex-rs/memories/write/src/runtime.rs:367` — `git_probe.try_join_next`.

Integration requirements:

- Retain one-second deadline/cancellation, credential-sanitized remotes, committed/unborn/unavailable HEAD distinction and stale-status rejection. The probe can establish dirty only when HEAD matches and full status remains unknown.
- Memory remains independently gated and nonforeground for native Goal origin; metadata does not grant original request authority, native budget enforcement or permission to activate memory/cloud routes.

Coverage limits:

- At most four chosen root directories become literal pathspecs, but enumerating the root and full Git status IO are not globally bounded; no total IO/latency benchmark claim.
- Selected parser/shared-scan/cancellation/actual-Git/partial-clone controls are static candidates; FIFO test is Unix, network/helper conditions remain.

## 5. 9b2d2c12db7a — adopt

Add an explicitly invoked authenticated Cloud environment catalog without activating Cloud, creating a thread or changing account/provider selection.

Canonical SHA256 `39f9554e344c7e0c4cd55917f3b0cd4b2b4472e47e2dd388795067372bd160c6` (10,290 bytes / 256 lines).

Primary source at exact final cohort:

- `codex-rs/cloud-client/src/environments.rs:31` — `pub async fn list_saved_environments`.

Integration requirements:

- Keep fixed original Client credentials/account headers, independent user/workspace cursors, scope-assigned sharing, non-draft filter and typed failure. An environment ID/name/shared flag is catalog data, not account activation or native cwd/root/exec authority.
- Preserve existing Cloud opt-in and source isolation; do not auto-refresh this catalog merely because the API exists.

Coverage limits:

- Ten seconds bounds the whole catalog and one MiB bounds each page, not aggregate bytes, retained entries or number of pages; cursor repetition fails.
- The one new happy pagination/header/default-filter selector does not cover timeout, oversize, malformed pages, repeated cursors, blank IDs/names or failure cleanup. No live Cloud request ran.

## 6. 1d9786941238 — adopt

Route both existing turn-start metadata updates through a dedicated delegating helper with unchanged native input ordering and merge semantics.

Canonical SHA256 `93e70227795cd5d7832afedee394872ba30e83d5d95be8c8a0cf9aaeef909350` (3,252 bytes / 80 lines).

Primary source at exact final cohort:

- `codex-rs/core/src/turn_metadata.rs:383` — `pub(crate) fn apply_start_responsesapi_client_metadata`.
- `codex-rs/core/src/session/turn_input.rs:505` — `.apply_start_responsesapi_client_metadata`.

Integration requirements:

- Preserve reserved metadata ownership, real start-or-steer/start-if-idle guards and settings/native admission chronology. Client metadata is not new policy authority.

Coverage limits:

- No new declaration was introduced; selected existing merge/input controls remain UNRUN and do not independently qualify every start-path schedule.

## 7. c65601417707 — adopt

Keep process launch, Windows token-session restrictions and snapshot script replacement in the actual prepared WrappedSandboxCommand while preserving explicit proxy and inherited-FD ownership. The bounded fixed-main caller census found no ALab use of the changed direct APIs.

Canonical SHA256 `c8b768f548d7d2b8783e025a883cd98eeb3039ec324e1a62ecc20973758815e0` (115,733 bytes / 2,630 lines).

Primary source at exact final cohort:

- `codex-rs/sandboxing/src/launch.rs:161` — `pub async fn spawn`.
- `codex-rs/sandboxing/src/launch.rs:198` — `pub fn into_command_request`.
- `codex-rs/exec-server/src/local_process.rs:121` — `Descendants can still use the proxy`.

Integration requirements:

- Preserve real native_program separately from argv0, command/policy cwd, validated env, sandbox/proxy/SID/filesystem overrides and attached/inherited stdio/FD lifetimes through actual spawn. Fallible plain-request conversion must refuse required Windows token sessions.
- Retain ALab native wrapper/admission FD/root/parent/source/input/mailbox/history/Goal/lease semantics. This refactor does not supersede a394/598/e51 or authorize deletion/activation of ALab transfer/held SourceV1 routes.
- Keep snapshot replacement limited to the submitted inner shell and original prepared sandbox/proxy. None versus Some(empty) runtime-readable paths is intentional automatic-CA versus executor-complete selection, never broadened filesystem access.

Coverage limits:

- Integrity checks remain observation-only with existing worker timeout; borrowed facts or timings are not containment proof. New WindowsSessionPolicy Box and retained proxy resources have unmeasured costs.
- Selected real proxy-after-leader-exit, snapshot retries, native validation, Unix helper and Windows TEMP/SID controls require actual applicable platform/backend/helper execution. Status0 helper checks do not claim the separate Core exit17 oracle.

## 8. 34c7cbb454b3 — adopt

Permit a trusted host to supply inherited root session identity for prepared forks while keeping child thread IDs distinct, rejecting explicit resume/native-child inheritance and retaining their actual ownership paths.

Canonical SHA256 `9c12991d8c1cc0358958d9ba95360212b2084d27db5c2c1ad063eb3adcf48458` (7,982 bytes /  177 lines).

Primary source at exact final cohort:

- `codex-rs/core/src/thread_manager.rs:2566` — `InitialHistory::New | InitialHistory::Forked`.
- `codex-rs/core/src/agent/host/types.rs:35` — `The root's session ID`.

Integration requirements:

- Keep actual fork/history/session persistence, host startup ExtensionData authority and native controller session inheritance. A shared session ID does not substitute for native parent/root/cwd/account/resource/FD leases or a source receipt.
- Preserve ALab prepared-fork/exec recovery and held-root restrictions; never turn transferred/session text into authorization.

Coverage limits:

- Three declared host history cases exercise real startup/resume and explicit invalid overrides but may skip without network. They do not qualify ALab image handoff or held receiver routes.

## 9. 0637230e89be — keep-opt-in

Keep new GuardianNestedApprovalContext disabled by default. When explicitly enabled, host-matched enclosing call context is optional review evidence while the nested action remains the exact independent action under review.

Canonical SHA256 `3cd65c8593beb27db804f1141d12318a98823145ab068aca53b4818488819799` (74,070 bytes /  1,730 lines).

Primary source at exact final cohort:

- `codex-rs/features/src/lib.rs:2219` — `key: "guardian_nested_approval_context"`.
- `codex-rs/core/src/session/mcp.rs:1352` — `review_context.enclosing_invocation_id = Some`.
- `codex-rs/guardian-context/src/transcript.rs:504` — `enclosing_invocations == 1`.

Integration requirements:

- Preserve explicit default false/ALab defaults absence, JS preflight exclusion, actual host originating call ID and ambiguous/omitted evidence refusal. Provider IDs or equal argument strings do not identify original authority.
- Only spare fixed/elastic budget may admit enclosing context, after ordinary transcript and screenshots. It must not displace action/evidence, upgrade a denied native Cause or mint foreground Goal permission.

Coverage limits:

- Actual MCP five-request snapshot/default-off and macro attribution cases plus pure budget/image tests are UNRUN; loops are not extra declarations.
- Nested context has redacted Debug but deliberately carries real original arguments in the opt-in review prompt; original tool permissions/account/source/privacy boundaries still apply. No generic/Empty/request authority gap closes.

Revisit: Consider activation only after actual native nested MCP attribution/default-off/ambiguous or missing evidence/budget/screenshot controls and ALab original cause/approval integration are qualified; preserve independent verdicts and original Goal ownership.

## 10. ad161203cb8e — adopt

Mask inherited system SSH drop-ins in filesystem-restricted Linux sandbox commands to avoid unmapped root-ownership rejection, preserving explicit read/write grants and safe path targeting.

Canonical SHA256 `48648e96a633d9845754a2aabac72da090c638bbee4b52faa77f887191b8912e` (10,501 bytes / 242 lines).

Primary source at exact final cohort:

- `codex-rs/linux-sandbox/src/bwrap.rs:487` — `let ssh_drop_ins = Path::new`.
- `codex-rs/linux-sandbox/README.md:84` — `KnownHostsCommand`.

Integration requirements:

- Preserve existing unreadable-root precedence, explicit descendant reads/overlapping writes and symlink/safe-ancestor refusal; main/user SSH configuration and native host-key checks remain actual.
- Record compatibility impact: masked site settings including KnownHostsCommand are skipped. This does not activate networking, grant SSH credentials, infer account permission or justify changing ALab admission/held routes.

Coverage limits:

- The real two-owner private-namespace SSH/changed-host-key fixture may return successfully without executing when nonroot or helpers/namespaces unavailable. Linux declarations do not require tests to execute on this Mac.
- No focused new explicit-read/write/symlink matrix was added. No machine SSH files were touched or live handshake executed by this review.

## 11. 5b3d2c7fce11 — adopt

Move local attachment snapshot and UserInput-to-content conversion to codex-user-input while preserving wire data, image-slot indices, labels and error placeholders. Fixed-main owned codecs hold UserInput data rather than calling the removed conversion APIs.

Canonical SHA256 `90f5fbdf69015c2a5df45df4dd5e9ed1d2ba4b45af867c5080dcb6d45f95b909` (101,694 bytes / 2,646 lines).

Primary source at exact final cohort:

- `codex-rs/user-input/src/lib.rs:9` — `pub use message::from_user_input`.
- `codex-rs/user-input/src/message.rs:82` — `pub fn from_user_input`.
- `codex-rs/user-input/src/local_media.rs:19` — `pub fn snapshot_local_user_input`.

Integration requirements:

- Retain actual pre-conversion accepted-input/source ownership, original snapshot/read errors, Process/Defer image-index mapping, queued media snapshots and ALab retained input/mail delivery/copy authority. Do not re-read source attachments on a destination or invent a media permission.
- Use actual new crate exports for any newly discovered conversion caller; remove obsolete protocol API compatibility rather than restoring blanket From conversions. Accept upstream manifest/workspace/lock changes normally and re-resolve ALab entries through maintained tooling later; no manual lock edits.

Coverage limits:

- The moved snapshot helper bounds actual media reads; the separate existing message converter still uses fs::read before preparation, so this extraction is not a new universal IO/memory bound.
- Stable generated asset was bounded-decompressed and parsed; only the two LocalImage/LocalAudio description strings changed inside RolloutLine schema. No schema generator/conformance/Rust run. Moved tests are one preserved declaration each, not new runtime evidence.

## 12. d3b7695db912 — adapt

Migrate actual ALab retained editor/stash/search/strict codec references from removed vim_history::VimHistory and composer fields to edit_history::EditHistory. Dropping native undo/redo/pending transactions would lose retained user state.

Canonical SHA256 `1fd0c72e29c914daff266fa76f912e143ea373f2ca726cbb85d8bfbe519cdd2b` (103,819 bytes / 2,639 lines).

Primary source at exact final cohort:

- `codex-rs/tui/src/bottom_pane/chat_composer/edit_history.rs:26` — `pub(super) struct EditHistory`.
- `codex-rs/tui/src/bottom_pane/chat_composer/edit_history.rs:55` — `fn step(`.
- `codex-rs/tui/src/bottom_pane/chat_composer/history_search.rs:68` — `original_edit_history: EditHistory`.

Integration requirements:

- Move the existing ALAB editor-state codec module hook from the removed upstream vim_history path to the new stable edit_history path, and update owned private types/fields/history_bytes/native constants in all eight identified retained editor/stash/search/capture/restore paths. Keep behavior ALab-owned and upstream hook thin/marked.
- Preserve exact rich draft text/elements/images/mention bindings/paste payloads, both bounded undo/redo stacks, pending transaction, temporary search original history and active Vim mode/register state across supported stash/exec transfer. Do not default, re-render, drop history or treat restored text as new input/admission.
- Review strict deny_unknown_fields and actual version/preflight negotiation before changing serialized field names; either support actual typed data with complete validation or explicitly refuse unsupported payloads. No silent old/new-schema flattening or invented destination ownership.

Coverage limits:

- The 875-line renamed upstream test source is byte-identical after recorded identifier/comment renames; only production stack transfer is consolidated. It adds no new semantic test cases.
- Selected native composer and actual ALab no-primary/stash/occupied-owner controls are existing candidates requiring re-execution after real adaptation. No retained codec implementation or source import is made by this record.

## Static candidate inventory

**44 upstream selector rows / 44 declarations, twelve packages**, plus **two existing owned declarations compiled inside codex-tui --lib**, yielding **46 declarations / 70 direct-attribute cases** before platform filtering. There are **55 verified module/include edges**. Exact package manifests, actual target roots, module attributes/cfg/ignore, macro labels and prerequisites are bound externally. Macro generated exact names, loop iterations, helper calls, snapshots and assertions are not additional declarations. Linux SSH may skip; Windows RPC is ignored off Windows; macOS/Unix source conditions remain explicit. The moved test source is not newly executed coverage.

| Row | Actual package / target | Declaration | Scope |
|---|---|---|---|
| 1 | `codex-guardian-v3 --lib` | `cutoff::tests::skips_a_partial_delivery_once_across_wakes_and_freezes` | Actual retained history skips an incomplete delivery once and preserves same-position freeze ordering; not an approval verdict. |
| 1 | `codex-guardian-v3 --lib` | `cutoff::tests::retries_unavailable_history_and_remembers_rejected_evidence` | Actual history unavailability retries while rejected provenance remains sticky. |
| 1 | `codex-guardian-v3 --lib` | `cutoff::tests::overlapping_reviews_only_expose_a_revocation_in_the_second_view` | Actual board writes and frozen subscription views keep the later revocation out of the earlier view; no native Goal authority. |
| 2 | `codex-mxc-sandbox --lib` | `tests::wrapper_preserves_exact_argv_and_separate_command_cwd` | Prepared transport preserves workload argv and separate cwd while optional host logging argument is platform bounded; no actual log-path validation. |
| 3 | `codex-app-server --lib` | `request_processors::thread_list_state::tests::upgrades_follow_probe_order_even_when_the_second_probe_finishes_first` | Actual read/admission/probe permits keep FIFO upgrades ahead of a queued writer. |
| 3 | `codex-app-server --lib` | `request_processors::thread_lifecycle::listener_shutdown_tests::only_canonical_id_resumes_share_list_state` | Canonical loaded resume shares while alias and missing-path probes wait for exclusive ordering. |
| 3 | `codex-app-server --test all` | `suite::v2::thread_resume::independent_cold_resumes_overlap` | Real required-MCP startup gate permits independent root resume to complete. |
| 4 | `codex-core --lib` | `responses_metadata::tests::memory_positive_status_requires_matching_head_and_unknown_full_status` | Positive Memory observation needs original matching HEAD and unknown full status. |
| 4 | `codex-core --lib` | `turn_metadata::tests::memory_workspaces_keep_observed_refs_when_status_exceeds_the_deadline` | Unix actual Git index FIFO holds status while observed HEAD/remote survive the one-second deadline. |
| 4 | `codex-core --lib` | `turn_metadata::tests::git_collection_keeps_independently_observed_refs_until_the_deadline` | Two paused-time macro cases retain separately observed HEAD/remotes while other lookups remain pending. |
| 4 | `codex-core --lib` | `turn_metadata::tests::git_enrichment_does_not_mark_an_old_head_clean` | Actual new HEAD status cannot overwrite independently observed older HEAD with clean state. |
| 4 | `codex-git-utils --lib` | `status::tests::change_probe_only_returns_git_verified_positive_status` | Real Git positive-only literal-directory probe never claims clean from a limited scan. |
| 4 | `codex-git-utils --lib` | `status::tests::concurrent_change_probes_share_one_scan` | Shared pending probe executes one closure; component scheduling, not benchmark. |
| 4 | `codex-git-utils --lib` | `status::tests::porcelain_status_requires_one_valid_head_observation` | Pure parser rejects absent, malformed or duplicate HEAD headers. |
| 4 | `codex-core --test all` | `suite::git_enrichment::user_turn_git_enrichment_redacts_remote_credentials` | Ordinary/partial-clone request metadata excludes actual configured URL credentials. |
| 5 | `codex-cloud-client --lib` | `tests::saved_environment_catalog_follows_each_scope_cursor` | Authenticated personal/workspace pagination encodes cursor and omits default environment; failure/limit matrix is not tested. |
| 6 | `codex-core --lib` | `turn_metadata::tests::turn_metadata_state_merges_client_metadata_without_replacing_reserved_fields` | Existing client metadata merge preserves reserved host fields; helper itself delegates unchanged. |
| 6 | `codex-core --lib` | `session::turn_input::tests::steer_only_enforces_expected_turn_id` | Actual existing input identity guard and same on-start request construction remain; no new admission grant. |
| 7 | `codex-exec-server --lib` | `local_process::tests::native_start_and_prewarm_validate_launch_inputs` | Actual LocalProcess start/prewarm reject foreign cwd/invalid scope, allow empty native prewarm but reject empty command. |
| 7 | `codex-exec-server --lib` | `process_sandbox::tests::native_request_preserves_native_launch_fields` | Prepared native argv/cwd/env/arg0 retained through fallible request conversion. |
| 7 | `codex-exec-server --lib` | `process_sandbox::tests::sandbox_request_preserves_executor_local_proxy_unix_socket_policy` | macOS snapshot capture keeps actual executor-local proxy socket policy and live proxy address. |
| 7 | `codex-exec-server --lib` | `shell_snapshot::tests::snapshot_failure_retries_are_bounded_and_single_flight` | Actual shared snapshot capture retains fallback/replay/backoff and environment with original shell invocation. |
| 7 | `codex-exec-server --test exec_process` | `exec_process_retains_output_after_exit_until_streams_close` | Local/remote declared cases retain real proxy reachability after leader exit while descendant output is open. |
| 7 | `codex-exec-server --test exec_process` | `windows_sandbox::tmpdir_uses_command_environment_over_rpc` | Three Windows RPC macro cases preserve command TEMP grants with empty roots and managed proxy offline SID; non-Windows ignored. |
| 7 | `codex-sandboxing --lib` | `manager::tests::inherited_terminal_preserves_native_linux_helper_and_uses_its_dispatch_name` | Unix actual native helper/arg0 direct+PTY execution retains status0; not the separate Core exit17 test. |
| 8 | `codex-core --test all` | `suite::agent_control::hosted_child_persists_inherited_session_id` | Fresh legacy/paginated and prepared fork retain trusted root session ID but distinct thread IDs; resumes/native children reject injected inheritance. |
| 9 | `codex-core --test all` | `suite::guardian_mcp_elicitation::node_elicitations_attribute_independent_reviews_without_changing_actions` | Actual MCP fixture produces independently attributed allow/deny actions with optional host-matched enclosing evidence; actual macro variants recorded. |
| 9 | `codex-core --test all` | `suite::guardian_mcp_elicitation::strict_browser_nested_review_omits_enclosing_call_by_default` | Actual default-disabled feature omits enclosing call while nested action remains reviewed. |
| 9 | `codex-core --test all` | `suite::scenarios::guardian_nested_elicitation_request_history` | Five actual request snapshot preserves enclosing call through transcript delta; not Goal/native authority. |
| 9 | `codex-guardian-context --lib` | `transcript::tests::enclosing_call_is_matched_by_id_even_when_wrapper_arguments_match` | Pure history attribution selects exact host call ID rather than matching arguments. |
| 9 | `codex-guardian-context --lib` | `transcript::tests::budget_drops_the_enclosing_call_and_its_label_together` | Fixed/elastic budget and entry limits omit parent label/context together while preserving action and ordinary evidence. |
| 9 | `codex-guardian-context --lib` | `images::tests::fixed_screenshot_admission_precedes_optional_enclosing_call` | Actual composition estimator gives selected screenshots priority over optional enclosing evidence; not hardware rendering. |
| 10 | `codex-linux-sandbox --test all` | `suite::sandbox::ssh_config_tests::ssh_masks_system_dropins_and_rejects_a_changed_host_key` | Actual two-owner private-namespace SSH success plus changed-host-key rejection; root/unshare/bwrap/tools/namespaces may skip. |
| 11 | `codex-user-input --lib` | `local_media::tests::snapshots_local_image_user_input_with_requested_detail` | Moved bounded snapshot preserves actual image detail and inline data. |
| 11 | `codex-user-input --lib` | `local_media::tests::rejects_invalid_local_media_without_changing_user_input` | Moved invalid-image/unsupported-audio rejection leaves original input unchanged. |
| 11 | `codex-user-input --lib` | `local_media::tests::rejects_oversized_local_media_without_reading_it` | Moved snapshot metadata/actual byte cap rejects oversize before replacing input; not a universal conversion read bound. |
| 11 | `codex-user-input --lib` | `message_tests::user_input_image_indices_survive_content_expansion` | Moved Process/Defer conversions retain original input-to-image slots across missing reads/framing/mentions/duplicate URLs. |
| 11 | `codex-user-input --lib` | `message_tests::file_image_user_input_serializes_file_id` | Moved file-backed image remains file_id with unchanged wire data; no host file resolution grant. |
| 11 | `codex-user-input --lib` | `message_tests::mixed_remote_and_local_audio_share_label_sequence` | Moved inline/local audio preserves actual labels and MIME/data content. |
| 12 | `codex-tui --lib` | `bottom_pane::chat_composer::edit_history::tests::undo_and_redo_group_complete_vim_edits` | Renamed existing real composer edits preserve full transaction undo/redo grouping. |
| 12 | `codex-tui --lib` | `bottom_pane::chat_composer::edit_history::tests::recovered_answers_undo_as_one_vim_edit` | Existing recovered drafts retain grouping, mode, search cancellation and escaped command prefixes. |
| 12 | `codex-tui --lib` | `bottom_pane::chat_composer::edit_history::tests::undo_and_redo_restore_deleted_large_paste_payload` | Existing actual paste payload survives undo/redo with rich draft state. |
| 12 | `codex-tui --lib` | `bottom_pane::chat_composer::edit_history::tests::undo_and_redo_restore_deleted_image_attachment` | Existing actual attachment deletion restores full draft on undo and removes it on redo. |
| 12 | `codex-tui --lib` | `bottom_pane::chat_composer::edit_history::tests::canceled_reverse_history_preview_preserves_vim_undo_and_repeat` | Existing temporary history preview preserves original undo/repeat/search semantics. |
| 12 | `codex-tui --lib` | `bottom_pane::chat_composer::inline_input::alab_stash::draft_checkpoint::retained_no_primary::tests::provisional_rich_draft_and_real_stash_round_trip_without_primary_identity` | Actual bounded provisional rich draft and real stashed prompt round-trip without a primary identity; extend with renamed history state during adaptation. |
| 12 | `codex-tui --lib` | `bottom_pane::chat_composer::inline_input::alab_stash::draft_checkpoint::retained_no_primary::tests::occupied_stash_and_size_bounds_do_not_replace_actual_editor_owners` | Actual occupied/native stash and size-bound refusal preserves existing editor owners; no ownership from serialized history. |

Eventual runtime boundary: actual selected applicable package targets plus preserved ALab retained editor/stash/exec/input/history/account/Goal/policy and held SyncPost Err/EOF controls. New Windows host logging-path, Cloud failure/limit, SSH grant/symlink and adapted strict EditHistory round-trip controls remain required gaps; current declarations do not substitute for them. No broad gap closes from this source-only intake.
