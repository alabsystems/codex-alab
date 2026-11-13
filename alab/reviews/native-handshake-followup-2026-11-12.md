<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Native startup replay and observed handshake timing

## Decision and source

Resume the standalone startup check deferred by another session's timed machine
reservation, then run the complete native TUI suite with the newly landed
handshake observations. Keep the source fixed so a recurrence can be attributed
to its recorded binary. Do not change deadlines or initialization order on the
basis of the earlier timeout alone.

Both checks ran at `ecf11e2e44e50dab2cbcdbdbfaca1355b5c4815c`, with a clean
worktree and the Trust unverified lane, `--locked --offline`, and an 8 MiB test
stack. The standalone startup/exec/DA1 case passed in 0.30 seconds. This closes
the previously deferred replay; the reservation was not bypassed.

## Complete run and separate retry

The four-thread native TUI run finished in 1,066.26 seconds: 6,385 passed, one
failed, and six ignored. All native upgrade children and both changed native
TUI PTY cases passed. The earlier launch-handshake timeout did not recur in
this run. That is not proof that its cause has been repaired.

The one failure was
`local_daemon_registers_approval_gated_mcp_tools_for_both_start_paths`, at
`tui/src/app/tests/session_lifecycle_requests.rs:2584`. Its five-second
`tokio::select!` chose the timeout arm while waiting for task registration.
No readiness snapshot was captured, so this does not establish that the other
arms were unready at exactly the deadline.

The unchanged test then passed alone in 9.16 seconds. Its independent
120-second process bound did not expire. Keep this passing retry distinct from
the failing full run; neither identifies the cause. The test scenario and MCP
bridge match imported upstream `d9a32c1789`, but their execution still traverses ALab Core
startup and assignment hooks. Identical test text does not establish equivalent
execution or justify assigning the failure to upstream.

## What the handshake observations show

The first connection in each active native fixture recorded these intervals:

| Case | Accept to authentication observed | Version received to write finished |
| --- | ---: | ---: |
| Delivered | 2.693363 s | 16 microseconds |
| Held | 2.964917 s | 12 microseconds |
| Target refusal | 2.297051 s | 15 microseconds |

All writes succeeded. Each active case retained 20 events with none dropped;
both inactive controls retained empty traces. These observations locate the
long interval before authentication is observed. They cannot distinguish a
delayed client write from delayed server task execution or reading.

`Supervisor::start` first runs from the thread lifecycle hook inside
`native.start_thread()`. Creating the embedded app server alone does not start
that probe. Moving the fixture's observer before `start_thread()` would wait
for a probe that has not started. Keep the existing ordering until a stronger
measurement establishes a valid change.

If the handshake timeout recurs, the next useful measurement is paired child
bootstrap and host markers on the same monotonic clock, without configuration
or wire contents. Current per-process elapsed origins cannot establish exact
cross-process overlap. Such overlap would still not, by itself, prove executor
starvation. No protocol, deadline, scheduling, or production behavior changes
were made for this follow-up.

## Retained evidence

Under `$HOME/.aterm/.ubd/`:

- `codex-startup-replay-resume-20261112.log`: completed startup replay.
- `codex-handshake-full-followup-20261112.log`: full run, including the failure.
- `codex-handshake-full-followup-source-20261112.json`: source, tree, binary and
  log hashes, clean-tree observation, command, and final counts.
- `codex-mcp-registration-failure-excerpt-20261112.txt`: normalized failure.
- `codex-mcp-registration-replay-20261112.log` and adjacent JSON: separate retry
  and its process exit/timeout result.
- `codex-handshake-followup-observations-20261112.json` and
  `codex-handshake-followup-witnesses-20261112/`: bounded metadata snapshots and
  non-credential fixture evidence, retained beyond temporary-directory cleanup.

These are scoped execution results, not a release gate or compiler proof.
