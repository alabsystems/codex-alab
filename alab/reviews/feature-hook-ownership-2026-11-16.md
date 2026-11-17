<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Account for three existing hook contracts

The maintained committed-tree inventory check rejects `extension`, `model-stop`
and `shutdown` as unowned at composed source `fd63114f0ade` and at both parents,
`d8ce7b23036e` and main `d0bb07e59b8f`. This is an inherited catalog omission.
The validator and hook parser are identical at those three sources. Preserve
their checks and the actual hook markers; assign each topic to one semantic
owner instead.

| Existing topic | Feature owner | Actual source responsibility |
| --- | --- | --- |
| `extension` (2 hooks) | `goal-storage-recovery` | The only `RunExtension` producers enqueue committed goal set/clear output through the existing thread listener, ordering response enqueue before the matching notification. |
| `model-stop` (1 hook) | `native-session` | Recheck the captured automatic source after the instruction await and immediately before creating its SQLite mutation. |
| `shutdown` (7 hooks in staging) | `native-session` | Close terminal submission admission and drop queued reply owners before extension retirement; preserve child-result draining and a live session after nonterminal ordinary suspension. |

List the corresponding owned `unloaded_goal_rollout.rs`, `goal_dispatch.rs` and
`assignment_lifecycle.rs` implementations. Add three explicit invariants and
scenarios so these requirements are visible to final evidence admission. They
are required contracts, not observations that the scenarios have passed. The
staged catalog changes from 83 to 86 scenarios across 14 features; the exact
main parent lacks three staged exec-handoff scenarios and the gzip/Zstd scenario,
so its corresponding change is from 79 to 82. Source bindings
and descriptions in older evidence records cannot be carried forward implicitly.
No runtime, hook, default, validator, permission or held receiver route changes.

The ordinary-suspension contract is explicitly limited to preterminal failures:
cleanup can fail after shutdown has already been requested and admission closed.
That later failure must not reopen the terminal session. Accepted child-result
draining is the tree-suspension boundary, not an added ordinary-shutdown join.

## Declared controls and required gaps

All controls below are **UNRUN** for this change. Names are source-declared
filters, with eventual compiled discovery and actual body execution required.

Goal output (`codex-app-server --lib`, owned `unloaded_goal_rollout.rs`):
`goal_output_queue_is_fifo_per_thread_without_global_backpressure` and
`goal_output_queue_rejects_a_thread_before_its_backlog_can_grow_unbounded` cover
the real queue's per-thread FIFO, independent-thread progress and per-thread
reservation rejection/release. They do not execute the whole `RunExtension`
listener delivery/fallback, response-notification ordering under observer or
listener cancellation, or global 128-reservation saturation. Those are required
coverage gaps. Awaited response enqueue does not mean client acknowledgment.

Model source (`codex-alab-autopilot --lib`, owned
`src/model_blocked_source_tests.rs`):
`a_host_round_trip_defeats_a_prepared_model_block_resume`,
`a_mode_round_trip_defeats_a_prepared_model_block_resume` and
`a_fresh_model_block_after_host_activity_establishes_new_epoch_authority` use
actual persisted goal, native turn and provider-request oracles. Their gates
precede submission; revocation during the instruction await, reaching the final
pre-SQL hook, remains a required gap. A task-local dispatch predicate test alone
cannot qualify that timing.

Terminal teardown has real component controls:
`codex-core --lib` filter
`terminal_dispatch_drops_channel_and_assignment_queued_reply_owners`, and
`codex-alab-autopilot --lib` filter
`a_failed_goal_read_holds_native_submission_and_owes_a_recheck`. These cover
actual reply-owner drops and a queued automatic input's native shutdown.
`codex-core --test all` filters
`graceful_suspend_preserves_a_finishing_child_result` (eight source labels),
`root_turn_suspension_preserves_unfinished_turn_history` (two labels) and
`tree_shutdown_waits_for_children_without_affecting_other_roots` cover accepted
child-result/history custody and tree isolation. The owned
`ordinary_suspension_ignores_dropped_and_independently_live_api_managers` control
covers successful ordinary suspension. A later child-writer failure following
successful root suspension is not a failed nonterminal suspension. Actual
channel-close teardown with a queued goal owner and NotActive, UnsupportedTask
or preterminal persistence failure followed by accepted live work remain required
coverage gaps.

The final feature binder must reject uncovered required clauses, including these
gaps and the separately recorded old stop-policy non-replay gap. Assigning topic
ownership or locating a test declaration does not establish semantic coverage,
runtime success, native admission, compiler adoption or application certification.
