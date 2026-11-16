<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Identify sync merges by their subjects

A miniature Git history confirmed that delta handles a stale upstream ref when
its newest real sync merge records a newer upstream parent. A follow-up maintenance
merge quoting `alab sync:` in its message body exposed a defect: `git log --grep`
matches the full message, so delta selected that maintenance merge's local parent
as upstream. The report then hid carried local hook changes.

The same full-message selection was used by sync's rewritten-history and concurrent
main checks, and by setup's rerere training. A local maintenance parent must not be
treated as the last integrated upstream commit in any of those paths.

Decision: share subject-based readers in `alab/lib.sh`. Enumerate first-parent
merges, filter the rendered subject, and return either the matching merge IDs or
the newest matching merge's second parent. Preserve Git read failures. Read the
candidate list completely rather than causing an early-pipe SIGPIPE. Delta, both
sync checks and rerere training use these readers; training still uses the newest
30 genuine sync merges. No fetch, sync admission, resolver or push policy changes.

The quoted-body scenario failed before the fix. It now retains the real upstream
baseline and its local hook patch. The shared-reader contract also returns exactly
the genuine sync merge and upstream parent; an injected history-read failure
returns a failing status without a fallback report.

Validation with atpkg Python 3.14: 29 delta-tool, 27 feature-inventory, 22 sync and
17 GitHub-tooling tests passed (95 total). Bash syntax checks passed individually
for lib, delta, setup and sync; `git diff --check` passed. A manual shared-reader
query returned the committed upstream parent in this checkout. All changed paths
are ALab-owned tooling, tests or this record; no application build is claimed.
