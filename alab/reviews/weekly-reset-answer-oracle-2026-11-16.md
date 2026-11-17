<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Weekly-reset recovery answer — 2026-11-16

Decision: a successful weekly-reset smoke must deliver the fixture's exact
answer once as a completed agent message, between its single native turn start
and completion. Request, reset and retained-tool checks remain necessary; an
answer-shaped warning or a replayed answer cannot establish successful recovery.

At source `90cb1af4011724b7198be9710fc599d5d08bab8b`, the real `case()` oracle
accepted duplicate answers, answers outside the turn, reversed turn boundaries,
an unfinished answer and answer text attached to an error item. The six new
test methods fail six assertions against the old predicate. They run the full
case oracle with its process and listener boundaries mocked, preserving the
backend authentication, consume policy and completed plan requirements.

A separate inert Python stand-in reproduced acceptance through the real
loopback listener and unchanged command guardians. It performed the five
authenticated backend requests and two expected model requests, then emitted
the fixture answer only as an error item. The old case saved `passed: true`.
The repaired oracle rejects the same stand-in and saves no passing result.
These requests use fake authentication and no real reset credit.

macOS Python 3.9 with `-B` passes all 29 weekly-reset fixture test methods after
the repair, including valid completed-tool output and nonfatal warning controls.
Managed Python 3.14 independently passes the same 29 methods and all eight
quota-wait oracle methods. `git diff --check` passes.
Only ALab-owned Python and this record change; no
Rust build, native Codex execution, certified package or installation is claimed.
