# Bind the unit runner to its source and output target

The ordinary runtime helpers already require matching Cargo metadata and a
non-test executable inside the admitted target. The unit runner still selected
an artifact by binary name and kind alone. This executable produces the gate's
library receipts, so use the same identity checks before preparing any library.

The runner belongs to the authoritative ALab workspace even during a pristine
upstream comparison. Query that workspace through its explicit manifest and
save `runner-metadata.json` separately from the tested tree's `metadata.json`.
Both runner metadata and compilation inherit the active output target. Neither
receives pristine source overrides. Select the resulting artifact against the
ALab package, manifest and source, the non-test profile and that active target.

This preserves the separate runner build, full selected library feature graph,
runtime arguments, source guards and receipt protocol. It does not reuse a
candidate runner for pristine output or infer a successful comparison from an
earlier run. No Rust or upstream-owned code changes.

The fixture now models this distinction: the runner crate exists only in its
ALab snapshot, metadata honors the requested manifest while retaining the
calling tree's identity, and the real fixture runner is copied into each active
target. Assertions distinguish ALab source from pristine output and keep
pristine path overrides on the upstream commands alone. Invalid runner
identity, profile or executable location must fail before library preparation;
valid output mixed with unrelated artifacts still selects the real runner.

All 88 tests passed: 48 gate fixtures and 40 adjacent library-result, V8,
disk-guard and proxy-home tests. A private baseline control at `5c418f0c33`
accepted seven invalid runner identity/profile variants and executed the
libraries. Its nine expected assertion failures include two additional old
diagnostic differences; there were no test errors. Shell syntax and whitespace
checks passed.

A native compatibility witness used Trust 15698 with the maintained project
admission and protected profile. From a pristine working directory it requested
metadata and built the runner through ALab's explicit manifest, then selected
the real artifact in the pristine target. Both commands and selection passed;
the artifact was cached, with `profile.test=false`. The witness's authoritative
source was `9254aebdc2a00b356bf0fae46a8aca707ead88dc`, whose Rust workspace and
runner sources match this change. This verifies the source/output arrangement
against actual Cargo records, not a new full-source qualification or fresh build.
The selected runner's bytes also match the real runner exercised by the fixtures.

Detailed commands, source hashes, tests and native records are retained under
`$ATERM_STATE/.ubd/runner-identity-20261112.noindex`.
Source V1 activation remains held.
