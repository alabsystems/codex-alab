<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve filename whitespace in churn rankings

Review of delta's report consistency found that the history counter assigned to
awk field `$1` to remove `uniq -c`'s count. Assigning a field reconstructs the whole
record, collapsing whitespace within the filename. Diff rows retain that whitespace,
so their names no longer match the history keys and receive zero churn and risk.

Decision: remove only the anchored count prefix with `sub`, leaving the rest of
the filename intact. Keep the existing history window and risk formula unchanged.
The one-line implementation change is entirely within ALab-owned tooling.

A real-repository regression creates three files in one baseline commit and then
changes one line in each. Ordinary names must retain churn 1/risk 2; repeated-space
and leading-space names must receive the same values. Before the fix, both unusual
names incorrectly received churn 0/risk 0. After the fix all three match.

Validation with atpkg Python 3.14: all 26 delta-tool tests and all 27
feature-inventory tests passed. `bash -n alab/delta.sh` and `git diff --check` passed.
No Rust application qualification is claimed by this tooling change.
