<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Unit-result directory descriptor cleanup

The next-issue review found a concrete exception path in
`unit_results.open_directory`. After opening a child directory, it closed the
parent before transferring ownership to the child. A close that consumed the
parent and then raised caused cleanup to retry that parent, replacing the
original error with `EBADF` and leaving the child open.

The fix transfers ownership before closing the previous parent. Cleanup attempts
the current descriptor once. It preserves the original ordinary error or pending
cancellation and attaches secondary ordinary cleanup failures as notes. As in the
existing guardian cleanup policy, a cleanup cancellation takes precedence over
an ordinary traversal error; a pending cancellation remains primary. Traversal
flags, relative opens and successful returned-descriptor ownership are unchanged.

Decision: fix this demonstrated local resource defect beside the prerequisite
and result-collector work. GitHub issue listing through the normal `agh` route
returned a non-retriable HTTP 403 for unavailable issue-read permission, so issue
selection proceeded from nearby source review. A separate delegated release-gate
validation check was rejected by automatic safety review for possible
cybersecurity risk before any established reproduction or source edits. That
check remains deferred; it was not rephrased or retried. The selected fix changes
neither native prerequisite admission nor result qualification.

## Counterfactual and checks

The private initial control loaded the actual committed `69f8b51aba3f30f247c5817b54f062e71952dab8`
blob (SHA-256 `2c72ae5cc801b243d6e61a3e64e49731dbb483ef2f4f719376580135d24977c7`).
All descriptor operations used a replacement namespace with a synthetic ledger;
none closed a real descriptor. The three ordinary/cancellation cases failed as
expected: close calls `[100, 100]`, child `101` still owned, and the exact original
exception replaced by `EBADF`. The actual control process exited 1.

The final maintained test module also ran against that saved blob through a
private source-selection runner, without changing the baseline source. Its 13
methods produced ten assertion failures, zero errors, and actual process status
1. The same module against the fix passed all 13 methods with actual status 0.
The controls include successful root/nested traversal, path refusal, root/child
open failure, failed closes at two traversal depths, secondary close errors and
mixed cancellation precedence. The existing `test_unit_results.py` suite passed
all 26 cases in 11.145 seconds with the reused authenticated native unit adapter;
positive validation therefore covers 39 unique test methods. No Rust build,
application certification or full gate was run.

Final source identities:

- `unit_results.py`: `b4fcf2b6fcca676f4b770aebf2bac2c18293d79e4a03d96e94d5ff54415037ca`
- `test_unit_directory_cleanup.py`: `29d0f7992d301569df48ba53db903db5543ec9a1daddc574c5a9f5d9af85a9ac`
- Existing parser tests: `be161b23b4e4a0d8b635c953d267a9efef6e4a2843938bac97721d4b491fb4af`
- Reused native adapter: `b3ba555c1b547cf89fc7931e15fc70b5e92ffe803f417b973e451f4bb0998d0c`

Private raw evidence is retained as
`<evidence>/codex-unit-directory-baseline.log`,
`<evidence>/codex-unit-directory-final-baseline.log`,
`<evidence>/codex-unit-directory-final-fixed.log` and
`<evidence>/codex-unit-directory-existing-results.log`. The source-selection runner
is `<evidence>/codex-unit-directory-selected-control.py` (SHA-256
`8b087e479e6d801ee72e96c4503395117dec3d9048655430f2d301f0bd26219b`).
Its ordinary unittest status and raw logs remain separate from any native gate or
certification claim. No background work remains from these checks.

Root independently verified the baseline bytes against the Git blob, final
focused log contexts and source hashes, all 26 parser log outcomes, and the native
adapter selection from its existing Cargo metadata/artifacts. Direct unittest
discovery also passes the same 13 methods without the private injection runner;
this is repeated coverage, not another 13 unique tests. Independent read-only
review found no blocker in the ownership or cancellation paths.

The common Git directory retains thirteen raw payloads and frozen sources at
`alab-validation/69f8b51aba3f30f247c5817b54f062e71952dab8/unit-directory-cleanup-nwcnjkfv/`.
Its index SHA-256 is
`c95113083ed0bd0e37d18c5cacfccb3fa7837712d2fbeb581975bd763c5b7165`.
Incoming main `4399414a10d258f39ea33e8b480099447fb02c10` changes only review
evidence path redactions; all tested production, parser and native adapter inputs
are unchanged.
