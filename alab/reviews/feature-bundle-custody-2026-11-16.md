<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retain failed evidence exports without deleting replacement data

Baseline `610696ddc177d4725ac598295b62decd5fb3f5ce` creates a bundle destination
exclusively, then recursively removes that pathname after an export error. A
caller can move the created partial directory aside and replace its pathname
before a later captured-log read fails. The rollback then deletes the replacement
and leaves the original partial behind.

The original controller completed with status 0 after two public bundle
observations. An ordinary second-read failure removed its own partial directory.
The replacement control moved the actual created directory, made another directory
with a sentinel at the original pathname, then raised the same module-local EIO.
Rollback deleted the replacement sentinel while the held directory retained the
created inode. Both calls preserved the original exception object, source log
inode/bytes and input record. These were owned filesystem fixtures with synthetic
execution declarations; no command named in those declarations was executed.

Decision: remove automatic recursive rollback. Failed exports retain their material
without attempting a pathname custody check followed by deletion. The exclusive
destination remains reserved, so retrying at that path still refuses. Success,
log deduplication and portable evidence validation retain their existing behavior.
Exception and cancellation objects propagate directly.

An early copy failure leaves no record, and the retained partial cannot validate
as a complete portable bundle. A late failure can leave complete, independently
valid record/log bytes. Retention does not establish a new completion protocol or
make every failed call's bytes invalid; the existing validators decide from the
available declaration and hashed logs. The ordinary cleanup expectation was
intentionally changed to require retained logs and no record for its early read
failure.

The final coherent pair uses identical nine new tests with exact baseline/fixed
source bundles. Reused fixture setup, Git, record and observation helper ASTs are
identical; the only other fixture change is the intentional old retention
expectation. Baseline exits 1 with eight assertion events across seven failed
methods and no errors. Fixed exits 0 with all nine passing. The combined fixed
consumer run exits 0: 43 distinct methods pass, with no errors, skips, expected
failures or unexpected successes. It includes the new nine, all 22 evidence tests
and all 12 feature-gate tests; the overlapping nine are not added again.

Controls cover ordinary and cancellation retention, replacement directories and
symlinks, held-original and sentinel identities, invalid evidence before claim,
incomplete portable validation, success/deduplication and exclusive existing
destinations. The consumer gate bodies stop at inert build hooks and a generated
argument-capture frontend. No native runner, Rust compiler, real gate/profile,
installed selector, certification or network operation is exercised.

The first validation controller exited 1 and remains separate history. Its two
fixed symlink assertions compared resolved `/private/var` paths with their `/var`
aliases; its private consumer closure omitted shell inputs and reported seven
missing-file errors before a build hook. The corrected run resolves both target
paths and captures all tracked top-level ALab inputs plus VERSION and publication
hooks. Production source bytes did not change during that correction. Source,
bundle and tool inputs match before/after each run; the exact controllers are
copied and indexed with their records.

Fixed source SHA-256:
`ccf6d7ae8c4140a2e6fb69ea4715b6f692a06a743de3c493880e8413b91ecef5`.
Final new test SHA-256:
`5b8cd453f926113e520f3305ae0315a60c20e838c95257a7440930a8626b7734`.

The sealed common-Git archive is
`alab-validation/8c599ba312f0449b3a18698772d9500931f564cd/feature-bundle-custody-eqrjqn3w`.
Its 340 payloads total 6,220,827 bytes; the index SHA-256 is
`8fc9dbb2daa4dc9f03dd4e721c6da163b82c5fec0bf02c79ede760eb5bbf27a8`.
It preserves the original proof, initial failed validation, corrected validation,
their controllers, raw logs, terminal observations and input copies. It also
contains the separately scoped [existing workflow differential](python-workflow-differential-2026-11-16.md).
The archive construction completed with status 0 after checking every indexed
size/hash and exact file inventory. Two earlier archive-only construction errors
remain recorded: they rejected, then incorrectly excluded, indexed generated
bytecode caches. The completed archive copies those six artifacts without
executing them or attributing them to Git source.

There are 267 exact Git source bindings. The original proof and baseline runtime
inputs bind `610696ddc177d4725ac598295b62decd5fb3f5ce`; fixed runtime inputs
and the identical final new test bind
`8c599ba312f0449b3a18698772d9500931f564cd`. The initial draft new test,
SHA-256 `a980080d7945427002861ba3802d358aeb05f7c2ef633950297d8805dbb76a66`,
appears in both initial arms and is explicitly unbound to any Git commit. The
corrected controller SHA-256 is
`9d5955bff62e790c1a101288cc68de98f2bac52fb6b619e7f3bddd252e93e4e7`.
Its original process 44142 exited 0; the original proof exited 0 and initial
validation process 30662 exited 1. These runs executed frozen private copies
before the fixed bytes were committed; a clean committed-tree run is not inferred.

Publication merges concurrent main
`98ca741f4124decfd7b61ff81eb758c199c432d6` through
`48f289a04e8576ec65c7b3029a583bb998e880b9`.
The merge adds two review records and one committed intake JSON and changes
`codex-rs/alab-tui/glue/startup_account_read.rs`. All 88 captured fixed runtime
inputs remain byte-identical to both the committed fix and the merged tree.
The recorded Python result therefore needs no repeated functional run. Incoming
native code is not claimed as executed. These observations establish neither a
native gate nor application certification.
