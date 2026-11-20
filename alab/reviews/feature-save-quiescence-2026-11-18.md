<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retain accepted experimental feature saves through original publication

Decision: register the existing accepted feature-save future in its actual
Session work ledger. The detached parent previously escaped a ledger capture
and ordinary shutdown joining while its original config write, readback,
warning event or popup response could still be pending.

Production commit `f79dc786094440e1549f5e85a11190ab88b7c2e6` changes one marked
upstream scheduling call to the existing all-platform `alab_spawn_original_work`.
Every other byte of the original future remains unchanged. The same future
retains the original write guard, request handle, pre-discovery, typed config
write, post-write readback, warning enqueue and popup oneshot. The existing
five-second discovery and fifteen-second write timeout policies remain intact;
a frontend join does not make an uncertain backend write succeed. Running-task
settings are not replaced by configured feature readback.

The actual-entry regression method uses its real App and one owned typed Cloud
transport per scenario. After joining that same Session's empty ledger baseline,
it enters the actual save, replies once to real pre-discovery, and holds the
original typed config write. Its capture must remain pending and canceling only
the observer must preserve the guard and original write. An open-popup success
returns overridden configured values through real post-write readback; the
closed-popup transport-error case must still queue the unresolved warning.
The result, exact original warning identity, guard release, unchanged running
task settings and absence of duplicate RPCs are checked after joining accepted
work. Ordinary shutdown cleans up the same Session after semantic failures.

Source review corrected the first external test before execution: post-write
readback admits a real descendant after the held-write capture, changing the
same ledger generation. The old capture must refuse with WouldBlock. A fresh
capture of that same ledger can then establish current quiescence without
resubmitting the save or any discovery. The error path returns before this
post-fetch and preserves the original capture's generation. No generation or
helper rule was weakened to obtain readiness.

The first actual four-method invocation on f79dc786 failed its new warning
assertion; three existing companion methods passed. It had inspected rendered
warning lines, but ordinary warnings intentionally retain diagnostics while
hiding those lines. Commit `2256629ea881da5b21f151298563ecd994a49bee` corrects
only that ALab test oracle: exactly one newly enqueued InsertHistoryCell must
own exactly one WarningEntry with the original message identity, details and
source. Independent source review confirmed this presentation mismatch. The
failed run remains preserved as failure; coverage and collector success do not
turn a failed test into a pass. Production warning visibility is unchanged.

The corrected maintained invocation on 2256629e passes all four exact TUI
methods: the new two-scenario actual-entry method and three existing methods
covering selected server profiles/running-task preservation, server defaults and
configured readback, and bounded pagination/bad-server behavior. Process
96778/start 59827f/completion a67d4e exits zero. Test, collector, coverage,
source-observation and guardian statuses are all zero; there are no failed or
ignored methods. Three complete native library harnesses are authenticated;
only the TUI harness has matching methods. Empty sibling order is not inferred.

Source-selected Trust 15698 executes the maintained unverified test lane through
the existing project admission and protected profile. Before/after source books
are identical over 12,516 inputs and 11 tools, SHA-256
`539cf14f0189dbc48b22aed619490d1e805ab492e9815d03ea60d6aff37fa443`.
Raw unit log SHA-256:
`0ef2c628eb4874f1a45ae69aa493d469802407a9ee04250931219df350f464cd`.
The original failed-run packet remains separate, index
`4355772abde47e499df5fd5f66a886289de9ee114fe3084d0d2f0d47a6b1d120`.
The corrected saved packet contains 48 payloads and 12,625,801 bytes, index
`bb758096be964e85ffbff2eb5225c095226d77aa4f7207be768770346bb34f4a`.
It includes twelve selected immutable Git source blobs and preserves the
unexecuted metadata draft separately from the actual corrected invocation.
Opaque native metadata is only hashed; binaries and caches are not copied.
These measurements apply to their exact source snapshots, not a later merge.

Only the two ALab test files were formatted; the upstream hook is +1/-1, risk 8
and churn 4 against integrated upstream ce3031018e. This is joining original
finite work within the original process. It does not migrate arbitrary Rust
futures, install a replacement App, grant Native authority, certify an
application or establish whole-scene capture/install. The enclosing scene still
needs custody of the actual App FIFO and every other owner. The earlier
[merged gate record](merged-wide-lint-2026-11-18.md) retains its full-scope lint
failure independently of focused unit results.
