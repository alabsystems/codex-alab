<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Smoke matrix oracle accounting — 2026-11-15

Decision: name the already declared, intentionally unrun
`five-hour-wall-far-unnamed` case in the test's exact outside-matrix set.
Keep the seven lost-login exceptions separately exhaustive, and exercise both
far-wall variants against their required proof. Production matrix membership,
proof tables and admission logic remain unchanged. The case's unrun status is
already recorded in [PARALLEL-LOGINS.md](../PARALLEL-LOGINS.md).

The full isolated Python workflow at
`207f853cd6a07eb16ceec545d8f525074147f2d1` ran 1,023 tests in 801.481 seconds,
with one failure, no errors and ten skips. Nine skips were whole tests and one
was a subtest; a pass count cannot be inferred by subtracting ten. The only
failure was this test's stale expected outside-matrix set. All three mandatory
native runner context controls passed. The optional native package and release
checks retain their recorded skips. All 39 immutable archive files were verified;
record SHA-256:
`de81e0f06cbd384eed58f9e25bba3862137ec06d3c690594fbb9dbdd8c20b703`.
Archive: `.git/alab-validation/207f853cd6a07eb16ceec545d8f525074147f2d1/full-python-workflow`.

The repaired module passed all 26 tests in 37.101 seconds, exit zero, without
skips, using managed Python 3.14 with `-I -B`. The tested worktree was base
`105b39b603c05b21af568a0f95dd5a140e4e8e0b` plus the archived test-only patch;
the test's Git blob is `942c2e5c7b44f193361e55ba7849f2bdab5f1acc`.
Between the full run and that base, changes under `alab/` were limited to the
contract and a separate review; Python support and fixture inputs were unchanged.
Both far-wall cases require their own nonempty dictionary proof and reject a
missing, empty, boolean or wrong-case proof. No missing-proof acceptance was
observed. Five focused archive files were indexed; record SHA-256:
`01823609fce5bd23145fd5df6ea6d791002dc563d35128279e60d5ac4f8c6743`.
Archive: `.git/alab-validation/smoke-oracle-accounting-20261115`.

The full workflow remains a recorded failure; no repaired full-suite pass is
claimed. Repeating unrelated native work adds no coverage for this test-only
change. These controller tests use stand-in hosts and establish no real-host
admission, live rotation or application certificate. Certified application
delivery remains deferred at the previously recorded Trust macro-admission
boundary; no installation or agent restart was performed for this repair.
