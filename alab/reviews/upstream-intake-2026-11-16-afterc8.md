<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream intake after c8e4cbcd9476 — source review, 2026-11-16

Exactly 12 contiguous cached commits, `c8e4cbcd9476795a08080f02e3bc09a64f6da80d` through `5cb38e79842a0e0a6dca5bb97874163e8ee50129`. Comparison source `1074b5ff72b1f567490f2dfa1a8a0f0fea5e2c6c`. An initial later origin observation `d725827deac2bbe5ac46390e12631cb898ed8370` changed startup account-read test coverage and its review. Final local origin observation `49d17b516e15e171ea0bafe6fa58251b1736eb46` has 14 changed paths from the fixed comparison: runtime/smoke diagnostics, history-publication Python controls, their tests/reviews, and that account-read test. The catalog/defaults and selected owned Rust declaration paths are unchanged. Preserve the newer maintained runtime/diagnostic controls for eventual qualification; this review rebinds no evidence. Source comparison remains fixed; any later integration needs exact current-source admission and execution.

Decision: **8 adopt, 4 adapt**. Only this review and the matching intake JSON are changed. No upstream merge or production adaptation is implemented.

All 60 selector rows (53 distinct source declarations, 7 packages) are **not_run**. Package manifests, unit/integration target roots and test declarations/annotations were checked statically. Macro annotations below are source variants, not fabricated generated libtest names or execution receipts. Source listing and patch strings are not runtime evidence. No Rust, compiler, native application, formatter, lint, pin/profile/cache/default change or qualification is claimed; final_binding remains null.

## Boundaries

- Keep actual native GoalService writes, accepted-input/history custody, Invocation identity, one-shot completion and source/account/root/permission leases. Upstream rollout spending/gates are distinct from goal token/status persistence and receipts.
- Carry the REQUIRED and UNIMPLEMENTED 81/92 terminal Err/EOF custody adaptation; synchronous hook accounting/output markers are not permission or early publication of a committed goal result. True TaskCancel wins. Retain the required integrated budget-stop/native-goal control gap below.
- Preserve disabled/opt-in apps, plugins and service defaults, explicit model/provider/account choices and the held SourceV1/full-capsule/all-root receiver. No Cloud service or receiver is enabled for evidence.
- Local waiter/read cancellation, navigation, a quiet stream or metadata generation cannot establish remote rollback, future stream health, archive/name ACK or stopped work.
- The unqualified ALab policy contract remains final one-shot API issuance; this intake does not promise retroactive cancellation after Core acceptance.

## Inherited requirements — not implemented here

The [prior after336 review](upstream-intake-2026-11-16-after336.md) declares `81b995a6c9a4c978f399775cfa5ce50702368749` (called **81** below) an ADAPT decision with a **REQUIRED and UNIMPLEMENTED** terminal Err/EOF custody adaptation. `92c1a4dc2e1ff83dd0d2f06f15f03df21b8cb2f2` (**92**) depends on that custody contract; its outcome/accounting carrier must not publish billing or native completion early. The same prior review requires an **UNIMPLEMENTED** owned-hook integration adaptation for `f226c9ccfb9762eb5b8fa5d45b43262304f5a360` (**f226**). This batch implements none of them.

The referenced ALab one-shot policy-issuance fix is a separate pending, unqualified source chain (`b2451d506fdc4f1dc894ef225d8913eaaf42f648` and `a437366676fb1647cca66ef65d926db72d620066`), absent from the fixed comparison and observed main. It is not silently imported or treated as implemented by these decisions. Its contract stops at final native API issuance, not retroactive Core queue cancellation.

## Canonical patches

Digests include exact headers and binary-safe patches from `git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA`. Bytes are retained externally and rechecked against cached Git.

