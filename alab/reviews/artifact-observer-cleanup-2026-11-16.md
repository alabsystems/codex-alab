<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve artifact inspection cancellation through descriptor cleanup

A local artifact inspection interrupted by KeyboardInterrupt or SystemExit(47)
could become an advisory packaged-stage OSError after a later descriptor close
failed. Standard ExitStack already registered each owned descriptor before
fstat, popped callbacks before calling them, and attempted every callback in
reverse acquisition order. No missing-close, retry or descriptor leak was
reproduced. The defect was exception priority: a later ordinary close error
replaced the cancellation, and observe then consumed that ordinary error.

Decision: retain ExitStack ownership and use its exit-callback interface for
one descriptor-close helper. Close once and return no suppression value. An
already pending cancellation survives later close and reporting failures; a
cleanup cancellation takes precedence over an ordinary pending error, with
that original error as its cause. Otherwise retain the ordinary pending error
and add cleanup diagnostics as best-effort notes. On a successful body, the
first close failure still refuses identity; later cleanup cancellation may
promote over that ordinary failure. Every remaining callback is still attempted
by ExitStack. No failed close is retried: an error may occur after consuming
its descriptor, so retrying could close a reused descriptor.

Package flags, regular-file and executable checks, bounded metadata reads,
mutation checks and observation output shape remain unchanged. This performs
no artifact execution, qualification, promotion or permission workaround.

Independent review approved the implementation and final controls. The final
24 maintained public package_identity/observe tests run against coherent exact
baseline `cb2e3eb2ed1074a651e75bf2a41669d7ae3ffa98` and fixed source with identical
final tests. Baseline: actual exit 1, 21 assertion events across 19 failing
parent methods, five fully passing methods, zero errors/skips. Fixed: actual
exit 0, all 24 pass, zero failures/errors/skips/expected failures/unexpected
successes. Before/after inputs are identical; only artifact_status.py differs
between selected source bundles. Real owned temporary descriptors are consumed
by real close before injected close faults; only this module's os namespace is
proxied. Tests verify primary identity, cleanup promotion/cause, naturally
retained phase/repr notes, open/fstat/read failures, close order and falsey
callback behavior. No observations or exception notes are fabricated.

The initial draft used a noncanonical macOS temp path and failed before any
descriptor acquisition. A corrected draft's baseline then stopped with actual
status -2 during its fourth parent method when the old code selected an unexpected
KeyboardInterrupt; its fixed 24-method run passed. Final tests catch
BaseException at their boundary and still require exact object identity, so a
wrong cancellation becomes an assertion failure instead of aborting unittest.
Teardown drains all remaining owned descriptors before asserting and rethrows
a cleanup cancellation after draining. Earlier packets retain their actual
statuses and distinct test hashes; they are not relabeled as the final run.

Root's final complete affected-module run uses clean committed source
`d2c062aa820271a0afe231395ac3c08bf9c2b78b`, tree
`602a84b57357a0dd6b5a6694a12ebff35644e7b8`: 69 distinct methods (18 artifact status,
24 artifact cleanup, 27 upstream status), actual exit 0 in 14.300s, zero
failures/errors/skips/expected failures/unexpected successes. Head, tree,
clean state, captured ALab sources/scripts/tests and Git/Python/sh/bash entry
identities are unchanged throughout. This includes main
`4d5db85044c44a8b01564c95407b358deeab3f21` and its assessment-error controls.
The counterfactual 24 methods and earlier complete 69-method snapshots overlap
this result; counts are not additive. Tests use local fixtures; existing
process-image controls mock ps/lsof. No artifact execution, network fetch,
production Rust gate or native application certification occurs in these tests.

Frozen implementation SHA-256:
`fe1897c9afd97f08679cb6060659b11a838caa300c6b419cf469feaec70bcd70`.
Final test SHA-256:
`dc4631fa1bb0e180c8234f41c3311325832360eb79b610cb6fe3b76ddafd57d8`.
The recursive common-Git archive is
`alab-validation/d2c062aa820271a0afe231395ac3c08bf9c2b78b/artifact-cleanup-owpcl8fu`:
461 payloads, 5,484,856 bytes, including exact source snapshots, final and prior
counterfactuals, drivers, contexts, raw logs and all three root snapshots.
Every payload size/hash, exact inventory and selected source Git binding is
verified. Index SHA-256:
`aeb40a187774cdc5175581d20e89da44bfd75010c560eadd5ce56e761c1da624`.
Final raw root log SHA-256:
`1d3d850cb337ad452b703cd141b13b519f224a424e6ef8e7d9dd39efbbe2e028`.
