<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Request recovery turn boundaries — 2026-11-16

Decision: request-recovery qualification must retain one native turn, with its
single exact completed answer between the start and completion. Missing or
replayed starts and an answer published outside that turn cannot establish
successful recovery, even when request history and retry timing are correct.
Specific-refusal checks and nonfatal warning items keep their existing behavior.

At source `9159727cb1`, `check_result()` and the full `case()` accepted absent
or repeated starts, reversed boundaries, and answers before or after the turn.
The original positive test fixture omitted `turn.started`; it now represents
the complete valid event sequence. Three new test methods cover the public
oracle, full case result and valid warning items. Before the repair, ten
regression assertions fail across the oracle and full case.

An inert Python stand-in confirmed the missing-start acceptance through the
real loopback listener and unchanged command guardians. It retained the exact
completed plan call and output, made the expected three requests, observed the
two-second Retry-After, and emitted the exact answer plus `turn.completed`.
The old case reported success without a start. The repaired case rejects the
same stand-in; adding its start makes the positive control pass. This uses no
provider account, compiler or native Codex binary.

Managed Python 3.14 with `-I -B` passes all 18 request-recovery smoke test methods
and 11 cleanup test methods after the repair. `git diff --check` passes. Changes
remain in ALab-owned Python and this review record; application build and package
qualification are separate.
