<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Preserve diagnostics-root validation errors during cleanup

The adjacent review found that `diagnostics_root` could replace its validation
error or pending cancellation when closing the owned descriptor also raised.
The existing `open_directory` path already had the required cancellation policy.
The fix extracts that policy into one private helper used by both paths. Each
attempts close once and then re-raises the selected error. The existing traversal
ownership transfer, no-follow flags and note wording stay unchanged.

An ordinary cleanup failure becomes a note on the original error. A cleanup
cancellation supersedes an ordinary validation error with an explicit cause and
note; an already pending cancellation remains primary. Successful validation
returns the owned descriptor without closing it. Invalid state and a failed open
do not trigger descriptor cleanup.

## Evidence and scope

The initial read-only control loaded the exact `unit_results.py` blob from
`57e908852b670cb086b44ebd164cc28644a60eb8` (SHA-256
`b4fcf2b6fcca676f4b770aebf2bac2c18293d79e4a03d96e94d5ff54415037ca`).
Its eight methods produced eight assertion failures and actual status 1: six
lost the required selected-exception identity, while two already selected cleanup
cancellation but lacked the established explicit cause and notes. The real
identity-mismatch refusal was included. Synthetic descriptor operations closed
only ledger entry `100`; no real unowned descriptor was touched.

The final maintained module ran unchanged against that saved baseline and the
fix. Baseline status was 1: 26 methods, nine assertion failures, zero errors in
0.005 seconds. The additional failure covers an ordinary identity-read `OSError`.
The fixed module passed all 26 methods with actual status 0 in 0.002 seconds,
including all 13 previous traversal controls. The existing result-collector
fixture suite passed all 26 methods with actual status 0 in 12.770 seconds.
Positive validation therefore covers 52 unique methods. These are ordinary
Python controls and existing fixtures using a reused authenticated native
adapter; no Rust build, protected-profile change, application certification or
full gate was run.

Final source hashes:

- `unit_results.py`: `30148dd0e3151ecb97ddb5c53c3b91da5f2d45a8d7a2307d92e773b39fe56fbf`
- `test_unit_directory_cleanup.py`: `e5839eb333df9a23ad270f215aed71b5dd5e97509166f565fa84e85f9970c5b5`
- Existing parser tests: `be161b23b4e4a0d8b635c953d267a9efef6e4a2843938bac97721d4b491fb4af`
- Reused native adapter: `b3ba555c1b547cf89fc7931e15fc70b5e92ffe803f417b973e451f4bb0998d0c`

Raw records are `<evidence>/codex-diagnostics-root-baseline.log`,
`<evidence>/codex-diagnostics-root-maintained-baseline.log`,
`<evidence>/codex-diagnostics-root-maintained-fixed.log` and
`<evidence>/codex-diagnostics-root-parser-results.log`. The initial control runner
hash is `214b2171d13d2c3a538870d4433c9de3cace3a7ee874e4edc8d03e1ea3c97b20`;
the maintained source-selection runner hash is
`8b087e479e6d801ee72e96c4503395117dec3d9048655430f2d301f0bd26219b`.

Root independently verifies the baseline bytes against Git, both source-selected
log contexts and outcomes, and all 26 parser outcomes. Direct unittest discovery
also passes the same 26 focused methods without the source-selection runner;
this repeats coverage and does not increase the 52-method count. The existing
native adapter is reselected from its Cargo metadata and artifacts and retains
the recorded hash. Independent read-only review approves the exact frozen
production and test sources with no blocker.

Thirteen raw payloads and frozen sources are retained under the common Git
directory at
`alab-validation/57e908852b670cb086b44ebd164cc28644a60eb8/diagnostics-root-cleanup-0m6mxnvn/`.
The index SHA-256 is
`07b9c0f8d2cec1f710411ca6fd7e9e7eb8b47179282615836700e30e5228085b`.
