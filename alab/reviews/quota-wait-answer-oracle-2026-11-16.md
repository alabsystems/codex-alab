<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Quota-wait recovery answer — 2026-11-16

Decision: require the stale-reset smoke to publish the fixture's exact answer
once, between its single `turn.started` and `turn.completed`. Request counts,
retained input, backoff and a successful exit establish recovery only when the
client also delivers the supplied result. Preserve the cancellation branch and
allow quota-wait warning items during a successful recovery.

At base `1bd04ff247e9c2c0f15f1f81dcf3295c33fa9b78`, the actual case oracle
accepted missing, changed and duplicate answers, missing or repeated starts,
reversed turn boundaries, and answers outside the turn. The new eight-method
module failed on eight subtests before the repair. A separate inert Python
stand-in reproduced missing-answer acceptance through the real loopback server
and unchanged command guardians: five requests respected the growing backoff,
the stand-in read the successful fixture reply, then omitted its answer from
JSON output; the case nevertheless saved `passed: true`.

Managed Python 3.14 with `-I -B` passes all eight new tests after the repair,
plus all fifteen request-recovery tests and five shared capacity-fixture tests
(28 methods, no failures, errors or skips). The new tests call the actual
`case()` with the process and listener boundaries mocked. They also retain
controls for process failure, extra requests, changed input, early retries,
handled SIGINT, cancellation retries, completion and guardian timeout. The
SIGINT control mocks signaling and starts no child.

Only ALab-owned Python and this record changed. No Rust build, native Codex run,
provider account, installation or certificate is claimed by these checks.
