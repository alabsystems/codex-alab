<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Recheck Autopilot after the intervening alignment changes

The earlier complete Autopilot run preceded the Kimi, policy and native-source
changes integrated while the tooling work continued. Run the complete library
suite again against the composed source rather than extending the old result.

At clean source `e60f2080d7b37ef40c2735462911e652ff8ac001`, the source-selected
Trust compiler built the changed dependency graph and the library suite
passed: 918 passed, four existing ignored cases, zero failures, in 121.77
seconds of test execution. The command was `targo --unverified test --locked
-p codex-alab-autopilot --lib`, with four build jobs and test threads and test
debug information disabled. The ignored cases remain unclaimed measurements.

The source lock, full log and invocation manifest are retained in the common
Git directory under
`alab-validation/e60f2080d7b37ef40c2735462911e652ff8ac001/autopilot-20261113/`.
This is runtime regression evidence across the merged layer. It does not
replace the certified application build or imply an installed update.
