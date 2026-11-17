<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Matrix output ownership and incomplete CLI exits

Decision: reuse the existing binary output-owner policy for the matrix's single
controller log, preserve the selected body failure through the final manifest
save, and apply the maintained CLI exit policy after argument parsing. The base
is `f42fc6b92cec1664ae1e5340b10916608b6f8cb6`.

## Concrete failures

The initial read-only audit exercised actual public `renewal_matrix.main` with
the maintained status fixture's explicit inert admission/controller seams and
real owned regular log files. Its 18 controls included the original source's
`__main__` AST guard. A selected controller `KeyboardInterrupt` survived healthy
close, but a secondary actual-close-then-`OSError` replaced it with an ordinary
failed report; a secondary `SystemExit(0)` escaped as zero from both API and
original entry. A genuine nonzero control could likewise lose its identity.
These fault reports remained `passed:false`: this was an API/CLI defect, not a
demonstration of full native false qualification.

The separate 50-control final-manifest audit exercised public main and the
original entry with healthy, ordinary-error and cancellation bodies and finite
before/after-write failures. Final save could replace the selected body error.
A healthy body followed by a real completed final write/close and
`SystemExit(0)` left both entry status zero and `passed:true`. These were inert
one-case Python observer controls; no native host or complete matrix was run.

Before the fix, 12 maintained behavioral log/entry methods produced 34 failing
controls, no errors and no skips on both managed Python 3.14 and system Python
3.9 against unchanged base source. The additional generic three-file owner
control is an API adaptation test and is excluded from that baseline count.
The initial audit artifacts are private under
`.codex/.ubd/renewal-matrix-log-audit-20261116`; the separate manifest audit is
under `.codex/.ubd/matrix-manifest-audit-20261116`.

## Implementation

`capacity_exec.OutputFiles` freezes named path pairs and preallocates empty
handle slots before acquisition. Each returned `xb` handle goes directly into
its slot inside the ownership guard. Handles return in requested order and
close independently in reverse order through the one existing failure-selection
and nonthrowing diagnostics policy. Class-based `__exit__` returns false to
retain active exception identity, including `StopIteration` and hostile
exception hooks. Close-only zero exits become `RuntimeError` with the exact
original cause. `OutputLogs` is the thin stdout/stderr specialization, retaining
its constructor, tuple, paths, labels and readable root/label/handle state.

The matrix uses one named `case.log` handle. Its command, admission order,
subprocess call, exact result-byte hash/decoding and stable input-digest policy
remain unchanged. Caught ordinary failures use the existing safe error fields,
including available native notes or the Python 3.9 fallback transport. A local
body primary survives a failed final save with safe secondary diagnostics;
without a primary, the original save error still escapes the API. An ordinary
body error with a healthy final save still returns 1.

The API `main()` retains its parser and error behavior. `cli()` parses before
running the guarded operation, preserving help status 0 and parser status 2.
The original entry now rejects incomplete zero exits while preserving genuine
nonzero controls and `KeyboardInterrupt`. The exact-path stdlib CLI helper is
already in the matrix's pinned inputs; the output owner is in its already-pinned
capacity support file. No new source-input dependency is omitted.

## Verification and limits

The tests use existing fixture modules by alias, avoiding duplicate TestCase
discovery. Log controls close real underlying files before injecting close
errors, observe closed state and `EBADF` before fallback, and independently
check one-close and forbidden process/signal/socket/thread counters after
guaranteed owned-file fallback. Positive controls retain the original command,
exclusive filename, bytes, controller status and result SHA. The new log module
has 13 methods; the new manifest module has 10 methods and 86 finite controls,
including healthy operation, before/after real writes, exact selected primary,
available or unavailable notes, hostile diagnostic formatting and an unrelated
caller exception. The historical 50-control manifest audit is separate from
these 86 final controls.

Managed Python 3.14 and system Python 3.9 each pass 135 selected methods with no
skips under `-I -B`; the selected IDs are unique. The exact filename patterns are
`test_renewal_matrix_log_cleanup.py`, `test_renewal_matrix_manifest_cleanup.py`,
`test_renewal_matrix_status.py`, `test_renewal_matrix_results.py`,
`test_renewal_matrix_digest.py`, `test_renewal_host_cleanup.py`,
`test_renewal_tail_cleanup.py`, `test_renewal_startup_cleanup.py`,
`test_write_json_cleanup.py`, `test_runner_output_ownership.py`,
`test_runner_child_cleanup.py`, `test_runner_signal_ownership.py`,
`test_capacity_exec.py` and `test_smoke_package_records.py`. Each is loaded with
standard `TestLoader.discover("alab/tests", pattern=name,
top_level_dir="alab/tests")` and composed in one suite. This is a selected Python
regression run, not the full workflow or a substitute for native prerequisites.

This is one close attempt per returned handle, not proof that an arbitrary
failing close retired its resource. It does not recover resources concealed
inside a failing open, add atomic replacement, promise durability, or roll back
a final manifest. A failed final save may leave `passed:true` on disk; the
actual CLI's nonzero status prevents a consumer requiring both report success
and zero status from accepting that incomplete observation. There is no new
native OS retirement, executed-image, certification or full qualification claim.
No app, guardian, native compiler, socket, PTY, thread, child or remote service
was launched by these new finite fixtures. The canonical native gate remains a
separate required boundary.

Final review: the hostile reviewer independently approved all five paths,
including both new test modules, and passed all 135 selected methods on Python
3.14 and 3.9 without failures, errors or skips. The integration reviewer
independently reviewed both production files, the 13-method log module and this
note, and independently passed the same 135-method selection on both versions;
that reviewer authored the 10-method manifest contribution and does not claim
independent review of it. Root read and approved all five paths and authorized
this commit. Diff and new-file whitespace checks are clean.


## Root composition

Clean source `fc655c091e1abcdd924cd0145a0ce5aa3f16830b` passed all 503
selected Python methods in 28.906 seconds with zero failures, errors or skips.
Source HEAD and worktree stayed unchanged. The selection includes all 23 new
matrix methods and the existing status/result/digest, renewal, cleanup, runtime,
weekly and finite-process controls, plus two explicit pre-promote fixtures.
Separate discovery found 2,077 unique identities without errors; it executed no
bodies or native prerequisites. The retained selected log is
`~/.codex/.ubd/matrix-output-composition-tests-20261116.log`.

Publication composition preserves incoming main
`43eb2dca2d761c31e0e5941facf6531c65ea9156`, whose startup-retry Rust test and
review refinement changes no selected Python source. The execution evidence
keeps its exact source attribution; it is not a Rust test, full native-gated
workflow, application certificate, package or installed-adoption result.
