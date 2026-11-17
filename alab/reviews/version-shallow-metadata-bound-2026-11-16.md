<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Complete bounded shallow-history metadata

`versions.py` admitted shallow repositories after examining only the first
`LIMIT` bytes of their shallow metadata. A parentless root boundary in that
prefix could pass while a later parent-bearing boundary still hid history from
the release ledger check. A real Git fixture reproduced the false acceptance:
Git saw only the shallow child while `whole_history` accepted the repository.

Read one byte beyond the existing 1 MiB limit and refuse an oversized shallow
file before examining any boundaries. Keep the existing object parsing and
parentless snapshot behavior. The change remains in ALab-owned Python tooling
and does not change application code or require a native build.

The regression places a root boundary exactly at a reduced test limit followed
by the parent-bearing child boundary, and checks Git's truncated history. It
failed before the fix. A companion real Git control accepts the root-only file
exactly at the limit and confirms all three commits remain visible.

Validation:

- Python 3.14: the complete `test_version_ledger` and `test_version_lines`
  suites passed, 56 tests with two existing certified-hook skips.
- Python 3.14: `WholeHistoryTests` and `test_versions_diagnostics` passed,
  14 tests, including the new regression and exact-limit control.
- macOS `/usr/bin/python3` (3.9): both new real Git cases passed.
- Independent read-only review found no issues; `git diff --check` passed.
