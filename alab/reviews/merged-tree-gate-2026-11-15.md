<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Widest merged-tree gate and tracking observation — 2026-11-15

Decision: run the unchanged widest native gate on the exact merged source,
retain an unavailable certification phase as failed, and correct the stale local
tracking observation through the existing verified Agh reader.

Fresh Agh main was `35f846c32c216fa9260faeb0661873f54a006b29`, identical to
root main and the clean detached diagnostics worktree. The reported 197 unpushed
commits were exactly the comparison against stale `origin/main`
`7d314abbfd79d79d9360eae7bd70c8cde7ef2768`. None of this session's current
HEAD commits were absent from authoritative main. Nine local branch tips were
audited; a historical November 13 fixture branch held one unreachable commit
whose patch was already included. Other sessions' refs and worktrees were left
unchanged.

The schema-5 `qualification.py` observer executed the unchanged `alab/check.sh`
in the diagnostics worktree with a fresh explicit target and source-selected
Trust 15698. Its before/after source, tree, base and upstream observations were
identical and clean. The full all-target Tippy phase completed with zero ALab
diagnostics and 41 upstream diagnostics set aside. The certified CLI build kept
its original 16 GiB admission claim, waited 3m03s, then was admitted.

The compiler refused the application's unaudited in-process procedural-macro
graph before proof measurement. Missing proof transport followed that refusal;
the report's synthetic unknown row is not an application proof obligation count.
Gate and controller exited 2 and `qualified_source` is null. Application runtime
scenarios and the ALab/hooked-library unit phases were not reached by this gate.
No compiler override, unverified application substitute, installation or restart
was used. The separate failure to retain the Tippy memory peak was an observed
permission refusal, not permission to bypass it.

The native observation is under the shared Git directory's
`alab-qualification/35f846c32c216fa9260faeb0661873f54a006b29/run-cruise-20261115-1605/`.
Its gate-log SHA-256 is
`d0c0e2d0a257ddbc622610667646d6c6b1c4f14394b0981e24ea8e2cae552c6f`.
Independent review verified the record, log and all four copied certification
reports. Earlier Python and runtime results keep their own source attribution;
they do not turn this failed gate into qualification.

After the gate completed, the selected managed Python ran the existing
`alab/github.py --repo CHECKOUT fetch origin refs/heads/main:refs/remotes/origin/main`.
It freshly enumerates Agh's remote refs, binds imported objects to exact IDs,
checks fast-forward ancestry and updates tracking refs with compare-and-swap.
It exited zero; `origin/main` became the observed `35f846c32c` and both production
probe count expressions returned zero before this documentation commit.
No local branch or HEAD moves. The README now explains why Agh's private
`fetched-main` ref alone does not refresh Autopilot's offline probe. No new
acceptance of arbitrary local refs, compiler policy or production behavior was
introduced.