| Commit | Decision | Subject | SHA256 |
| --- | --- | --- | --- |
| `c8c3f70893f4` | adopt | Account for synchronous MCP post-hook costs in rollout budgets (#61580) | `12f1f3250e39ed33e73a1c5d2215f3dba943a921f535338bda3bbf47f04337c4` |
| `2e61e8ec4e1c` | adapt | Record neutral interruption markers for rollout budget stops (#61581) | `620ff3d5ba709a10f09342700de3922f42cd11ae82776489ffb8921c56736659` |
| `ce82e38b1689` | adapt | Gate new rollout work on shared budget exhaustion (#61582) | `7e05e616ee1dcb99ad6e4b3c8b13a642934cdde4e36dff740f9d6ec17650bb3f` |
| `eaa25e1f43a5` | adopt | Refactor rollout budget checks and turn completion state (#61583) | `0e7f1e53a2497802fb8ec1697c0e2d124f662a076b9d71ab11879c44e8c11f8b` |
| `3b8bd2ee7e95` | adopt | Add request IDs and attempt numbers to model request hooks (#61587) | `2a1cb9464f568dd358790b3d46de8e842e94eba54f48fd7a2dbbe99c778621ad` |
| `e8e5108c9798` | adapt | Scope TUI history item lookups to their owning turns (#61595) | `1ba63e58cdd0e2eb92fa2b5706004bb488d70d380a52182ce48e0c0ce29f2692` |
| `b7056f340852` | adopt | Keep agent overview refresh state scoped to each backend (#61596) | `6b07f026fe9dc55b06273b0d86a7c18fa155293680b3d3d5ad87dd8c3d5590fa` |
| `8e30237f643c` | adapt | Report Cloud stream recovery failures and cancel abandoned reads (#61599) | `27bbd82a569d0d76aa729e95919b5b7e1e96667b9a70278b96f3cbc170a04ff1` |
| `b016cd184725` | adopt | Enable automatic titles for Cloud conversations (#61600) | `b0deb6ebeca2f1cef47758034270f9bf6b682091fcf6e0e0f6211c867406e067` |
| `02e745f3a081` | adopt | Return to the command center while archiving a task (#61619) | `07b568e7d6544be31a86c6c548a6993f259fcd78555e2a67a9223a5c8accfc07` |
| `ee344ca94583` | adopt | Clarify window ID discovery in history tool guidance (#61622) | `ccbb6d7af20f48505c752b706d8c8c044f7e66c4676c68dbb54458e06402105e` |
| `5cb38e79842a` | adopt | Fix config reads with relative working directories (#61663) | `bb7b990e68ae263f004fb49b9e49bc9a790bb952e5e2372930f83bbafb7e00d9` |

## c8c3f70893f4 — adopt

Accept synchronous successful MCP post-hook contributions as separate finite, nonnegative rollout charges paired with the provider response. Preserve posted-bill custody, successful sibling contributions and nonretryable accounting errors; these costs are not native goal token-budget receipts or permission.

Affected contracts: `native-session`, `product-policy`, `goal-storage-recovery`, `qualification-fixtures`.

Required integration behavior:

- Retain provider usage separately from hook charges, including hook-only completed responses and provider-reported incomplete usage. Never bill failed/async/pre hooks as successful synchronous prices; preserve configuration-order aggregation, overflow lower bounds and privacy-safe invalid-price paths.
- Preserve producer/consumer posted status so one response is charged once, and preserve the original incomplete/backend error while accounting its actual reported usage. Invalid accounting is not an inference retry or rollback of completed native work.
- Implement the REQUIRED and UNIMPLEMENTED 81 terminal-outcome custody adaptation and its 92 dependency before admitting this range: an Err/EOF received before synchronous hooks cannot become successful steering follow-up. Outcome-observation custody must not publish usage, commit or settlement early.

ALab interactions:

- AgentControl.record_usage accepts optional provider usage plus HookCharge; AgentUsageReport adds independent charge/overflow fields. No ALab-owned implementation or exhaustive literal for these types was found in 1074; upstream updates its implementations.
- Existing ALab model_goal_invocation, goal_completion and real GoalService accounting remain separate authorities. Hook stdout and structured price metadata grant no goal, turn, source or account permission.

Required source/coverage gaps (not closed by this review):

- Inherited REQUIRED and UNIMPLEMENTED 81 terminal Err/EOF custody controls remain required; generic hook dispatch/cost tests do not supersede the actual owned terminal-steering oracle or qualify the proposed outcome carrier.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-core --test all` | `model_request_post_hook_orders_tool_dispatch` | `codex-rs/core/tests/suite/hooks.rs:6223` @ `5cb38e79842a` | #[test_case::test_case(ToolDispatchCase::Eager; "default runs the tool before the response ends")]; #[test_case::test_case(ToolDispatchCase::AsyncHook; "an observer does not hold the tool")]; #[test_case::test_case(ToolDispatchCase::AfterComplete; "buffered text and tools wait for completion")]; #[test_case::test_case(ToolDispatchCase::SyncHook; "streaming text and tools do not wait for synchronous pricing")]; #[test_case::test_case(ToolDispatchCase::InterruptedHook; "interrupt discards the held response and its charge")]; #[test_case::test_case(ToolDispatchCase::ExhaustedHook; "the response bill blocks its own tool")]; #[test_case::test_case(ToolDispatchCase::OverflowHook(AgentRuntimeOwnership::Host); "the host receives an overflow lower bound and keeps its policy")]; #[test_case::test_case(ToolDispatchCase::OverflowHook(AgentRuntimeOwnership::Controller); "a native overflow closes even the largest allowance")]; #[tokio::test]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --test all` | `host_accounting_reports_raw_usage_and_preserves_error_semantics` | `codex-rs/core/tests/suite/rollout_budget.rs:750` @ `5cb38e79842a` | #[tokio::test]; #[test_case(HostAccountingBehavior::Exhausted, false ; "host exhausts budget")]; #[test_case(HostAccountingBehavior::Fail, false ; "accounting failure never retries inference")]; #[test_case(HostAccountingBehavior::Fail, true ; "incomplete response keeps original error")]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --test all` | `compaction_accounting_errors_fail_without_retry` | `codex-rs/core/tests/suite/rollout_budget.rs:583` @ `5cb38e79842a` | #[tokio::test(flavor = "multi_thread", worker_threads = 2)]; #[test_case(false, false, false ; "local token usage")]; #[test_case(false, true, false ; "local provider units")]; #[test_case(true, false, false ; "remote v2 token usage")]; #[test_case(true, true, false ; "remote v2 provider units")]; #[test_case(false, false, true ; "local compaction host accounting failure")]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --test all` | `invalid_provider_rollout_budget_units_fail_without_retry` | `codex-rs/core/tests/suite/rollout_budget.rs:216` @ `5cb38e79842a` | #[tokio::test(flavor = "multi_thread", worker_threads = 2)]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --lib` | `steering_cannot_erase_a_received_cyber_policy_refusal` | `codex-rs/alab-core/glue/terminal_steering_tests.rs:649` @ `1074b5ff72b1` | #[test]; Owned host include; preserve actual hook marker, parsed backend failure, real steer/cancel, original verdict and request-count oracle. The 81 adaptation remains REQUIRED and UNIMPLEMENTED. |
| `codex-core --lib` | `an_actual_interrupt_keeps_priority_over_a_received_refusal` | `codex-rs/alab-core/glue/terminal_steering_tests.rs:692` @ `1074b5ff72b1` | #[test]; Owned host include; preserve actual hook marker, parsed backend failure, real steer/cancel, original verdict and request-count oracle. The 81 adaptation remains REQUIRED and UNIMPLEMENTED. |

## 2e61e8ec4e1c — adapt

Accept neutral model-visible history for automatic rollout-budget stops and preserve true user/task interruption. Reapply ALab retained goal-tool settlement and transfer-reason ownership across the refactored task finish/abort and late tool-start paths.

Affected contracts: `native-session`, `execution-continuity`, `exec-handoff`, `goal-storage-recovery`.

Required integration behavior:

- Keep automatic SessionBudgetExceeded guidance neutral/developer-scoped and flush its actual marker before the terminal event; marker text is not a user stop, native mutation receipt or accepted-input authority.
- Retain both owned settle_cancelled_task and settle_turn boundaries in tasks/mod.rs before terminal lifecycle/retirement, with the real already-cancelled task joined. Preserve source assignment activity through those callbacks.
- Retain Invocation identity, the canonical terminal_outcome OnceLock and actual transfer attribution in tools/parallel.rs; preserve one actual terminal outcome without restoring the obsolete terminal_outcome_reached atomic. A budget refusal before handler start reports Aborted exactly once; an already committed native operation keeps its actual completion and outcome instead of acquiring a fabricated model stop.

ALab interactions:

- Canonical task completion now carries abort reason plus optional ErrorEvent and derives neutral history from the real stop cause. ends_turn_as_interrupted still recognizes true TurnAborted; automatic budget errors are not silently relabeled as human interruption.
- ALab goal on_turn_abort settles original progress and distinguishes Interrupted from Replaced/ReviewEnded/BudgetLimited; preserve these typed reasons and the upstream neutral marker. Upstream rollout units remain distinct from GoalService goal token/status persistence.

Required source/coverage gaps (not closed by this review):

- Required integrated control: real rollout budget denial/stop while an actual ALab-owned goal-tool completion is retained must preserve exactly one native result and correct nonhuman abort provenance. Existing component controls cover real budget gates and real native waiter teardown independently; no single integrated result is claimed.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-core --test all` | `exhausted_budget_respects_enforcement_on_current_and_later_turns` | `codex-rs/core/tests/suite/rollout_budget.rs:436` @ `5cb38e79842a` | #[tokio::test(flavor = "multi_thread", worker_threads = 2)]; #[test_case(AgentRuntimeOwnership::Controller, true ; "enforced")]; #[test_case(AgentRuntimeOwnership::Controller, false ; "advisory")]; #[test_case(AgentRuntimeOwnership::Host, true ; "host without accounting")]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --lib` | `multi_agent_v2_interrupted_marker_uses_developer_input_message` | `codex-rs/core/src/thread_manager_tests.rs:3841` @ `5cb38e79842a` | #[test]; Source declaration only; actual body execution is not_run. |
| `codex-alab-autopilot --lib` | `a_claimed_tool_finish_survives_forced_waiter_teardown_without_replaying_its_outcome` | `codex-rs/alab-autopilot/src/model_blocked_partial_finish_tests.rs:1108` @ `1074b5ff72b1` | #[test]; Actual native GoalService invocation/one-shot waiter-teardown control; not a synthetic rollback or rollout-budget equivalence proof. |
| `codex-alab-autopilot --lib` | `a_claimed_native_failure_survives_waiter_teardown_without_becoming_a_model_stop` | `codex-rs/alab-autopilot/src/model_blocked_partial_finish_tests.rs:1123` @ `1074b5ff72b1` | #[test]; Actual native GoalService invocation/one-shot waiter-teardown control; not a synthetic rollback or rollout-budget equivalence proof. |

## ce82e38b1689 — adapt

Accept a shared rollout work gate checked before new sampling, compaction, MCP/task startup and late tool execution. Preserve the ALab native/account/root/permission admissions and retained completion identities through the new check boundaries.

Affected contracts: `native-session`, `exec-handoff`, `account-isolation`, `goal-storage-recovery`.

Required integration behavior:

- Implement the REQUIRED and UNIMPLEMENTED f226 owned-hook integration requirement using work_budget/tool_start_denial and the checks before/after observer waits. Keep the real goal_completion prepare-before-eager-dispatch, Invocation source scope, transferred completion retain and reason-attributed abort paths; do not replace them with generic upstream lifecycle notification.
- Budget refusal must occur before new handler work, while in-flight work may finish and report its bill. Recheck after readiness/parallel slot/start-observer waits; no diagnostic flag proves quiescence, backend rollback or durable native completion.
- Retain native assignment/source/exec-root/permission guards at real task admission and queued turn startup. The shared budget gate is an additional admission condition, not service/source authority and not a native GoalService budget mutation.

ALab interactions:

- The host and controller share new-work denial even when the host owns numeric accounting. Missing host accounting is not automatically native accounting; canonical tests retain this distinction.
- The existing final ALab one-shot API issuance policy contract is not retroactive cancellation of already accepted Core queue work. New Core checks remain independent and cannot qualify unpublished ALab policy fixes.

Required source/coverage gaps (not closed by this review):

- Required integrated goal-tool/budget-stop control is shared with 2e; source-declared MCP slot/start/result tests do not by themselves prove actual ALab GoalService receipt custody.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-core --lib` | `mcp_budget_stop_at_handoff` | `codex-rs/core/src/session/turn_start_mcp_tests.rs:250` @ `5cb38e79842a` | #[test_case::test_case(McpPause::Start; "stop_before_send")]; #[test_case::test_case(McpPause::Result; "stop_after_send")]; #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-core --test all` | `model_request_post_hook_orders_tool_dispatch` | `codex-rs/core/tests/suite/hooks.rs:6223` @ `5cb38e79842a` | #[test_case::test_case(ToolDispatchCase::Eager; "default runs the tool before the response ends")]; #[test_case::test_case(ToolDispatchCase::AsyncHook; "an observer does not hold the tool")]; #[test_case::test_case(ToolDispatchCase::AfterComplete; "buffered text and tools wait for completion")]; #[test_case::test_case(ToolDispatchCase::SyncHook; "streaming text and tools do not wait for synchronous pricing")]; #[test_case::test_case(ToolDispatchCase::InterruptedHook; "interrupt discards the held response and its charge")]; #[test_case::test_case(ToolDispatchCase::ExhaustedHook; "the response bill blocks its own tool")]; #[test_case::test_case(ToolDispatchCase::OverflowHook(AgentRuntimeOwnership::Host); "the host receives an overflow lower bound and keeps its policy")]; #[test_case::test_case(ToolDispatchCase::OverflowHook(AgentRuntimeOwnership::Controller); "a native overflow closes even the largest allowance")]; #[tokio::test]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --test all` | `exhausted_budget_respects_enforcement_on_current_and_later_turns` | `codex-rs/core/tests/suite/rollout_budget.rs:436` @ `5cb38e79842a` | #[tokio::test(flavor = "multi_thread", worker_threads = 2)]; #[test_case(AgentRuntimeOwnership::Controller, true ; "enforced")]; #[test_case(AgentRuntimeOwnership::Controller, false ; "advisory")]; #[test_case(AgentRuntimeOwnership::Host, true ; "host without accounting")]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-alab-autopilot --lib` | `a_claimed_tool_finish_survives_forced_waiter_teardown_without_replaying_its_outcome` | `codex-rs/alab-autopilot/src/model_blocked_partial_finish_tests.rs:1108` @ `1074b5ff72b1` | #[test]; Actual native GoalService invocation/one-shot waiter-teardown control; not a synthetic rollback or rollout-budget equivalence proof. |
| `codex-alab-autopilot --lib` | `a_claimed_native_failure_survives_waiter_teardown_without_becoming_a_model_stop` | `codex-rs/alab-autopilot/src/model_blocked_partial_finish_tests.rs:1123` @ `1074b5ff72b1` | #[test]; Actual native GoalService invocation/one-shot waiter-teardown control; not a synthetic rollback or rollout-budget equivalence proof. |

## eaa25e1f43a5 — adopt

Accept consolidated rollout work checks and turn/compaction completion handling while preserving actual installed checkpoints, contributed visible terminal items, pending input and original terminal errors.

Affected contracts: `native-session`, `execution-continuity`, `history-pagination`, `goal-storage-recovery`.

Required integration behavior:

- Preserve the one-way shared work stop while retaining every completed bill, including advisory overflow and actual host decisions. Inline/root-final versus standalone compaction must preserve real install/finish result ordering.
- Retain contributed final-item visibility and mailbox deferral distinctions; record accepted input and last agent message only from actual completed items, not a rollout reminder or a stopped-work flag.
- Retain the REQUIRED and UNIMPLEMENTED 81 terminal Err/EOF custody requirement through the refactored SamplingRequestResult/response-completed loop. A settlement_started/posted-bill observation is not a backend-error identity, and true task cancellation keeps priority.

ALab interactions:

- The private gate module is folded into rollout_budget.rs; no ALab-owned use of the deleted module or changed direct method was found. ALab retry/weekly/quota hooks and pending goal/native input custody remain required on the final composed source.

Required source/coverage gaps (not closed by this review):

- Inherited REQUIRED and UNIMPLEMENTED 81 Err/EOF custody and the shared 2e/ce integrated native-goal budget test remain unqualified; this refactor supplies no superseding execution evidence.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-core --lib` | `plan_mode_uses_contributed_turn_item_for_last_agent_message` | `codex-rs/core/src/session/turn_tests.rs:71` @ `5cb38e79842a` | #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-core --lib` | `handle_output_item_done_returns_contributed_last_agent_message` | `codex-rs/core/src/stream_events_utils_tests.rs:323` @ `5cb38e79842a` | #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-core --lib` | `finalized_turn_item_defers_mailbox_for_contributed_visible_text` | `codex-rs/core/src/stream_events_utils_tests.rs:477` @ `5cb38e79842a` | #[test_case::test_case(false; "default_mode")]; #[test_case::test_case(true; "plan_mode")]; #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-core --lib` | `finalized_turn_item_keeps_mailbox_open_for_nonterminal_text` | `codex-rs/core/src/stream_events_utils_tests.rs:519` @ `5cb38e79842a` | #[test_case::test_case(MessagePhase::Commentary; "commentary")]; #[test_case::test_case(MessagePhase::PartialAnswer; "partial_answer")]; #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-core --test all` | `compaction_accounting_errors_fail_without_retry` | `codex-rs/core/tests/suite/rollout_budget.rs:583` @ `5cb38e79842a` | #[tokio::test(flavor = "multi_thread", worker_threads = 2)]; #[test_case(false, false, false ; "local token usage")]; #[test_case(false, true, false ; "local provider units")]; #[test_case(true, false, false ; "remote v2 token usage")]; #[test_case(true, true, false ; "remote v2 provider units")]; #[test_case(false, false, true ; "local compaction host accounting failure")]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --lib` | `steering_cannot_erase_a_received_cyber_policy_refusal` | `codex-rs/alab-core/glue/terminal_steering_tests.rs:649` @ `1074b5ff72b1` | #[test]; Owned host include; preserve actual hook marker, parsed backend failure, real steer/cancel, original verdict and request-count oracle. The 81 adaptation remains REQUIRED and UNIMPLEMENTED. |
| `codex-core --lib` | `an_actual_interrupt_keeps_priority_over_a_received_refusal` | `codex-rs/alab-core/glue/terminal_steering_tests.rs:692` @ `1074b5ff72b1` | #[test]; Owned host include; preserve actual hook marker, parsed backend failure, real steer/cancel, original verdict and request-count oracle. The 81 adaptation remains REQUIRED and UNIMPLEMENTED. |

## 3b8bd2ee7e95 — adopt

Accept a logical model-request UUID retained across retries/fallback plus a one-based attempt number, while each opened stream keeps its own hook lifecycle ID and transport reconnect is not a new attempt.

Affected contracts: `native-session`, `exec-handoff`, `qualification-fixtures`.

Required integration behavior:

- Preserve distinct request_id, attempt_number, lifecycle_id and provider response identity in sampling and foreground/background compaction. These are observation/correlation data, not native dispatch identity, proof, source permission or goal receipt.
- Keep hook schema/serialization and actual retry/fallback/compaction observations aligned. Retain ALab Invocation identity instead of using a request/call string as an operation owner.

ALab interactions:

- No owned ModelRequestHookInput struct literal or replacement request-hook implementation was found. Existing model/default/account selection is unchanged.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-core --test all` | `model_request_hooks_report_failures_and_retries_but_cannot_veto` | `codex-rs/core/tests/suite/hooks.rs:6103` @ `5cb38e79842a` | #[tokio::test]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |
| `codex-core --test all` | `async_compaction_reuses_pending_work` | `codex-rs/core/tests/suite/scenarios.rs:4504` @ `5cb38e79842a` | #[test_case::test_case(PendingCompactionOutcome::Success, CompactionTrigger::Auto; "auto uses pending result")]; #[test_case::test_case(PendingCompactionOutcome::Failure, CompactionTrigger::Auto; "auto falls back after failure")]; #[test_case::test_case(PendingCompactionOutcome::ReadyFailure, CompactionTrigger::Auto; "auto falls back after failure before checkpoint")]; #[test_case::test_case(PendingCompactionOutcome::StaleSuccess, CompactionTrigger::Auto; "auto falls back after account changes")]; #[test_case::test_case(PendingCompactionOutcome::Interrupted, CompactionTrigger::Auto; "auto can be interrupted")]; #[test_case::test_case(PendingCompactionOutcome::Absent, CompactionTrigger::Manual; "manual without background")]; #[test_case::test_case(PendingCompactionOutcome::Success, CompactionTrigger::Manual; "manual uses pending result")]; #[test_case::test_case(PendingCompactionOutcome::Failure, CompactionTrigger::Manual; "manual falls back after failure")]; #[test_case::test_case(PendingCompactionOutcome::StaleSuccess, CompactionTrigger::Manual; "manual falls back after account changes")]; #[test_case::test_case(PendingCompactionOutcome::Interrupted, CompactionTrigger::Manual; "manual can be interrupted")]; #[test_case::test_case(PendingCompactionOutcome::SuccessOverBudget, CompactionTrigger::Auto; "pending adoption still over budget")]; #[tokio::test(flavor = "multi_thread", worker_threads = 2)]; Mock native Core/provider control; skip_if_no_network and body execution must be observed. |

## e8e5108c9798 — adapt

Accept turn-scoped item lookup and hidden-review matching. Reconcile upstream tuple projection with ALab owned HistoryPageItem/bounded borrowed grouping; retain page-order, exact coverage and empty-interruption guarantees rather than treating the new scope key as complete equivalence.

Affected contracts: `history-pagination`, `native-ui`, `execution-continuity`.

Required integration behavior:

- Adapt the native Vec<(String,ThreadItemPayload)> seam into the owned grouping implementation or mechanically update the owned representation and its genuine tests. Remove redundant upstream turn-ID projection hooks once their intent is supplied natively; do not retain duplicate conversions or discard turn identity.
- Port upstream hidden_item_ids as (turn_id,item_id), including repeated user-message hiding and parked-parent pages. Preserve ALab bounded slices, page-order completion, reused IDs, partial pages and anchored itemless interruptions.
- Do not replace the owned page-order grouping solely because upstream now keys its offset map by turn. Verify all owned reversal/interleaving/coverage controls and real TUI pagination before declaring that stronger behavior superseded.

ALab interactions:

- Current app/history_completion.rs delegates to owned group_turn_items and current app_server_session/history.rs returns owned HistoryPageItem. Canonical code instead returns tuples/ranges; this is a concrete integration shape overlap, not an observed runtime regression.

Required source/coverage gaps (not closed by this review):

- No runtime equivalence or safe removal of the stronger owned grouping is established. Existing source controls must execute on the reconciled implementation.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-tui --lib` | `split_turn_only_completes_on_the_page_with_its_last_item` | `codex-rs/tui/src/app/history_completion_tests.rs:41` @ `5cb38e79842a` | #[test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `repeated_prompt_matching_removes_only_the_hidden_occurrence` | `codex-rs/tui/src/app/history_pagination_tests.rs:106` @ `5cb38e79842a` | #[test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `hidden_last_item_keeps_turn_groups_and_completion_boundaries` | `codex-rs/tui/src/app/history_pagination_tests.rs:174` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `older_page_updates_the_parked_parent_without_moving_focus` | `codex-rs/tui/src/app/tests/pagination_completion_tests.rs:128` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `reversed_turn_endings_reproduce_the_76_to_38_crash` | `codex-rs/alab-tui/glue/history_page_tests.rs:66` @ `1074b5ff72b1` | #[test]; Owned host include; actual grouping/coverage oracle, not source strings or backend pagination execution. |
| `codex-tui --lib` | `reused_item_ids_do_not_complete_another_turn` | `codex-rs/alab-tui/glue/history_page_tests.rs:80` @ `1074b5ff72b1` | #[test]; Owned host include; actual grouping/coverage oracle, not source strings or backend pagination execution. |
| `codex-tui --lib` | `interleaved_turns_complete_in_item_order_with_their_empty_interruptions` | `codex-rs/alab-tui/glue/history_page_tests.rs:91` @ `1074b5ff72b1` | #[test]; Owned host include; actual grouping/coverage oracle, not source strings or backend pagination execution. |
| `codex-tui --lib` | `metadata_permutations_and_partial_pages_preserve_exact_coverage` | `codex-rs/alab-tui/glue/history_page_tests.rs:150` @ `1074b5ff72b1` | #[test]; Owned host include; actual grouping/coverage oracle, not source strings or backend pagination execution. |

## b7056f340852 — adopt

Accept backend-scoped overview discovery, pins, refill and refresh scheduling so delayed work or a focus change cannot publish another backend's rows, notices or actions.

Affected contracts: `native-ui`, `account-isolation`, `history-pagination`.

Required integration behavior:

- Preserve stable first-observed thread ownership and bind refresh requests/results, show-more, failures, pending pin changes and queued refreshes to BackendId. Disconnect only the matching live handle; do not reset a global owner map to accept foreign events.
- Preserve explicit launch model/provider/permission choices and saved blank-session/draft state across backend selection. The overview is metadata, not native source/account admission or permission to operate another root.

ALab interactions:

- ALab running-version overview notice remains sourced from its actual running executable. Native task/account inspector events and account/source leases retain their independent owners and defaults.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-tui --lib` | `late_overview_page_only_refills_its_own_backend` | `codex-rs/tui/src/app/agents_overview_tests.rs:297` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `queued_overview_page_survives_a_focus_change` | `codex-rs/tui/src/app/agents_overview_tests.rs:352` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `overview_keeps_rows_and_routes_actions_to_the_owner` | `codex-rs/tui/src/app/agents_overview_tests.rs:393` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `overview_hides_show_more_when_only_paginated_backend_disconnects` | `codex-rs/tui/src/app/agents_overview_tests.rs:572` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `pin_completion_cancels_the_originating_backends_stale_refresh` | `codex-rs/tui/src/app/agents_overview_actions_tests.rs:218` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `foreign_backend_events_leave_thread_cache_and_connection_untouched` | `codex-rs/tui/src/app/tests/backend_event_tests.rs:125` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `replacement_uses_server_defaults_and_preserves_explicit_launch_settings` | `codex-rs/tui/src/app/tests/new_session_tests.rs:337` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |

## 8e30237f643c — adapt

Accept explicit Cloud stream-loss reasons, bounded credential-aware idle reattach and cancellation of abandoned read requests. Mechanically adapt the owned actual source-recovery control from Option outcome to the new Result outcome without weakening source/Attach/drain assertions.

Affected contracts: `provider-recovery`, `account-isolation`, `native-session`.

Required integration behavior:

- Update alab-core/glue/cloud_idle_source_tests.rs189 outcome.is_none() to the new failed Result contract, keeping native source-changed denial distinct where representable. Preserve all three cases, real storage/current credential read, exact Attach request counts 0/1/0 and sealed-resource drain.
- Retain the existing auth-bound HTTP factory and IdleNetworkPermit source adapter: idle recovery owns no permanent source activity; accepted episodes reacquire the frozen source across credential reads, Attach, backoff and response lifetime.
- Preserve immediate EOF versus quiet successful Attach, blocked RPC5/7/16 reasons, bounded transient retries, real generation/current credential rechecks and same-thread loss supersession. Reads ThreadList/ThreadTurnsList/ThreadItemsList cancel locally when the waiter drops; mutations retain accepted custody.

ALab interactions:

- Recovery Open changes Option to Result<attachment,Option<reason>>. This is a concrete owned source API adaptation; no runtime failure is claimed. Local cancellation/timeout neither rolls back a remote mutation nor guarantees future stream health.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-app-server-client --lib` | `dropping_an_in_flight_read_unblocks_the_native_worker` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs:229` @ `5cb38e79842a` | #[tokio::test]; Mock native Cloud client/worker control; local cancellation is not remote rollback. |
| `codex-app-server-client --lib` | `dropping_an_in_flight_start_unblocks_the_native_worker` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs:263` @ `5cb38e79842a` | #[tokio::test]; Mock native Cloud client/worker control; local cancellation is not remote rollback. |
| `codex-app-server-client --lib` | `successful_reopen_supersedes_only_earlier_losses_for_that_thread` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs:591` @ `5cb38e79842a` | #[tokio::test]; Mock native Cloud client/worker control; local cancellation is not remote rollback. |
| `codex-app-server-client --lib` | `accepted_idle_reconnect_reports_changed_login` | `codex-rs/app-server-client/src/cloud_backend/connection_tests.rs:634` @ `5cb38e79842a` | #[tokio::test]; Mock native Cloud client/worker control; local cancellation is not remote rollback. |
| `codex-app-server-client --lib` | `cloud_idle_recovery_reads_and_attaches_only_under_its_original_source` | `codex-rs/alab-core/glue/cloud_idle_source_tests.rs:66` @ `1074b5ff72b1` | #[tokio::test]; Owned include inside cloud_backend::idle. Adapt Result shape only, preserve actual source/credential/Attach/resource oracle. |

## b016cd184725 — adopt

Accept automatic Cloud thread titles generated with the selected Cloud account and validated backend routing, outside the conversation command queue. Preserve cancellation, manual rename ordering, expected-name checks, bounded prompt/output and first-message fallback.

Affected contracts: `native-ui`, `account-isolation`, `model-discovery`, `product-policy`.

Required integration behavior:

- Construct CloudTitleClient only after the existing auth.alab_bind_http_client_factory hook so its direct Responses transport retains native credential/source policy. Preserve restricted HTTPS allowed origins, configured-origin match, routing allowlist, no redirect/request logging and tools=[] metadata-only generation.
- Retain actual selected Cloud account, explicit session model/provider defaults and the existing separate title-model selection rule. Do not enable Cloud, plugins/apps or held SourceV1/all-root routes for review or qualification.
- Keep title read/write separate from current attachment and command queue; preserve cancellation-before-save, expected-name guard, manual rename semaphore/retry and bounded shutdown. A completed generation is not a saved-name ACK or a user action receipt.

ALab interactions:

- The canonical constructor is after the existing factory binding and native client construction; no demonstrated source-policy bypass was found. Transport source admission comes from the bound factory, while idle reconnect uses its separate episode permit.
- Expected-name comparison/manual gate controls local competing writes; no cross-client server compare-and-swap or remote rollback claim follows. Current ALab agent model default remains gpt-6.1-sol; this review changes no defaults.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-app-server-client --lib` | `generates_a_structured_title_using_the_selected_cloud_account` | `codex-rs/app-server-client/src/cloud_backend/thread_title_tests.rs:20` @ `5cb38e79842a` | #[tokio::test]; Actual mock account/routing/Responses request; not real account-service qualification. |
| `codex-cloud-client --lib` | `reads_a_saved_name_without_attaching_to_the_thread` | `codex-rs/cloud-client/src/client_tests.rs:163` @ `5cb38e79842a` | #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-cloud-client --lib` | `rename_sends_the_thread_and_title_and_requires_acknowledgment` | `codex-rs/cloud-client/src/client_tests.rs:136` @ `5cb38e79842a` | #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-tui --lib` | `bounds_the_entire_title_prompt_for_dense_unicode` | `codex-rs/tui/src/app/thread_title_tests.rs:77` @ `5cb38e79842a` | #[test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `cloud_conversations_save_the_first_message_if_generation_is_unavailable` | `codex-rs/tui/src/app/thread_title_tests.rs:96` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `automatic_thread_title_respects_origin_metadata_after_switching` | `codex-rs/tui/src/app/thread_title_tests.rs:182` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `canceled_thread_title_ignores_late_start_and_completion` | `codex-rs/tui/src/app/thread_title_tests.rs:592` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `cloud_manual_rename_can_retry_after_an_in_flight_automatic_write` | `codex-rs/tui/src/app/thread_title_tests.rs:669` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |

## 02e745f3a081 — adopt

Accept opening command center while an archive request completes under retained target/backend/result custody. Cleanup failure keeps the original session and pending exit outcomes become explicit warnings rather than fabricated archive success.

Affected contracts: `native-ui`, `execution-continuity`, `exec-handoff`, `native-session`.

Required integration behavior:

- Complete side-chat/voice cleanup before removing the current owner; preserve input capture, target subscription/approvals and backend identity until actual archive outcome. Queue the archive before refresh and retain its one-shot result across UI event-channel replacement.
- Preserve failure return/progress behavior, embedded versus persistent-server ownership, bounded pending-archive shutdown joins and redacted reconnect/stop guidance. Navigation or client exit is not server archive ACK, stopped remote work or durable native goal completion.
- Retain ALab native_event handoff invalidation before mutations and life at_exit/signal propagation in CLI exit handling. No held all-root receiver is opened by command-center navigation.

ALab interactions:

- Canonical AppExitInfo adds archive_warnings and updates its upstream constructors. No ALab-owned exhaustive AppExitInfo literal was found; ALab CLI lifecycle hooks remain around the same actual exit boundary.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-tui --lib` | `embedded_archive_completes_after_opening_command_center` | `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs:2406` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `archive_opens_command_center_before_the_server_replies` | `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs:2424` @ `5cb38e79842a` | #[tokio::test]; Body covers Cloud success and remote rejection with actual queued request/result and event-channel replacement; not real remote service execution. |
| `codex-tui --lib` | `archive_shows_progress_and_keeps_the_session_when_side_chat_cleanup_fails` | `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs:2556` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `lifecycle_actions_stop_parked_voice_before_removing_owner` | `codex-rs/tui/src/app/tests/realtime_start.rs:152` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |
| `codex-tui --lib` | `daemon_disconnect_exit_summary_includes_reconnect_and_stop_instructions` | `codex-rs/tui/src/app/tests/background_exit_tests.rs:221` @ `5cb38e79842a` | #[tokio::test]; TUI state/consumer control; not remote execution or backend authority. |

## ee344ca94583 — adopt

Accept clarified history guidance to discover opaque window IDs through window listing before item queries; returned IDs stay opaque and the tools remain private read-only eventually consistent model history.

Affected contracts: `history-pagination`, `native-session`.

Required integration behavior:

- Preserve normalized persisted ordinal/creation ordering, current-agent/default and explicit other-agent naming semantics. Prompt guidance does not expand backend read scope, grant source authority or make stored history a delivery/goal receipt.

ALab interactions:

- Only namespace description and generated tool-contract snapshots change; no history query or permission implementation is changed.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-history-notes-extension --lib` | `bundled_tool_contracts` | `codex-rs/ext/history-notes/src/tools_tests.rs:195` @ `5cb38e79842a` | #[test]; Tool specification snapshot, not runtime DB/query/permission execution. |

## 5cb38e79842a — adopt

Accept a single effective config-layer stack for config/read, including relative cwd resolution and external edits, while preserving requirements enforcement, validation-error precedence and packaged-default origin filtering.

Affected contracts: `account-isolation`, `product-policy`, `native-session`.

Required integration behavior:

- Use load_latest_config to resolve request cwd once; derive config/read response from that exact stack. Preserve layer/decode errors before full-config validation errors and preserve requirements/applied feature reporting.
- Retain ALab compiled defaults, explicit user model/provider/account/workspace choices and hidden packaged-default origins. Read-only config reads must not become credential/source selection, settings mutation or held native receiver admission.

ALab interactions:

- ALab packaged defaults enter the existing loader before response projection; no direct owned ConfigManager API caller or exhaustive response literal was found.
- Observed1074→d725 main drift changes only startup_account_read test coverage and its review; no production config reader change. It does not silently rebind this fixed1074 comparison or qualify the new relative-cwd path.

Source-declared candidates, all **not_run**. Owned includes compile in the named host package, not their physical alab-* directory. Macro attributes are retained as declaration data.

| Package / target | Filter | Declaration | Source annotations / limits |
| --- | --- | --- | --- |
| `codex-app-server --test all` | `config_read_with_relative_cwd_picks_up_external_edits` | `codex-rs/app-server/tests/suite/v2/config_rpc.rs:858` @ `5cb38e79842a` | #[tokio::test]; Source declaration only; actual body execution is not_run. |
| `codex-app-server --test all` | `config_read_returns_effective_and_layers` | `codex-rs/app-server/tests/suite/v2/config_rpc.rs:885` @ `5cb38e79842a` | #[tokio::test(flavor = "multi_thread", worker_threads = 2)]; Source declaration only; actual body execution is not_run. |

## Source verification and limits

The maintained snapshot/validator checks the exact range and complete decision set. The active-record source census covers 154 pending commit declarations from the actual `ce3031018ef1` frontier through this target in 7 active records, without missing, duplicate or invalid rows; this includes the new draft, not runtime/admission evidence. Static declaration checks bind each selected source blob, actual package Cargo metadata and target root. Canonical patch bytes and the fixed catalog/defaults/owned interaction paths were read; generated snapshots are data, not execution. No actual merge, proof, tests or source adaptation is performed. Prior 52-risk notes are only a pointer: their 12 selected patches were noncontiguous and are not substituted for this formal contiguous12 review.

Required gaps remain explicit in the corresponding intake evidence and rows. Inherited81 Err/EOF outcome custody and actual budget-stop/retained-native-goal composition are not closed by general hook, lifecycle or component source declarations. Stronger remote atomicity/end-to-end service claims are not silently required or inferred. Root review followed by independent peer review and exact-source native qualification remain separate.
