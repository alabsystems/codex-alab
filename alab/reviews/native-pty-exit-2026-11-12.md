<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Drain native PTY fixtures through child exit

## Evidence and decision

The broad native TUI run at `db518933b02d2ada98e10f76bf2ec34187a9a98a`
did not finish. The input-custody fixture received its success marker, then
failed its original 20-second deadline while waiting for the child to exit.
Unwinding entered its local `Reap` guard and blocked in `Child::wait`.
A sample of the parent confirmed the `Reap::drop` to `wait4` stack. The child
could not be sampled despite remaining in the process table. Stopping the
owned test job released both processes; neither remains running.

The fixture stopped reading its PTY after that success marker, before the
child finished emitting output and exited. Its cleanup also killed and waited
for the child while retaining the PTY master. Three native fixtures copied
that cleanup pattern and retained the spawning `Command`, including its
parent-side slave descriptors.

Keep draining the input fixture until the actual child exits, then collect
buffered trailing output. Retain every protocol assertion, the successful
exit-status check, and the original deadline. Emit and verify a one-MiB payload
after the success marker so the fixture exercises output backpressure at this
boundary on every run.

Share a test-only owner for the child and master among all three fixtures.
Consume the command on spawn, and close the master before forced kill and
reap during unwinding. The other two fixtures already read through exit;
they also drain trailing bytes before inspecting their markers. Retry
interrupted reads and handle EOF/EIO without treating either as child success.

This fixes an observed missing-drain dependency. It does not establish why
the historical child was in its kernel exit state, or promise a bound on every
possible operating-system wait. The helper still reaps its own child normally.
No production input, terminal-mode, upgrade, or timeout policy changes.

## Validation

The first fixed input-custody run passed in 6.44 seconds, including the full
one-MiB payload. A temporary mutation then removed only the final loop's read.
It failed at the existing deadline in 20.09 seconds with the expected child-exit
diagnostic, and the cleanup guard reaped the child normally. The source file
was restored byte-for-byte; its saved before/after SHA-256 values match. The
counterexample proves the regression detects missing drainage, not the precise
cause of the historical kernel state.

On `01e48105fabc8318b176514dcef03b49540cd30f`, composed over
`65d94a96402cffde4bde440933b20882f2a9c4bd`, all three affected native cases
passed individually: input custody in 6.31 seconds, same-PID startup/DA1/resize
in 0.31 seconds, and original-mode recovery in 0.05 seconds. The rebased startup
case retains main's actual DA1 exit query and reply assertions.

The composed four-thread run completed both suites. ALab TUI passed 368 tests,
with five ignored, in 98.36 seconds. Native TUI passed 6,385 tests, failed one,
and ignored six in 855.82 seconds. All three changed PTY fixtures passed in this
run. This is not an all-green full-suite result.

The remaining failure is
`an_unprepared_target_reports_refusal_and_continues_native_work`: its private
launch handshake timed out before the child produced an outcome. The retained
host request list contains `version`; the child says the host did not answer in
time. That does not establish whether the host wrote its reply. This is the
previously observed handshake failure class, separate from the PTY child-exit
failure repaired here. Its witness is `/tmp/ac.2ITTQH/target-refused`.
The non-credential diagnostic files were also copied to
`$HOME/.aterm/.ubd/codex-pty-exit-full-handshake-failure-20261112/`.
The follow-up diagnostics work records those missing handshake stages without
changing deadlines or claiming to repair that timeout.
An unchanged isolated retry passed in 3.74 seconds, retained as
`codex-pty-exit-handshake-retry-20261112.log` in the same private log directory.
That retry does not turn the failed full run into a passing one.

The completed run is `codex-pty-exit-full-20261112.log` under
`$HOME/.aterm/.ubd/`; its adjacent `codex-pty-exit-full-source-20261112.json`
records source and tree IDs. Only this review text changed during the run.
Tests used the Trust unverified lane with the committed lockfile, offline
resolution, and an 8 MiB test stack. No release gate or compiler proof is claimed.

The earlier interrupted broad run is not a passing suite result. Its raw log
and parent stack sample are
retained privately as `codex-notified-probe-full-20261112.log` and
`codex-notified-probe-parent-sample-20261112.txt` under `$HOME/.aterm/.ubd/`.
The first pass and mutation are `codex-pty-exit-fixed-20261112.log` and
`codex-pty-exit-counterexample-20261112.log` there; the mutation's adjacent JSON
records its exit status and the matching source hashes.
The composed focused logs are `codex-pty-exit-final-input-20261112.log`,
`codex-pty-exit-final-startup-20261112.log`, and
`codex-pty-exit-final-modes-20261112.log` in the same private directory.

## Composition with subsequent main changes

Main subsequently added joined-reader backpressure and native goal restoration
at `5b11594d75`. The merge retains the tested `01e48105fa` in history. On the
composed `c60e0f91de156262bc06aa664d5a56ae1aed789e`, which also adds the separate
handshake observations, a three-thread `native_` selection passed 24 ALab TUI
tests (one ignored) in 1.16 seconds and 71 native TUI tests (two ignored) in
25.33 seconds. This includes the changed input and terminal-mode fixtures and
the three active/inactive native upgrade parents. The log is
`$HOME/.aterm/.ubd/codex-pty-handshake-composed-native-20261112.log`.
This scoped composition check does not replace the earlier full-run record.

An additional standalone startup replay on this composition was queued but did
not execute: another session held the machine's timed-work reservation. The
queued request was cancelled without bypassing that reservation. Its admission
log is `codex-pty-handshake-composed-startup-20261112.log` in the same private
directory. This replay was deferred at that stage; the earlier startup pass,
the full-run startup pass, and the composed native input/mode results remain
distinct.

The later replay completed at `ecf11e2e44` in 0.30 seconds. See
[the follow-up record](native-handshake-followup-2026-11-12.md) for its source,
the subsequent complete native run, and the separate MCP registration failure.

A later test-support lint run found the ownership accessor's `expect()` violated
the workspace lint policy. An explicit invariant match now retains the same
panic condition without that forbidden method. The test-support library passed
Tippy with warnings denied; this changes neither PTY ownership nor cleanup order.
