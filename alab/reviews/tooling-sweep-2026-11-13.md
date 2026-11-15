<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Complete configured tooling sweep

The full Python workflow suite passed at clean source
`a3b230d61ffe61aa522b9ebc3fc3b83db2d463aa`: 944 tests, eight skips, zero
failures or errors, in 1018.236 seconds. The worktree remained clean, and the
post-run native child audit reported nothing left running.

The run used managed Python 3.14, the built `alab-unit-runner` component via
`ALAB_TEST_UNIT_RUNNER`, and the source-selected Trust 15698 executable via
the native context fixture's `ALAB_NATIVE_CARGO` input. The command was
`python3 -B -m unittest discover -v -s alab/tests`. The runner source is
unchanged from the component build used for this run.

This closes the earlier failed sweep with one complete configured run,
rather than only substituting isolated passing replays. The intervening
changes repaired the runtime-canary scope and the lint fixture's outdated
application stages; they did not weaken production containment or certified
build requirements. The native context timeout was not assigned a cause by
the successful run with an explicit compiler.

Seven skips require opt-in native bridge/controller/cache fixtures that were
not supplied to this sweep. One filename subcase cannot be represented by
this filesystem. Their exact names and reasons are retained in the manifest;
none is counted as passed. This is Python workflow evidence, not native
package qualification, application certification, or installation of the
opaque-400 fix. The previously recorded compiler blocker remains separate.

The complete log and invocation manifest are retained in the clone's common
Git directory under
`alab-validation/a3b230d61ffe61aa522b9ebc3fc3b83db2d463aa/python-tooling-20261113/`.
The log's SHA-256 is
`ff0619c4027ad018e8add86cd52e4ef08fd8399c60e007d6b8c23db4cb9d91c8`.
The manifest also records the interpreter, explicit native compiler, runner
path and runner hash. Do not extend this result to later source revisions
without examining their changes.
