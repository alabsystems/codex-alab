<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve failure and cancellation during observer fixture cleanup

Hostile review of landed `0004168404..1b384cf8e6` found two test-only defects.
Both lifeline cleanup blocks wrote their cooperative stop marker before every
other process cleanup action. A failed or interrupted write therefore skipped
the owned guardian signal/wait and original-process exit waits. Separately, an
exit-watcher registration error followed by close failure raised the close
error instead of the registration error. Python retained the earlier error as
context, but its identity and cancellation precedence changed. No successful
path false exit observation or production guardian defect was established.

Decision: keep the repair local to the existing lifeline test module. Cleanup
attempts the stop marker, owned guardian signal, owned guardian wait, each
original-process exit wait and every registered watcher close independently.
Each existing wait retains its one-minute bound. Close ownership is cleared
before the native close and is never retried against a possibly reused FD.
Failed cleanup remains an error; it never fabricates a retirement observation.

Both fixture bodies explicitly capture their own primary error. Reading ambient
`sys.exc_info()` in a finally block could otherwise inherit a caller's handled
exception and suppress a cleanup-only failure after a successful fixture body.
Error selection follows the repository's request-recovery cleanup convention:
an already pending body cancellation retains its identity; otherwise the first
cleanup cancellation wins over ordinary errors. Ordinary cleanup errors retain
an ordinary body error, or the first cleanup error when the body succeeded.
Secondary errors and a superseded body error remain attached as notes. The
constructor applies the same policy to registration and close errors.

The existing birth, native-handle registration, private acknowledgement and
pending-watch checks remain unchanged. Signals target only the owned Popen;
there are no numeric PID/PGID existence probes, privileged retries or new
fallbacks. Actual macOS kqueue observations retain their known-command/writer
scope. Simulated pidfd controls do not establish Linux execution. Guardian,
custodian, receipt, profiles and production deadlines are unchanged.

On base `1b384cf8e6` plus this test-only repair, the final managed Python `-I -B`
regression invocation fully exited zero: all 47 existing command-guard,
lifeline, post-fork and receipt/result tests passed in 1.414s with no skips or
errors. Its original handle was `28260`; the log is
`<evidence>/codex-guardian-cleanup-regressions-final.log`. Syntax and
`git diff --check` pass. Root and an independent reader found no remaining
source-review blocker after the body-local and cancellation refinements.

The independent author added 13 fault-test methods using only the existing
OriginalProcessExit and public fixture methods. A private module override loaded
the exact committed `1b384cf8e6` implementation for the counterfactual: it exited
one with 12 meaningful failing subtest assertions, zero errors/skips, in 0.688s.
The same controls against the repair exited zero: all 13 passed in 0.660s. Six
baseline fault scenarios recorded skipped waits/retirement before compensation;
all eight fixed real-process scenarios recorded guardian waits, both original
exits, both watcher attempts and exactly one close per watcher. Independent
compensation completed before either test invocation ended. Simulated kernel
errors also verify refusal, event identity, partial ownership and close failure;
real controls verify body/cleanup cancellation and an unrelated caller exception.

Logs are `<evidence>/codex-guardian-observer-errors-baseline.log` (SHA-256
`7da29699590eb4469a78365afbde838747b8dcde804608819c5415231b831924`)
and `<evidence>/codex-guardian-observer-errors-current.log` (SHA-256
`c1585670f30a57c5ba70225e85a3a852a1a76aca92fd178a79e26c1b77a3cb79`).
The frozen new-test SHA-256 is
`824a229fefd794396c6b6534658db579fcf0b26c4b0ba93638ab76e014eb190e`;
the tested lifeline module SHA-256 is
`2641aba09e3bfb2046241792aa243a828a6e02821cba320c362bc5f4abea437b`.
Root verified and archived nine evidence files under
`.git/alab-validation/1b384cf8e61d41cb474ab0ca114b915cce483ade/guardian-hostile-review-h6kthvyt/`,
with index SHA-256
`eb0e8ed4d818c5852902dd669b8a3f3f72e335e3dc1ec32660c4c17e0662a2a5`.
The archive includes the actual committed baseline, frozen sources, private
runner and failing/passing logs; the initial draft's earlier regression log
retains its separate source scope.
Independent read-only review found no blocker in either file. The two final
regression invocations establish 60 passing test methods; they do not establish
an application build, certificate, installation or broad workflow result.

Root then tested the integrated source at
`3cb7b30c99a0fa976e09107018458653e0a5e8ff` in one ordinary-capture suite.
Discovery asserted 60 unique test IDs before execution; all 60 passed in
2.075s with no errors or skips. The original command handle `28137` fully
exited zero. Its source-bound log and scope metadata are retained under
`.git/alab-validation/3cb7b30c99a0fa976e09107018458653e0a5e8ff/hostile-review-composition-03afjjon/`,
with index SHA-256
`33a03fac4e03315c9e6c853e9e648b298ab47dd0f14b5a378931b18bec9b01ef`.
This composition confirms the repaired fixture and new controls coexist with
the existing guardian and receipt/result tests. No production build input
changed, so the earlier recorded application proof-transport refusal remains
deferred; it is not replaced by a full-gate success claim.
