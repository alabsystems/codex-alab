<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve literal filenames in delta patches

Review of `alab/delta.sh --base=REV --patch` found that patch generation decoded
Git's displayed `--name-only` output as if it were a literal filename list, then
passed those names back as pathspecs through `xargs`. Git quotes control characters
even with `core.quotepath=off`; a tab-containing tracked filename reproduced a
successful report that silently omitted its modification.

Decision: remove filename reconstruction. Ask Git for the modified/type-changed
patch directly, retaining the same derived-file exclusions used by the summary.
This handles literal filenames within Git and avoids quoting, pathspec and argument
batching errors. Additions and deletions remain outside this focused patch report.

The new real-repository regression failed before the fix because the tracked
`src/tab\tname.rs` modification was absent. It also covers brackets, wildcard
characters, newline-containing names and a leading pathspec-like filename, alongside
added, deleted and derived files. Each selected change must appear exactly once,
and `git apply --reverse --check` must accept the generated patch. Explicit-base
and default-upstream modes must produce the same patch for the same comparison.

Validation with atpkg Python 3.14: all 21 delta-tool tests and all 27
feature-inventory tests passed. `bash -n alab/delta.sh` and `git diff --check` also
passed. Only ALab-owned tooling, its tests and this record change.
