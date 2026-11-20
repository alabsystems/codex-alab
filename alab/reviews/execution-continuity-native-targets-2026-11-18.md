# Execution continuity on the 0.170 candidate

On `aeb6e36ed78b7ef5c4c55916bb0cfa091d84e2fa`, the candidate CLI passed
the pending-parent, sample and fork scenario and both cold-history controls.
Scenario 5 remains pending. Two kernel restrictions prevented its end-to-end
tests from passing, and two clauses also need stronger test inputs.
[Feature contract](../features.toml)

## Scenario 6

The `tool-recovery-cli` stage runs the candidate's
`codex-cli --test alab_tool_result_recovery` binary using the selected
Trust toolchain. The named tests launch the real candidate app-server and
inspect requests sent to a loopback model. The parent holds an actual
dynamic tool request open while the sample and fork each receive the
unknown-outcome history. The parent then receives the original result.
The two cold-restart checks preserve distinct recorded and missing tool
results in legacy and paginated history. All three exact names are required.
[CLI integration tests](../../codex-rs/alab-core/glue/tool_result_recovery_integration.rs)

```text
a_sample_and_fork_report_an_unknown_outcome_while_the_parent_receives_the_result
cold_restart_keeps_recorded_and_unknown_outcomes_in_legacy_history
cold_restart_keeps_recorded_and_unknown_outcomes_in_paginated_history
```

## Scenario 5

These observations describe test assertions at the frozen revision. The
core target is `codex-core --test alab_shell_output`. The CLI recovery
target contains the following tests under its `durable_shell_logs` module.
[Core shell-output tests][core] · [CLI recovery tests][cli]

| Test | Exact function name |
| --- | --- |
| Core surfaces | `clipped_shell_output_is_recoverable_through_both_tool_surfaces` |
| Core capacity | `deleting_one_consumed_log_makes_room_for_the_next_build` |
| CLI retained build | `resumed_shell_reads_and_consumes_its_original_build_diagnostic` |
| CLI invocation binding | `resumed_input_observes_the_original_invocation_and_only_reaches_its_bound_shell` |
| CLI first terminal result | `initial_terminal_result_recovers_stderr_from_direct_and_exec_server_shells` |
| CLI interrupted publication | `hard_kill_after_publication_recovers_the_unattached_diagnostic_once` |
| CLI terminal poll | `terminal_poll_recovers_stderr_from_the_original_direct_and_exec_server_shells` |

| Contract observation | What the fixture measures |
| --- | --- |
| Clipped text and Code Mode, launch and poll, partial capture at the limit | Core surfaces compares the original buried bytes and path across launch and poll. The Code Mode check reads the path, recognizes a partial capture, and measures the file at 8 MiB. Core capacity removes a consumed file, retries the waiting shell and compares every surviving file. |
| Compiler emits 8 MiB prefix, known omission and exact last 4 KiB in a replacement CLI | CLI invocation binding builds a synthetic byte stream with a 128 KiB omission, observes the actual process exit, compares the retained prefix with an independent file and decodes the final interval from the resumed request and stale poll. The command reads prepared bytes with `cat` and exits with the fixture's requested status. A real compiler emitting this stream still needs a witness. |
| One-shot and Code Mode disclose the same range, conditional on failing status and EOF | CLI invocation binding checks exact one-shot offsets and UTF-8 content. Core surfaces independently checks precise offsets and content from its own failed Code Mode command and no tail from its successful command. The streams, offsets and payloads differ between those tests. One common input whose interval is compared across surfaces still needs a witness. The core library's oversized-stream tests additionally check unknown/successful exits, authentic nonzero exit and EOF. [Core library coverage][unit] |
| Same-thread continuation of completed diagnostic, consumption and deletion | CLI retained build reopens the same thread after graceful shutdown. A real shell reads and removes its retained file. Removing a fork and then the owner retires only their respective files. |
| New waiting shell and old invocation paired with its new session number | CLI invocation binding issues old, unbound and correctly bound writes against the same new live number. It observes the historical status/path and reads the exact accepted bytes written by the new shell. |
| Unpolled exit between turns, recovered exit and original diagnostic | CLI invocation binding waits for the original native command completion, checks the exit and original log in a replacement, and verifies the original execution marker occurred once. |
| Rejection, interrupted response, accepted follow-up, second replacement, history integrity | CLI invocation binding observes the failed first resumed turn and both requests in the interrupted/accepted turn. It verifies the four uncollected exits, compares the text of eight previous tool results exactly and observes an empty summary after the next replacement. |
| Identical commands in different working directories | CLI invocation binding launches the same shell script in four directories and checks the command, canonical cwd, distinct binding, session number and exit code in each resumed request. |
| Quiet build first emits a late diagnostic and fails, replacement reads it without rerun | CLI invocation binding confirms the original running tool returned empty output, releases a FIFO, waits for the original native exit and removes the source. The replacement copies the retained bytes, including NUL and non-UTF-8, and the launch marker is counted once. |
| First tool call returns terminal failure before helper emits stderr, direct and local exec-server modes | CLI first terminal result verifies the early exit and lack of running handle or diagnostic. The real helper emits later. The first replacement receives the diagnostic, the second performs the read, and its requests contain no repeat summary. |
| Visible file before SQLite attachment, abrupt termination and two replacements | CLI interrupted publication injects an attachment failure. It checks the durable intent, length, SHA-256 and exact visible bytes before killing the original process. Only the first replacement receives the recovered summary. The second reads the same bytes, and the fixture shell's execution marker occurs once. This command also emits synthetic diagnostic bytes. |
| Poll returns while helper holds stderr, release then recover without regaining an input handle | CLI terminal poll checks the original exit, empty final tool output and refused stale write. It releases the helper after the poll, waits for EOF, replaces the CLI, copies the original late bytes and verifies the cold write was refused. |

