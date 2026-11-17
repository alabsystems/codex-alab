<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve qualification observer failures while retiring owned resources

When a qualification read or output operation was interrupted, a secondary
`terminate` error skipped waiting, and a consumed stdout-close error could
replace `KeyboardInterrupt`, `SystemExit` or an ordinary streaming error. The
outer gate-log close could replace the selected exception again. A separate CLI
edge let `SystemExit(0)` or `SystemExit(None)` from an incomplete `run` appear to
its caller as command success.

Decision: retire the owned Popen and close its stdout independently, collecting
cleanup failures before selecting an exception. Preserve an already pending
cancellation; otherwise promote the first cleanup cancellation over an ordinary
primary error with that original error as its cause. Other failures become
best-effort exception notes. Reporting failures do not replace the selected
ordinary error, while a new reporting cancellation still takes precedence over
an ordinary error. Three owned start/log/final file contexts share this policy;
normal close errors still propagate. The caller's output stream is never closed.

Retirement checks Popen's cached returncode before further operations. A failed
TERM attempt does not skip a 30-second wait while the child remains unknown.
Initial wait timeout requests KILL escalation. After KILL returns normally, the
existing unbounded reap is retained; cancellation during it is recorded and
retried while returncode remains unknown. After a failed or cancelled KILL,
there is one bounded 30-second reap attempt instead. An ordinary reap error,
failed final wait or timeout leaves retirement unconfirmed, with its error
retained rather than claiming the child was reaped. Stdout closure is attempted
once regardless. All signals remain through the owned Popen object; no raw PID,
process-group, privilege or permission workaround is added.

The public `run` API retains exact cancellation objects and exit codes. At the
CLI boundary only, zero/None SystemExit from `run` becomes status 1 and an
incomplete-observation diagnostic. Ordinary returned success stays zero;
nonzero SystemExit and KeyboardInterrupt retain their behavior. Argument parsing
remains outside this catch, so `--help` still exits zero without running a gate.
Gate command, directory, environment filtering, source checks, schema and proof
policy are unchanged. The observation/retirement failures covered here occur
before publication of a completed qualification record.

The same 34 maintained public-run/main controls use coherent exact Git input
files from `64e4e8271ef831b49fc95bdb9c21543478cfb260`; the baseline qualifier
blob is also identical at the task base `877ed2221db78a482702d1a1406bcfa9802358e3`.
Baseline: 26 assertion failures, eight passes, zero errors/skips, actual exit 1.
Fixed: all 34 pass, actual exit 0. Process, pipe and callback faults use a wholly
module-local fake subprocess namespace. File wrappers close only real owned
temporary files before injecting an error. No fake descriptor reaches the OS,
and these fault controls launch no gate or signal any process. They do not call
or mock the new private cleanup helpers.

Separate controlled managed-Python CLI subprocess probes compare zero, None,
nonzero 47 and help cases. Before the CLI guard their actual child exit codes
are `[0, 0, 47, 0]`; afterward they are `[1, 1, 47, 0]`. The callbacks supply the
SystemExit faults, `run` is mocked, no completed record is made, and no actual
gate executes. Exact KeyboardInterrupt identity is checked without an OS signal.
These probes establish the callback/CLI boundary, not spontaneous native-gate
success or an application certificate.

The final complete three-module union runs from committed composed source
`9d1876085840b0ed51c1b63dff7f8e19bbaf51e2`, tree
`88d327f6c4a3f33081ab2ebf68d4d24258a9edd2`: 37 existing qualification methods,
34 cleanup/main methods and 15 upstream-status methods. All 86 distinct outer
methods pass in 55.173 seconds, actual exit 0, with zero failures, errors, skips,
expected failures or unexpected successes. Head/tree and clean state remain
unchanged, as do all captured ALab Python sources, selected scripts/tests and
Git/Python/sh/bash entrypoint fingerprints. Existing integration controls use
only temporary committed shell gates, including an owned shell's self-TERM;
they verify source guards, schema, credential filtering, signal/exit mapping,
record inspection and status consumers. The production Rust gate is not invoked.

The earlier cleanup-only 84-method run and subset counterfactuals overlap this
final union and add no unique coverage count. During test preparation a file
fault shim compared a noncanonical `/var` temporary path to `/private/var`, so
four injected faults were unobserved. That failed fixture packet is retained,
not relabeled. Resolving the owned root and requiring nonempty file observations
fixes the fixture; fresh coherent baseline/fixed packets establish the claims
above. The earlier 25- and corrected 32-method inventories remain historical
observations, with their original hashes and results recorded separately.

Root verifies exact baseline Git bytes, coherent fixed/helper/test snapshots,
case inventories, actual outcomes and the CLI transition. Independent review
approves the lifecycle, cancellation selection, main boundary and public tests.
Final SHA-256 identities:

- Qualifier: `3a0911a1e7bf4b90074cec4c62d7272e0ba6d45db6205ddb24dc1bd4ca446d65`.
- Controls: `571628e47df86b88d5b8f1e11efb7772a1e2055d284c0e6b2921157f728ec4c2`.
- Final union log: `f6ccf189a538d0a592c727dc32bf3dc26c31805d7182992c11b1fee6b94dc458`.

The common Git directory retains 96 payloads (833,601 bytes), including committed
source snapshots, counterfactual/CLI source and raw output, final and prior
observations and explicit fixture history, at
`alab-validation/9d1876085840b0ed51c1b63dff7f8e19bbaf51e2/qualification-cleanup-0nsj6rnw/`.
Index SHA-256:
`32322c5eb1309f4a491aeb62b20548721a44de3981bfe0ea5c58fda633579a68`.
Root checks every payload size/hash and the exact regular-file inventory.

This composition preserves concurrent main `d2892c7774badc1ad0de7555d569a89c61d8fe39`.
The incoming sync file differs, but its 1,111-byte extracted `run_check` body is
byte-identical to the earlier task base, SHA-256
`198ed82d79d976f4f167e8e6642596ddcf7ad8a2d8550a39f15c337e8b26d5fb`, and is
exercised by the final fixture union. Incoming Rust, feature and intake changes
are not relabeled as validated by these Python checks. No complete Python
workflow, shipping build, native application qualification or changed execution
lane is claimed.

A final publication fetch observes `b458b893b02805f31a66882c93206b003dbaf8da`,
adding further sync read checks and their tests. The qualifier, cleanup controls
and supporting Python inputs are unchanged; the extracted `run_check` body
still has the same 1,111-byte hash. Preserve those commits without replaying
unrelated incoming tests or changing the source identity of the final 86 run.
