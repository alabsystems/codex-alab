<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve weekly-reset case cancellation during cleanup — 2026-11-16

Decision: reuse the shared fixture cleanup support already imported through
`capacity_exec.py`. Listener teardown must attempt every later safe action,
retain request evidence, and preserve the original cancellation. Cleanup
failure fails the case and stops the selected suite before another case starts.

At source `433a952e2c`, public `weekly_reset.case()` already saved its result
after teardown; no stale passing result was observed. Its sequential cleanup
still skipped close and request retention when shutdown failed. A bounded
control using the existing successful-turn fixture reproduced that loss.
Through public `main()`, a body `KeyboardInterrupt` plus a shutdown `OSError`
became an ordinary failure return with the cleanup error, no interruption flag,
and no request trace. These controls use fake authentication and process/listener
boundaries; no native application or earned credit is used.

The case now captures its primary exception explicitly and calls shared
`cleanup_case()`. A failed shutdown skips only the unsafe unbounded join;
closing and trace retention are still attempted. An ordinary cleanup failure
saves a failed case record and returns it without running the success oracle.
The suite checks that returned verdict before marking selected coverage as
successful. Shared error serialization retains native exception notes on newer
Python and the existing `cleanupErrors` report field on Python 3.9. The original
cancellation still wins if final probe hashing, report writing or printing fails.

Only the existing capacity import is expanded, after the early isolated exec
wrapper. No new dependency or custody-list entry is introduced. The runtime,
history and renewal-matrix loaders already load or pin `capacity_exec.py`.
The command runner and all quota, reset, retained-tool, answer, cancellation,
binary-identity and coverage assertions remain unchanged.

Eight new public case/main regression methods reuse the existing full fake
preflight flow. They cover successful teardown, each cleanup failure,
simultaneous failures, body versus cleanup cancellation precedence, failed
suite accounting, and cancellation with both cleanup and summary failures.
The normal control passes against the old source; seven negative methods
expose 55 assertion failures there. All 52 weekly-reset test methods pass on
managed Python 3.14 and independently on macOS Python 3.9. `git diff --check`
passes. No Rust build, native application run, certified package, installation
or new reset-behavior qualification is claimed.