The protected core run returned status 101. Both tests reached the product's
shell-output API. The first failed to disclose its capture path and the
second reported unavailable storage for its first clipped command.
The store creates a private, per-thread directory immediately under
`/private/tmp`. Qualification grants writes to its own temporary directory.
A broad write grant would also reach other sessions' captures.
[Store naming](../../codex-rs/alab-core/src/shell_log_store.rs) · [Protected profile](../lib.sh)

The eight-test CLI run returned status 101 with three passes and five
failures. Its workspace-write shells also encountered
`sandbox-exec: sandbox_apply: Operation not permitted` inside the outer
qualification profile. Granting a preallocated capture path would leave
this second restriction in place. The normal fork allocates its own child
ID. A future executor must preserve both policies and exercise the actual
fork's independent capture lifetime. [CLI retained build test][cli]

## Scenario 7

The earlier native run on `5f481a46172f8e627ddc5b225edcc72ac06cc343`
replayed the first captured environment after cancelling the second
validation and observed a fresh shell after the owner released the snapshot.
The new selector distinguishes this strengthened body from older revisions
that checked only the filenames. The reconciled source needs to execute
the new name before whole-feature admission.
[Earlier native receipt](execution-continuity-snapshot-replay-2026-11-18.md) · [Snapshot fixture](../../codex-rs/alab-core/glue/shell_snapshot_tests.rs)

```text
shell_snapshot::alab_snapshot_tests::canceling_validation_preserves_the_first_captured_value_until_its_owner_releases_it
```

## Protected execution records

Each native run selected the unchanged aeb source and Trust 15698 through
`alab/feature_recipes/run.sh outer`. The public CLI producer is
`cfa9ad99ebf87dc6652360df9593213933d29c44`. The separate shell-output
experiment used producer `0c1f89ce66941f676e393c0b38e42f87964bfc22`.
These SHA-256 values identify the complete command logs.

| Selection | Command status and observation | Log SHA-256 |
| --- | --- | --- |
| Public CLI stage, the three scenario 6 names above | 0. Three passed, none ignored. The production evidence reader confirmed all three. | `91adc27f813e7b6273f37e4efea18a0f81a753c5b17b618631c906227ea6dd1c` |
| Public CLI stage, the valid fork test plus `unavailable_selector_control` | 1. The real fork passed. The focused results reader refused the missing exact name. | `5a3bb6885ed709dde7d1deb190a58619a4125c8044ce459794414616ca97f105` |
| Experimental core integration stage, both shell-output tests | 101. Both failed at their expected capture paths. | `fbdd9c7ea262f2f1c906a7b6c5fed69a331192757fdbe8f6154d3d626a7d8d5d` |
| Experimental CLI stage, all eight tests | 101. Three history tests passed and five durable shell tests failed. | `1b5030de2ec59c765143c74a257bd46dcd1262603602e2f1657c5fb0b87ca26a` |
| Core library, both snapshot names and three scratch-backed shell-output controls | 0. Four passed. The production evidence reader accepted the old snapshot and the three shell controls, and refused the strengthened selector against these same bytes. | `fd8819393e53395ae6f0ab50520849407d1a0c928b7310ebc103aa18d480ebda` |

The three passing shell-output controls exercise slot availability, copying
every byte across the in-memory prefix boundary, and retrieving buffered
bytes when a slot reopens. Their exact names share the
`unified_exec::shell_output::tests` module. [Core library controls][unit]

```text
small_running_commands_leave_slots_for_clipped_output
crossing_the_prefix_limit_writes_every_byte_once
running_commands_recover_their_retained_bytes_after_slots_reopen
```

[core]: ../../codex-rs/alab-core/glue/shell_output_integration.rs
[cli]: ../../codex-rs/alab-core/glue/tool_result_recovery_integration.rs
[unit]: ../../codex-rs/alab-core/glue/shell_output_tests.rs
