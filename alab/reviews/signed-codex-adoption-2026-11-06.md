<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Verify the artifact source after a restart

The authorized self-restart completed: the proven supervised PID changed from
62817 to 24968, and the session continued from its handoff without losing work.
No check or build was running when the restart began. All source was on remote
main, and the session's worktrees and branches had been removed.

The first adoption claim was incorrect. The local tag atpkg-codex-2026110601
points to 8c20cbf8e9, which contains the opening-exchange change, but the selected
artifact identifies an older source. Actual observations:

- Signed selection: Codex build 2026110601, index 94.
- Installed alab-build.json source and packager:
  5c1ea61e73ab3a32f211b5e6879d177d6a318d2f.
- Installed codex-package.json version, CLI --version and the live PID 24968
  session record agree on 0.1.0-dev.14858+codex.0.167.0.g5c1ea61e7.
- `git merge-base --is-ancestor e43dc7ca82 5c1ea61e73` exits 1. The artifact's
  declared source still has unconditional Cruise takeover and leaves no opening
  exchange under Cruise. It does not contain the first greeting fix.

There is no demonstrated compiled-version/metadata mismatch: the binary label,
live record and package metadata agree. The mistake was inferring artifact
contents from a repository tag instead of reading the package's own identity.
No release pipeline defect is established by this observation alone.

Decision: confirm adoption with the live session record and artifact metadata,
then check the declared source against the required fix. A newer build number or
a package tag's target alone is insufficient. Do not restart repeatedly into the
same artifact or claim that source on main is already deployed.

The greeting, quiet-state publication and restart-evidence source changes remain
landed. A package containing the required source is still needed. Apple's tools
and the qualified compiler runner remain unavailable on this Mac, so a local
validated replacement cannot be built here. The open installer window and the
earlier not-frontmost refusal remain respected; no consent boundary was bypassed.

## Exact-source promotion check

The existing local promotion guard was exercised read-only against the actual
installed 2026110601 artifact and an isolated temporary selection marker for
the prior 2026110301 package. The requested source was the full greeting-fix
commit e43dc7ca82732a0b4498a6134e957aefb8971d7e. It returned `selected=false`,
`the package is not a forward source/version update`, with zero selector calls.
The installed store and selection were not modified by this simulation.

A regression now covers a valid, internally consistent, forward artifact that
lacks a newer requested commit. It proves no selector is consulted, then proves
the same artifact is eligible when its actual source is requested. All 17
promotion tests passed. This adds coverage for the adoption incident; it does
not require changing the already-correct production promotion guard.

## Truncated required tools

Independent artifact-reuse review found a missing structural condition: a
zero-length codex, code-mode-host or rg retained its executable mode and valid
package records, so each could reach the selector. The new regression failed for
all three before the fix. Require a nonempty regular protected executable for
each required tool. Restoring the file contents makes the same candidate eligible
again, and every truncated case now causes zero selector calls.

All 18 promotion methods passed, and the 52 available update methods passed on
commit c4c7434be2. The historical-bootstrap prerequisite remains deferred as
previously recorded. This is an additional structural artifact check, not a
claim that package contents have been comprehensively rehashed.
