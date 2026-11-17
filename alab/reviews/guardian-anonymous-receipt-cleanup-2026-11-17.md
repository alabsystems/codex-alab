<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve anonymous receipt failures during cleanup

Decision: replace only the exception handler in `command_guard.run_recorded_fd`.
A failed receipt invalidation or duplicate-descriptor close previously replaced
an active disk resource stop, interrupt or exit. In an actual Python child with
an inert command boundary, a resource stop followed by ordinary cleanup failure
exited 1 instead of 75; cleanup `SystemExit(0)` instead made that child exit 0.
Both invalidation and close reproduced this behavior. These are receipt failure
precedence defects, not evidence of a native admission bypass.

The handler attempts invalidation once for a validated descriptor, then close
once, collecting both failures independently. An existing `BaseException`
primary keeps its identity. For an ordinary `Exception` primary, the first
cleanup `KeyboardInterrupt` (including its subclasses) or exact non-successful
`SystemExit` may become the selected exception, with the original primary as
its explicit cause. Ordinary errors, successful integer/None exits and
unsupported `SystemExit` subclasses do not replace the primary. Integer exit
classification uses actual types and unbound `int.__index__`, retaining POSIX
native-width wrapping and out-of-range behavior. No foreign `__class__`, code
getter, payload operator, string or repr participates in selection.

Each collected failure may add only a fixed phase note. Optional note hooks
cannot mask the selected exception, even when they raise a cancellation.
There is no retry, active-primary exit normalization, receipt protocol change,
new import or change to phase precedence. The success path still retains its
duplicate until process exit by the existing design.

Source scope on admitted base
`4da443a5f0b8b29ea78c3174057e0a1e10081d31`:

- Original `command_guard.py` SHA256:
  `5b50bb042a61d37ee30783d5bc85ac9f7c33e4218cc41c912d0929212c069267`.
- Candidate SHA256:
  `f698d9f9cb10f0d13b739f83dc33aed2acf0bec7b568d611bcda1d3af0dab9b9`,
  exactly matching the manager's proposed whole-source candidate.
- Every byte outside that handler is unchanged. Validation, `run` arguments,
  excluded descriptors, completion write and return have identical ASTs; the
  unchanged try-body AST SHA256 is
  `2ed3138092374e4eace5db056e1225acb6942e9b166c2dcc552b6fdda9e6fa85`.
- Frozen new regression SHA256:
  `e1ffc510b4fed0b462559c1708f26ca84409569ab1e925c9858c491b8d8692a2`.

Managed Python 3.14.8 with `-I -B` executed the new eight test methods in
2.919 seconds, exit 0, with no failures, errors or skips. The source-bound
runner recorded 271 controlled API calls, 24 actual Python children and zero
foreign classification/formatting callbacks. The API matrix covers ordinary,
resource, hostile ordinary, interrupt, nonzero exit, zero exit and other
`BaseException` primaries; both cleanup phases; hostile exit
subclasses; opaque payloads; hostile integer subclasses; two-phase failure
ordering; optional note failures; validation refusal; and partial writes.
The dedicated optional-note controls deliberately invoked ten failing hooks;
all ten were suppressed without changing the primary.

The 24 children execute the original source entry AST or its public receipt API,
with only the command boundary and named cleanup operation replaced. Their OS
statuses, source hashes, entry hashes, excluded descriptors, original descriptor
shape and exact observed inode bytes are asserted. Actor seams refuse any
custodian, subprocess or signal activity; all actor counters remain zero.
These are real Python exit and inode observations with injected descriptor
failures, not native command-group, custody, compiler or application proof.
When truncation is refused, the real partial bytes remain. When close is
refused, only an attempt is established; independent fixture compensation
closes owned descriptors after recording the observation.

The same frozen tests replayed the exact committed original source from a
single-file snapshot, without a second worktree. That run exited 1 in 3.356
seconds: 283 failed assertions, zero errors/skips, 263 controlled API calls
(some methods stop early), 24 actual children and zero foreign callbacks.
Those counts describe this regression matrix, not distinct bugs or native
qualification. Earlier manager-side 233-case memory experiments remain
separate and were not counted as repository tests. The current and baseline
runners both asserted unchanged before/after source and test hashes.

The broader existing-test invocation ran all 85 guardian/result/wiring methods
without a failure, but exited 1 because `test_check_lint_step.setUpClass`
requires the unavailable actual `codex-alab-unit-runner` artifact. It was not
replaced, simulated or waived. The clean prerequisite-independent rerun of `test_command_guard*.py`,
`test_guard_result.py` and `test_disk_guard_wiring.py` passed all 85 unique
methods in 20.189 seconds, exit 0, with no failures, errors or skips.
Its complete selected source/test SHA256 bookends were unchanged.

Independent native-subagent review checked the production selection policy,
fixed note handling, hostile witnesses, actual inode limitations and unchanged
success path; it found no blocking issue. Root independently checked the exact
whole-source hash and handler-only source/AST scope with the same conclusion.
No Rust, application build, certification, package, adoption or native
qualification was performed for this source topic. Private evidence lives in
`/private/tmp/receipt-cleanup-evidence-20261117/` (runners, exact source/test
snapshots, source-scope JSON, logs and before/after result JSON).
