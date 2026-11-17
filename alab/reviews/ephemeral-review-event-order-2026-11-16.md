<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep ephemeral review work inside its original turn

Decision: ephemeral review qualification requires its single completed command
before its single rendered answer, both after the native turn start and before
completion. Matching counts and contents cannot establish continuity when work
is published outside that turn or in reverse order. Apply the same requirement
to the existing no-renewal control.

At source `e8320f61c4`, the public `Review.verify()` oracle accepted reversed
turn boundaries, a completed command or answer before or after its turn, and
an answer before its command. Its existing delegate/account identity, command
output and exact answer checks all passed. Private before/after controls retain
all twelve malformed recovery/control traces under
`~/.codex/.ubd/exec-review-order-negative-20261116`. The repaired oracle refuses
them before publishing `reviewContinuity`; the ordinary ordered controls pass.
These controls use synthetic requests and the public Python oracle, not a
native receipt or an actual Codex process.

Four new test methods cover the six malformed orderings, the no-renewal control,
and valid nonfatal warnings and reasoning items. Seven regression assertions
fail before the repair. All 18 module methods pass afterward with managed
Python 3.14 (`-I -B`) and macOS Python 3.9 (`-B`); `git diff --check` passes.

Only the ephemeral rendered-evidence path changes. Durable structured review
evidence and the existing native parent-turn and account bindings retain their
meaning. No compiler, provider account, native application run or certificate
is claimed.
