<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve renewal drive outcomes through host cleanup

Decision: own the constructor-returned host before driving it, retain the selected
drive error through every cleanup phase, and attempt independent safe evidence
work. Reuse the approved log owner under the deliberate public name `OutputLogs`;
the shared runner uses that same name and no obsolete private alias remains.

The baseline audit used actual public main on candidate
`5781393531c2a9a9b6070e0a7e8ddd6dc3ac6e1c`, with the existing inert tail fixture,
real file-only configuration, inert listeners/admission records and an owned
fake Popen. It interrupted the actual nested drive at select, without extracting
or copying that function. Of twelve finite controls, the healthy-cleanup control
retained the same KeyboardInterrupt and host-wait export. Nine stop/wait/diagnostic/
terminate/pipe/export faults replaced that cancellation with an ordinary failed
run and lost its interruption marker. A wait that published returncode zero
before raising still skipped pipe close and host-wait export. A pipe close error
after actual close skipped the independent export. A log close zero replaced the
API primary; the original CLI still refused it with exit one, so no CLI-zero pass
is claimed. Listener cleanup and request traces still completed in every control.
The private observations are in `renewal-host-cleanup-audit-20261116-e96eh_v7`.

Two small private helpers collect host failures and preserve an active primary
using the maintained safe labels/notes. No active primary means cleanup failure
prevents success using the existing ordinary/genuine-control/zero-exit policy.
Drive's own stop publication now retains its local primary. The host block stores
the Popen result inside its guard before drive or another statement can fail.
Both regular log files keep their exclusive names, objects and output behavior
through the shared owner; log close cannot replace the selected host outcome.

The natural wait remains `HOST_EXIT_SECONDS` (15 seconds in production). A timeout
still marks the run stalled and invokes diagnostics with the original
`started + guardian_seconds - monotonic() - 20` budget. Diagnostics failure no
longer skips the independent owned terminate and, if stop was accepted, the
original ten-second post-terminate wait. ProcessLookupError remains compatible
with that wait; genuine terminate refusal permits none. Already-published status
is rechecked after errors and avoids unnecessary waits or termination.

Cleanup has at most one explicit cleanup status poll, including the existing
diagnostic helper's observation. Attempting diagnostics reserves it because that
helper calls the owned Popen's poll method. If diagnostics fails
before observation, cleanup conservatively retains uncertainty without a second
poll. Non-timeout error paths can use the one explicit cleanup poll. Drive's
existing observations, commands, oracle bodies, native authority and outer
guardian boundaries are unchanged; no raw PID/group signal, kill escalation,
communicate fallback, optional timeout override or supervisor is added.
Popen wait/terminate retain their standard synchronization and PID-reuse-protecting
internal behavior, including internal polls. Neither this caller observation budget
nor the inert API counters bounds those internals or proves native OS retirement.

Only confirmed process status permits buffered stdout close. Unknown retirement
is an explicit failure and skips that potentially blocking close. Host-wait export
gets an independent attempt even if safe pipe close failed. The normal record
retains exactly pid, returncode and stalled; failed cleanup records add diagnostics,
and unknown status explicitly carries `retirementConfirmed: false` and
`stdoutCloseSkipped`. None of these fields asserts complete release or native
custody. No returned constructor object means no invented host-wait record.

Twelve new methods reuse the tail fixture through a module alias, so its test IDs
are not duplicated. They run actual main/configuration/drive/host-finally/tail with
real owned regular handles and inert Popen behavior. They cover every phase,
published status after wait errors, single-poll diagnostic reservation, refused
termination, returned-constructor interruption, zero-exit and hostile diagnostics,
normal host record/command/handle behavior and actual oracle refusal. A fixture
driver-end seam reaches clean host cleanup while the unchanged case oracle still
rejects absent native observations; this is not a passing native renewal.

All 90 selected methods pass without skips on managed Python 3.14 and system
Python 3.9 with `-I -B`: twelve host methods, fourteen renewal-tail methods, six
renewal-startup methods and 58 bounded writer/output/child/signal/capacity/package
methods. Owned regular handles are inspected before independent raw-handle
fallback. Counters independently enforce one log close, at most two waits with
the original 15/10 bounds, no wait after terminate refusal and one explicit cleanup
status poll;
forbidden process/socket/thread/raw-signal operations are asserted unused.
`git diff --check` passes. No app, actual child, native guardian, thread, socket,
PTY, compiler or remote operation was launched by this selection.

Limits: constructor-internal resources concealed before return remain outside
this owner's authority. Diagnostic failure before its reserved poll can leave
retirement unobserved. Refusal/timeout can retain an unclosed buffered pipe;
explicit failure is not cleanup completion. A failed close is an attempt, not
proof of closure; fault fixtures perform real close before their injected error.
These controls do not qualify a native host/application or certify a build.
Existing input closures already bind both modified production files.

Both independent reviewers passed all 90 selected bounded methods on managed
Python 3.14 and system Python 3.9 without skips and approved the source, tests and
stated API-boundary limits. Root read all four paths and approved this topic for
composition. The final clarification changes only this note; runtime and tests
remain the reviewed versions.


Root composed this topic with the adapted notice oracle and published main
`3234fd0c3e8596051b0c1a75fe9be10ee227018d`. Clean source
`7dc2df6985c14624388dc6b5217129a6d1600773` passed all 480 selected Python
methods in 29.038 seconds with zero failures, errors or skips; HEAD and source
files stayed unchanged. That run includes all twelve host methods, the existing
renewal/cleanup controls, four finite capacity-process methods and the current
24-hour notice fixtures. Discovery separately found 2,054 unique identities
without errors and executed no bodies. The execution log is
`~/.codex/.ubd/reset-notice-host-composition-tests-20261116.log`.
No native or application qualification is inferred from these controls.
