<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Reject repeated observed test identities within one library

The record-mode collector rejected repeated failures but accepted repeated
passes, repeated ignores and contradictory outcomes for the same test name.
For example, `same::case ... ok` followed by `same::case ... FAILED` could
produce an attributed original failed copy from an internally inconsistent
observation. The Cargo aggregate, round and package checks still apply; this
is not a demonstration that a real failed Cargo invocation became gate success.

Decision: generalize the existing failure-only uniqueness check to every
recognized outcome within each prepared library artifact. A test name must
identify one observation there. The same name in another library retains its
separate provenance. Remove the narrower obsolete check rather than layer a
second policy over it. Failure-summary consistency, final-summary selection,
failed-copy limits and Cargo outcome checks remain unchanged. Quiet and
benchmark-shaped output with no recognized case rows keeps its existing
behavior; this does not introduce a complete-result-count requirement.

The same fourteen maintained public-collector methods run against the exact
Git blob from `b38f8251047969c83f7d347bb9d32b65192a6721`: five assertion
failures, nine passes, zero errors/skips, actual exit 1. With the fix, all
fourteen pass, actual exit 0. Only the five newly rejected repeated or
contradictory cases fail the baseline. Compatibility controls preserve distinct
pass/fail/ignore names, identical names in separate libraries, existing duplicate
failure rejection, quiet/benchmark/empty-library shapes, failure-detail text
containing a summary example, and record-mode `passed: []` semantics. These
controls invoke real `collect` with module-local synthetic directory descriptors
and owned temporary round-status files; no fake descriptor reaches the real OS.

Final ordinary discovery runs all five complete affected modules: 36 directory
cleanup, 19 preparation cleanup, 16 collection cleanup, 14 record-observation
and 26 existing native collector fixture methods. All 111 distinct methods pass
in 10.968 seconds with actual exit 0 and no failures, errors, skips, expected
failures or unexpected successes. Source, all five test files and the reused
native adapter remain byte-identical before and after execution. Earlier subset
runs overlap this union and do not add coverage. Independent review approves
the exact production change and public-function controls.

The updated reader separately recollects the retained actual twelve-library
runtime packet from source `599ed29083688a6b937b7d96750eca2ec3b13ba3`.
Original Cargo status and new collector status are zero; the output names twelve
libraries, no failed copies, and is byte-identical to the original result,
SHA-256 `6491e48b24254e8b9ff6725b53ceb797e77aec6e6ca45d1129621b0776e05ffa`.
All original inputs remain unchanged. A separate read-only scan confirms
observed names are unique per library in that old transcript. Neither action
replays or requalifies the native run as this reader's source revision.

Frozen SHA-256 identities:

- Baseline reader: `5e8f7292c29edc41a9a098ebee610832a382a2d18ed31704ee0eeaeeef958be1`.
- Fixed reader: `d3a0aa8291b37b784aa403d85f267fa854968ecc539e0b5e9f04e28299546773`.
- New controls: `1897b70ff449b9f3cb415d6fef9a9fd9d6334d06e2015e86d520b5228b64d475`.
- Final union log: `0d1fa2d8b4a5556b44187c9b7e2fc09d862acb0d789b2f9820a1d0b8dd60a2da`.

Root verifies the exact baseline blob, source/test fingerprints, all case IDs,
counterfactual outcomes and the final passing union. The common Git directory
retains 22 payloads (271,874 bytes) at
`alab-validation/b38f8251047969c83f7d347bb9d32b65192a6721/record-observation-identity-n3ub3nam/`.
Index SHA-256:
`8ce03575e6df271f31da102bfe13f57c6e667b9a336ef46576e087cc532111f2`.
Every payload size/hash and the exact regular-file inventory are verified.
The index references the immutable earlier native evidence packet and its
verified index, avoiding another copy of that transcript. This base-keyed
archive binds the working fix by its hashes, not a qualification of the base.

Fresh publication main `d8f6d9694a212df4e2129e05b42b27b6e0717557` adds sync
ancestry work in disjoint paths. Preserve it without relabeling these checks as
a run of unrelated incoming code. This packet changes only ALab-owned reader
code, tests and this record; it neither runs nor claims the complete Python
workflow, application gate or a new native build/certificate.

That first publication loses a fast-forward race without changing remote main.
The next fetch observes `afa1d02db7e62b003d746fd39761b96dc0f599f9`, adding
API retry handling, its tests, documentation and lockfile entries. These do not
change the reader, its five selected test modules or native adapter code.
Preserve them during composition; the passing Python results and retained
native evidence keep their recorded source/artifact identities. This is not
a validation of the incoming Rust changes or a rebuilt adapter from that lock.
