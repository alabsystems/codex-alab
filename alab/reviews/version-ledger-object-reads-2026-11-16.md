<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Refuse unreadable historical ledger objects

`versions.py` treated every failed `git cat-file -e COMMIT:alab/versions.toml`
as an absent ledger. A real Git fixture first refused a recorded baseline edit,
then accepted the same release after the original ledger blob was removed from
the local object store. Its committed tree still listed that ledger, but the
history check silently skipped its original rows.

Use the existing tree-entry reader to distinguish an absent path from a present
object that Git cannot read. An unreadable tree or ledger now refuses the check;
commits predating the ledger remain valid. This changes only ALab-owned Python
tooling and requires no application build.

The regression removes the historical blob after proving the baseline edit is
refused, then checks the real package release gate. It failed before the fix by
printing the accepted release version. A companion control releases a valid
ledger whose earlier source commit had no ledger path.

Validation:

- Python 3.14: the complete `test_version_ledger` and `test_version_lines`
  suites passed, 58 tests with two existing certified-hook skips.
- Python 3.14: `WholeHistoryTests` and `test_versions_diagnostics` passed,
  16 tests, including the missing-object regression and historical-absence control.
- macOS `/usr/bin/python3` (3.9): a real Git fixture accepted historical absence
  and refused a missing historical blob during the complete ledger history read.
- Independent read-only review found no issues; `git diff --check` passed.
