<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Wait for fixture launch observations without polling

## Decision

The native upgrade fixtures repeatedly called `yield_now()` while reading the
shared launch-probe flag. That keeps the observing task runnable while a socket
handshake needs the same current-thread runtime. Tokio does not promise another
task or I/O poll after every yield. Its current implementation does poll I/O
regularly, so this is not proof that the loop caused earlier handshake failures.

Use the existing `Shared::wait_launch_probed` notification-based observer on a
blocking worker. Its generation and condition-variable protocol handles early
publication and avoids lost wakeups. Keeping it off the runtime thread lets
the real handshake and other asynchronous work progress.

Once its blocking worker starts, the fixture observer waits for at most 12
seconds, matching the existing startup observer's allowance for one five-second
connection and one five-second status read. Worker scheduling delay is additional.
It does not change either client deadline or the parent's 60-second child
detector. A completed probe may still have no witness; the native tests retain
their separate successful-witness assertions.

The wrapper awaits the blocking task normally. A started blocking task cannot
be cancelled, but its underlying wait is bounded. Cancelling its async caller
does not cancel that worker; the parent detector still bounds the isolated
child. This support is available only to tests. No production startup or
shared-state implementation changes.

## Baseline

The unchanged three-test baseline passed in 16.30 seconds. Its log is
`$HOME/.aterm/.ubd/codex-probe-wait-baseline-20261112.log`; the tested binary
digest is retained in `codex-probe-wait-handoff.json` in the same directory.
That passing baseline is not evidence that the historical intermittent
timeout was reproduced or conclusively fixed by this change.

## Focused proof

All four new current-thread checks pass. They exercise a real private host's
authenticated connection and status reply while the observer waits, an already
published witness, a dropped version reply that completes without a witness,
and a short observation timeout that joins and releases its owned state.

As a counterexample, temporarily calling the blocking wait on the runtime
thread made the real-I/O check fail after 12.02 seconds. Restoring the worker
implementation byte-for-byte made all four checks pass again. This demonstrates
that the check detects blocking the executor; it is not evidence that the old
yield loop necessarily blocks it. The mutation was removed before validation.

Logs are `$HOME/.aterm/.ubd/codex-probe-wait-unit-20261112.log`,
`codex-probe-wait-blocking-counterexample-20261112.log`, and
`codex-probe-wait-unit-final-20261112.log` in the same directory.

The affected native upgrade and backend checks then passed with three test
threads: three passed in 19.25 seconds, followed by three passed in 18.44 seconds.
The logs are `codex-probe-wait-native-20261112.log` and
`codex-probe-wait-native-repeat-20261112.log` beside the unit logs. Both native
waits remain before the fixtures pause Tokio time.

All test builds used Trust's unverified lane with `--locked --offline` and the
8 MiB test stack. The four unit cases and the two native runs have no failed or
ignored tests. The failed direct-blocking counterexample is retained separately.
This is scoped validation, not a full-suite, release-gate, or compiler-proof claim.
