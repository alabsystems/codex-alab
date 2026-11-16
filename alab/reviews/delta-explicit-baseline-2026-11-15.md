<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Explicit local baselines for delta reports

While reviewing server-error recovery on source `bd29f58d50`, `alab/delta.sh`
refused before measuring anything because this checkout has neither an `upstream`
remote nor `refs/remotes/upstream/main`. The committed sync history still contains
an upstream parent suitable for an explicit local comparison. No remote was added
and no fetch or source-history mutation was needed for this read-only operation.

Decision: support `--base=REV` for summary and patch reports. Resolve the selected
commit and require it to be an ancestor of the requested source before emitting a
delta. Label explicit comparisons as baseline measurements and report upstream lag
as unknown. Do not infer that a caller-selected ancestor is the newest upstream
commit, and do not change the existing default upstream-selection behavior.

Empty, unknown, unrelated and repeated baselines are rejected. `--topics` rejects
an explicit baseline because its committed hook inventory does not compare trees.
Patch output remains limited to modified baseline files, leaving source additions
out of the upstream-style patch report.

The four new real-repository scenarios failed before implementation. Validation
covers offline summary and patch reports, explicit selection even when a local
upstream ref exists, historical sources, empty diffs, and invalid or ambiguous
requests. A manual report against the latest committed sync's second parent
`d9a32c178951695f365faa36a39d542e06a96e4f` also completed in this checkout.

Validation: all 20 delta-tool tests and all 27 feature-inventory tests passed with
the atpkg Python 3.14 interpreter. `bash -n alab/delta.sh`, `git diff --check`, the
help output and the real explicit-baseline report also passed. The default
upstream summary/patch scenario remains covered by the delta-tool suite. This
change touches ALab-owned shell tooling, tests and documentation only.

Publication found main advanced to `fc597906ae`. Merge `700f1da2c7` composed the
independent terminal/mail and intake changes without conflicts. Both Python suites
passed again on that merged source: 20 delta tests and 27 inventory tests. This
validation covers the tooling change; it is not a new Rust application qualification.
