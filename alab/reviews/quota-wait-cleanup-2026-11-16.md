<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retain quota-wait cleanup and interruption evidence — 2026-11-16

Decision: a quota-wait case may save a passing result only after its listener
cleanup and request-trace retention succeed. A cleanup failure fails the case;
all later safe cleanup actions still run. A failed shutdown skips only the
unbounded thread join. An original cancellation keeps its exception identity
and exit semantics while secondary cleanup and reporting failures remain visible.

The public `quota_wait.case()` at source `e8320f61c4` saved `passed: true` before
its `finally` block. A bounded fake process/listener control, using the real
quota fixture and full request/backoff/answer oracle, injected a shutdown
failure: the passing case file remained, neither join nor close ran, and the
request trace was absent. Through public `main()`, an ordinary interruption
saved no incomplete summary; the same interruption plus shutdown failure was
replaced by an ordinary `OSError` exit and lost its cancellation identity.

The existing request-recovery cleanup implementation is now shared from
`capacity_exec.py`, which both smokes already import. That support module is
already part of the protected runtime and renewal-matrix input closures; no
new module, loader or custody-list entry is needed. Request recovery retains
its Python 3.11 minimum and native exception notes. Quota wait still runs on
Python 3.9: it keeps the same primary exception, transports secondary diagnostics
on that object, and serializes them in the existing `cleanupErrors` report
field. This does not claim native traceback-note support on older Python.

Quota wait saves its case result after shared teardown, stops before another
case when cleanup failed, and retains the active case in incomplete summaries.
Its final summary writer rethrows the original cancellation even if writing or
printing the summary also fails. HTTP request counts, retained input, growing
backoff, exact answer and native SIGINT assertions are unchanged.

Eight public case/main regression methods cover successful-save ordering, each
cleanup phase, simultaneous failures, primary and cleanup cancellations,
incomplete summaries, and cancellation with both cleanup and summary failures.
All eight methods expose the old implementation when loaded in memory; the
control records 49 failing subtest assertions and four errors. The repaired
source passes 50 selected methods on managed Python 3.14: 16 quota wait,
29 request recovery (including its cleanup suite), and five shared capacity
fixture methods. The eight new methods also pass on macOS Python 3.9.
`git diff --check` passes. No Rust or native application runs, build,
qualification, package or installation are claimed.
