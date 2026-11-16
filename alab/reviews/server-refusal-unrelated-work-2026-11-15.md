<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Receiving later work after a server refusal

The reported failure occurred on 0.8 during an active goal. A terminal server
refusal must finish its own turn without stopping unrelated work or preventing
later server messages from being received.

At the 0.8 release-ledger commit `97b701fa49`, the goal extension routes policy
errors through `ActiveGoalStopReason::TurnError`, which blocks the active goal.
The 0.9 release-ledger commit `73d34a6698` includes the
`ALAB(policy-recovery)` branch in `codex-rs/ext/goal/src/extension.rs`: it accounts
progress and finishes the failed turn while keeping the goal eligible for its
ordinary idle continuation. These are source comparisons, not measurements of
the binary installed in the reporting tab.

The existing native integration test covered a refusal followed by one successful
response. The strengthened regression checks the typed terminal refusal, two later
automatic turns receiving successful responses, and a paused goal remaining paused.
A separate regression checks that a second terminal refusal in one root does not
prevent an unrelated root from starting and receiving its successful response.
Both use a real Core thread and a loopback Responses server with the production
goal extension; aterm control is supplied by the existing in-process fixture.

No new retry policy, server-error classification, prompt, model selection or account
selection is introduced by these regression changes. The existing implementation
already supplies the relevant behavior on current main.

## Validation

On source based on `257042ffca`, the extended goal regression passed. The final
`policy_` integration group passed five tests and the `daybreak_` group passed
three, with one overlap: seven unique passing tests and no remaining failure.
The first group run exposed an extra unused scripted response in the modified
fixture; the fixture now adds that response only for the unrelated-root case.

Commands (from `codex-rs/`):

```sh
targo --unverified test -p codex-alab-autopilot --test autopilot policy_ -- --nocapture
targo --unverified test -p codex-alab-autopilot --test autopilot daybreak_ -- --nocapture
```

The atpkg PATH Trust toolchain (`27009.noindex`) ran these tests in the unverified
execution lane. This is a test result, not certified application-build or full-gate
qualification. Existing deprecation warnings in diagnostics, Core and test support,
and the linker's compact-unwind size warning, remain unchanged.

## Follow-up: deterministic pause ordering

Review of the landed regression found that its 500 ms response delay was not a
synchronization guarantee: a sufficiently slow goal write could let the response
finish before the pause. Replace the delay with a test-only turn-start gate. The
third automatic turn signals that it has been admitted, the test pauses future
goal scheduling, and an explicit release lets that turn reach the server. This
preserves the same behavioral assertions without depending on machine speed.

The gated regression passed with the same unverified test command and PATH Trust
toolchain described above (`a_daybreak_refusal_keeps_receiving_later_independent_goal_turns`:
one passed, zero failed). The response delay is removed; the 60-second timeout
bounds a missing lifecycle event rather than controlling operation ordering.

## Follow-up: HTTP error-body coverage

The integration scenario originally supplied only an SSE `response.failed` event.
`codex-api` also maps an HTTP 400 body carrying `error.code=cyber_policy` to the
same typed turn error. Exercise that transport independently through the same
native goal scenario to guard the entire receive/classify/finish/continue path,
without duplicating the test or adding a client-side text classifier.

Both transport tests passed with `targo --unverified test -p codex-alab-autopilot
--test autopilot receiving_later_independent_goal_turns -- --nocapture`: two passed,
zero failed, using the PATH Trust toolchain described above.

The neighboring `goal_recovery::` integration group also passed all seven tests,
including already-blocked goals, pauses during compaction and provider pacing.

## Follow-up: goal identity and budget

Review found that the integration test's `expect("same goal")` only checked that a
goal existed; its assertions checked status alone. A replacement goal with a reset
budget could therefore pass. The scenario now creates a goal with a finite token
budget and retains its returned identity. After recovery and again after the
explicit pause/in-flight completion, it asserts the same goal ID, objective and
token budget as well as status.

Both SSE and HTTP transport scenarios passed with these stronger assertions:
`receiving_later_independent_goal_turns` selected two tests, both passed under the
same unverified PATH Trust execution lane. This change adds assertions only.

The full native Autopilot integration suite on landed source `1412fc16f1` passed:
163 passed, zero failed or ignored, in 196.82 seconds, using
`targo --unverified test -p codex-alab-autopilot --test autopilot -- --test-threads=4`.
That run began before the identity assertions were edited. The two affected
transport scenarios were separately rebuilt and passed with the new assertions;
no production code changed between those runs.
