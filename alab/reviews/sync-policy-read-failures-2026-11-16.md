<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Require complete policy reads before consuming sync pathspecs

The follow-up probe audit found that path policies still entered arrays through
process substitution. Git read status was lost in removed-path handling,
owned-path handling, watched-path notes and the merge probe. A failed read could
become an empty policy, and a partial read could still authorize path handling or
a clean probe. The raw reader also emitted partial Git output before returning
its error.

Decision: capture the complete Git policy before filtering or emitting it. Preserve
its error status and stderr. Each consumer captures and checks its own policy
before decoding pathspecs; the probe checks removed and owned policies separately
so one successful read cannot mask the other failing. Skip empty decoded lines
explicitly, preserving successful empty policies without inventing a blank pathspec.

The production-function regressions failed in eleven subcases before the fix:
ten consumer cases with absent/partial failed output, plus partial output escaping
the raw reader. They now stop before any consumer action. Positive controls retain
empty-policy behavior, resolver classification when no paths cover a conflict, and
successful removed-path handling. Fixtures record command boundaries without
removing source files or publishing anything.

Validation with atpkg Python 3.14: 45 sync tests, 30 delta-tool tests, 27 feature
inventory tests and 17 GitHub-tooling tests passed (119 total). The sync group was
rerun after preserving Git stderr and passed again. `bash -n alab/sync.sh` and
`git diff --check` passed. Changes are ALab-owned tooling, tests and this record;
no application build qualification is claimed.
