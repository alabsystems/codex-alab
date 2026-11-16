<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Observe the original fixture processes after guardian loss

The final composition run at `a2a4093bfc9fdb6027f41c8f874cccaab0b236ae`
executed six lifeline tests: five passed and the receipt BEFORE case errored.
Its post-guardian-wait `killpg(pgid, 0)` probe returned EPERM. That observation
does not establish whether reuse, zombies or another condition caused the
refusal. After the custodian's lease ends, a numeric PGID is insufficient to
identify the original processes even when a lookup succeeds.

The original failed log is
`<evidence>/codex-widest-last-lifeline.log`. Its retained 22-file record is
`.git/alab-validation/a2a4093bfc9fdb6027f41c8f874cccaab0b236ae/lifeline-observer-failure-c2e5jmjm/`,
with index SHA-256
`298acdf7f28b99bc64968656bcf938549e84be2bdd45064f81f7e67c9ff94888`.
Earlier `6bad` and `5f6` successes retain their original source and scope; none
is rewritten into this repaired observation.

Decision: fix the test's identity boundary. The production guardian, receipt,
custodian, profile and deadlines remain unchanged. A test-local exit watcher
registers kqueue NOTE_EXIT on macOS, or opens a pollable pidfd on Linux. Each
original command and writer first publishes its PID, then remains held. The
parent arms both native handles before releasing a watch-armed marker. Both
originals must acknowledge afterward and their watches must still be pending.
An already dead/recycled PID cannot supply that second acknowledgement.

Both receipt controls and both existing cache signal controls use those stable
handles. AFTER requires both original exit notifications immediately at the
actual `run_recorded` return gate. BEFORE waits for the original exits after
its owned guardian is killed. The cache tests still require the original flock
to be unavailable before the signal, then require both original exits when the
lock becomes available; lock inode, mode and contents stay unchanged.

All post-retirement PID/PGID existence probes are removed from this test module,
including failure cleanup. ExitStack closes every native watch. Registration
failures, including ESRCH/EPERM, unexpected events and unavailable mechanisms
fail explicitly; none is interpreted as process absence or bypassed. Cleanup
uses private cooperative markers and only the still-owned guardian Popen for
destructive signals. The watcher records known process exit, not its raw wait
status, reaping, exhaustive group absence or detached descendants.

Linux's [pidfd_open manual](https://man7.org/linux/man-pages/man2/pidfd_open.2.html)
documents readable exit and hangup-after-reap notifications. This Mac exercised
the actual kqueue branch; no Linux execution is claimed. Unsupported native
watch mechanisms remain an honest failed capability requirement.

On isolated base `6bad0fa139237e6e8d368b2354b320f87b07d803` plus this test-only
repair, managed Python `-I -B` passes all 33 `test_command_guard*.py` tests in
1.424s and all 14 receipt/result tests in 0.672s. Both focused test invocations
exited zero with no skips or errors. The former includes four actual signal
scenarios with prearmed kqueue watches. Raw guardian `-9` remains distinct from
supervised-command unknown/normalized-zero outcomes; receipt contents retain
their empty/completed distinction. Python syntax and `git diff --check` pass.
Logs are `<evidence>/codex-guardian-exit-watch-tests.log` and
`<evidence>/codex-guardian-exit-watch-results.log`.
Root and an independent read-only reviewer approved the exact repair, including
registration failure, acknowledgement, return ordering and cleanup boundaries.

These are local fixture-process observations. No guardian production change,
application build, certificate, installation or new full-workflow result is
established by this repair.
