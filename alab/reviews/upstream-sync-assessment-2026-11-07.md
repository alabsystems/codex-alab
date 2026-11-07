# Upstream sync assessment

Decision: measure the next sync without changing the qualified greeting source or
starting an unvalidated merge. Agh fetched upstream main into the local-only
refs/agh/review-upstream-main. The fetched snapshot is
cce9d3228b5d649cea400bc6d19514a0342ad998; upstream moved after the earlier API
lookup, so the assessment uses this exact fetched object, not that lookup's SHA.
Compared with our clean 1c47069442da001709e7980707cfbe05085d6f09, 339 upstream
commits are absent. Since the last merged upstream 3026a8e54e0f, 1931 files
changed, with 118510 insertions and 23988 deletions.

An object-only git merge-tree --write-tree --name-only assessment used the same
Cargo.lock merge attribute and upstream-copy driver as sync.sh. It exited 1,
reporting 69 conflicted paths. Comparing those paths with the repository's
removed-paths and owned-paths policies accounts for 39 through removal and none
through ownership. Thirty remain before rerere replay or manual resolution.
This is a raw merge assessment, not the result of the sync script's complete
adaptation, lock resolution, hook audit or native gate.

The remaining paths include the two precomputed protocol schema blobs,
app-server extensions and thread processor, API responses/WebSocket handling,
codex-mcp manifest, core residency/client/compaction/config/guardian and session
handling, tasks, exec, auth manager, memory guard, model manager, thread store,
and seven TUI paths. They require semantic review preserving upstream behavior
and reapplying ALab hooks. No merge tree was checked out, no main/index changed,
and no check or proof was claimed for the candidate tree.

Full local assessment output is retained at
~/.codex/handoffs/upstream-merge-assessment-2026-11-07.txt,
SHA-256 1823ed6e2d665afe39d80e0683f892f8b64d445c3f36a2a1914eaae5c2b5b338.
The object-only candidate tree is c45ce548498f845ebd643a75d62e0c3e013f8d2d;
it contains unresolved conflicts and must not be used as a build or release.

Neither local checkout has an upstream remote configured. Source inspection
confirms setup.sh already delegates GitHub fetches to alab/github.py, which
requires Agh and refuses raw GitHub Git fallback. Thus transport does not need
a new workaround; a future dedicated sync should use the supported setup and
sync workflow, train rerere, resolve the remaining conflicts, and pass the real
native source gate before landing. This assessment does not alter the staged
greeting package or resolve its separately deferred rollout/adoption.
