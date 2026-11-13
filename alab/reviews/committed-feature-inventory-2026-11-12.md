<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep feature inventory on one committed source

## Finding and decision

After making local topic inventory independent of an upstream remote, review of
its caller found a mixed-source report. `feature_inventory.py` read hook topics
from committed `HEAD`, but read the feature catalog and implementation paths
from the working tree. An uncommitted catalog could repair a committed unowned
hook; an untracked implementation could satisfy a path absent from the commit.
Conversely, deleting a local implementation rejected an otherwise valid commit.
A staged unowned hook was also absent from the report without any source label.

Make this CLI consistently inspect a commit. `--source REV` defaults to `HEAD`,
is resolved once, and supplies the catalog, hook topics and owned path entries.
The result names the full commit ID. This follows `delta.sh --topics` and the
existing committed-source admission tools. No working-tree mode is needed by
the known callers. Relative `--repo` paths are resolved before invoking child
commands.

Read owned paths from the Git tree, including directory and symlink entries.
A symlink entry itself is an owned path; a filesystem target or a child reached
through that link cannot satisfy another path. This checks declared source
ownership, not whether a link resolves on the checking machine. The in-memory
validator retains its filesystem default for existing direct callers, while
the CLI injects committed path membership.

All object reads and the topic helper share a clean Git environment, disable
replacement objects and lazy fetching, and ignore system/global Git config.
An inherited `GIT_DIR` cannot redirect the requested repository; replacement
refs cannot substitute another tree under the printed source ID. These are the
same source-selection safeguards already used by admission tooling.

This is a local ownership diagnostic correction. The admission gates already
have independent committed-source readers; the old CLI was not an admission
bypass. Semantic qualification remains separate.

## Validation

Three negative controls against the prior CLI, in a separate temporary source
tree, failed exactly at their assertions: it accepted a dirty catalog repair
and an untracked implementation, and rejected a valid commit after a local
implementation deletion. No setup errors occurred; the temporary tree was
removed without changing the implementation under review.

The regression suite covers those cases, staged hooks, older revisions with
different catalogs/topics/paths, annotated tags, invalid and non-commit
sources, relative repository paths, directories and policy files, Git
environment overrides, replacement objects, and symlink ownership boundaries.
All 45 feature inventory, evidence and admission tests passed, as did the nine
topic-inventory CLI tests. The canonical checkout reports 79 hook topics and
11 features at the selected full source ID. Whitespace validation is clean.

No Rust code, lockfile, installed binary, or remote configuration changes.
These checks make no compiler-proof or release qualification claim.

## Deferred observations

GitHub issue listing still returns HTTP 403 because the available development
token lacks issue-read scope. That operation is deferred; no credential change
or alternate identity was attempted. Local review supplied this improvement.

Review also identified a pre-existing schema gap: non-list `policy_files`
values are silently skipped. It does not affect the source-coherence change
and is addressed separately in
[feature-policy-schema-2026-11-12.md](feature-policy-schema-2026-11-12.md).
