<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Validate both marker sets before committing a sync

The marker audit after `7c74b2bdf7` found that staged and committed marker checks
compared diff output through process substitution. The diff readers' statuses did
not reach the caller. Failed empty reads could authorize a commit; partial failed
reads could instead be diagnosed as genuine conflict markers.

Decision: share a marker-set collector that captures and checks each sorted reader
before intersecting the sets. The collector returns comparison failures too, and
both finish paths require its success before using the result. Its process
substitutions contain only validated data and built-in printing, not Git reads.
Keep the existing staged/committed diff arguments and the rule that markers
appearing only on one side are upstream content rather than new resolution debris.

Eight production-caller read-error subcases failed before the fix: both readers,
empty/partial output, and staged/committed paths. They now stop before commit or
gate actions with read-error diagnostics. Additional sort/comparison failures also
stop. Positive controls retain genuine-marker resolution stops and commits for
known empty or one-sided marker sets. The fixtures execute production functions
with recording boundaries and never commit or publish this source checkout.

Validation with atpkg Python 3.14: 55 sync, 30 delta-tool, 27 feature-inventory,
31 GitHub-tooling and 33 Python-runner tests passed (176 total). Bash syntax and
`git diff --check` passed. Only ALab-owned tooling, tests and this record change;
no application build qualification is claimed.
