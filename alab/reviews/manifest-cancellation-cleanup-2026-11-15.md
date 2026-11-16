<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve cancellation while indexing advisory diagnostic manifests

A read-only error-path review beside the merged-tree checks found that
`diagnostic_manifests` could turn cancellation into advisory success. An exception
from manifest inspection entered `finally`; if closing the child also raised
`OSError`, the outer advisory handler suppressed that replacement error. The
original cancellation or nonadvisory error disappeared. A later cleanup
cancellation could also replace an already pending cancellation.

Decision: retain the inner advisory `OSError` handler and give its body the same
error-close policy as directory traversal and diagnostics-root validation. The
outer `BaseException` handler closes the child through the existing shared
helper, then re-raises the selected exception. Normal completion closes it once.
The borrowed parent is untouched. Bounds, no-follow flags, regular-file and size
checks, sorted results and ordinary advisory errors remain unchanged.

## Counterfactual and checks

The initial private control loads the exact committed source from
`599ed29083688a6b937b7d96750eca2ec3b13ba3`, SHA-256
`30148dd0e3151ecb97ddb5c53c3b91da5f2d45a8d7a2307d92e773b39fe56fbf`.
Its ten methods yield five assertion failures, zero errors and actual exit 1.
Three cases lose `KeyboardInterrupt`, `SystemExit` or `RuntimeError` to advisory
success after a secondary close `EIO`; two replace the original cancellation
with a later cancellation. Five ordinary indexing/advisory controls pass.
Every opened synthetic child closes exactly once; no real descriptor operation
or source edit is involved in that baseline control.

The final maintained module runs unchanged against the same baseline and the
fix. Baseline: 36 methods, five assertion failures, zero errors/skips, actual
status 1 in 0.019 seconds. Fixed: all 36 pass with actual status 0 in 0.011
seconds, including all 26 previous directory cleanup controls. The existing
result-collector fixture suite also passes all 26 methods with actual status 0
in 11.795 seconds. Positive validation covers 62 unique methods. The controls
replace only this imported module's OS namespace with synthetic operations;
normal collector fixtures use the reused qualified native adapter read-only.

Final frozen identities:

- `unit_results.py`: `db540dbfc1bee29955e9e548e1cd9feb0887da134ba05c59ca3218e6c9c1d994`
- `test_unit_directory_cleanup.py`: `5af59b4c683264c0cc0b680b1b37431cdce2bfa809a2b14f931432629d5e952a`
- Existing parser tests: `be161b23b4e4a0d8b635c953d267a9efef6e4a2843938bac97721d4b491fb4af`
- Reused native adapter: `b3ba555c1b547cf89fc7931e15fc70b5e92ffe803f417b973e451f4bb0998d0c`

These identities remain unchanged after testing. Syntax and Git whitespace
checks pass; independent read-only review approves the exact frozen code and
controls. No Rust build, compiler/profile change, application certification or
full-gate success is claimed by these focused checks.

Raw records are retained as
`<evidence>/codex-manifest-cleanup-baseline.log`,
`<evidence>/codex-manifest-cleanup-maintained-baseline.log`,
`<evidence>/codex-manifest-cleanup-maintained-fixed.log` and
`<evidence>/codex-manifest-cleanup-parser-results.log`.
The initial control driver SHA-256 is
`384188d8b3638fcf52111bdc47f7cca3d46a1bf407ae2a7b906f3c225eb00108`;
the maintained source-selection runner SHA-256 is
`8b087e479e6d801ee72e96c4503395117dec3d9048655430f2d301f0bd26219b`.

Root verifies the saved baseline against Git and the final source/test/driver
hashes against both counterfactual logs. All 26 parser outcomes are present.
Direct unittest discovery independently passes the same 36 focused methods;
this repeats coverage and does not increase the 62-method total. Fourteen raw
payloads and frozen sources are archived under the common Git directory at
`alab-validation/599ed29083688a6b937b7d96750eca2ec3b13ba3/manifest-cancellation-cleanup-culc9h5q/`,
with index SHA-256
`33b4ab0675143185a598dd9ab0e8b4014fa92005801c280513ef780a335a2d79`.
