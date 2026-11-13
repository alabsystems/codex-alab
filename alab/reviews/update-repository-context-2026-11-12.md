<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep update operations in their requested repository

## Finding and decision

The update controller's Git calls inherited repository redirection variables.
With `GIT_DIR` and `GIT_WORK_TREE` naming a private foreign clone, an update
requested for one repository fetched the foreign origin, built its newer source,
and selected that fixture artifact. The common-directory lookup likewise placed
its journal in the foreign clone. The separate
[promotion fix](promotion-repository-context-2026-11-12.md) closes the related
ancestry-authorization gap.

Use the shared promotion Git environment for update Git calls and the binary
common-directory lookup. Pass the same environment when launching the build,
fetched controller and cache guardian, so inherited redirects cannot reach
downstream Git operations. It removes inherited `GIT_*` settings, disables
global/system Git configuration, replacement objects and lazy fetch, and
suppresses optional lock writes. Repository-local configuration and explicit
`-C` paths remain in effect; required fetch and worktree writes remain allowed.

Keep the original repository, fixed source, selected checkout and prefix
arguments unchanged. Preserve non-Git environment values, including ALab and
ATPKG custody, compiler, cache, interpreter and host settings. No extra selector
permission or unconditional retry is added.

## Validation

Both new library regressions failed before the change: one selected the foreign
origin's candidate, and one returned the foreign common directory. A separate
public-entry regression also failed before the change. After composition with
the promotion fix:

| Module | Passed | Seconds |
| --- | ---: | ---: |
| `test_promote` | 21 | 6.091 |
| `test_local_update` | 18 | 13.674 |
| `test_local_update_job` | 39 | 62.619 |

The real-Git fixtures use private local origins. The public entry records status
in the requested clone, selects its already available artifact, and does not
build the foreign source. Existing tests exercise fixed-source handoff, cache
locking, crash recovery, prepared jobs, changed selection, preservation and
external common-directory paths. Independent review found no concrete blocker;
whitespace validation passed.

The tests use fixture selectors and inert packages. No live selection, fetch,
installation or deployment was performed. This is local workflow validation,
not a native release gate or proof of provider credentials.

The five selected logs are retained under
`$HOME/.aterm/.ubd/update-git-context-20261112-b4p34fpe.noindex`. Their manifest
SHA-256 is `3ef17b01320eb2f916b6c555f2e95c07e741399e3909e8ce562012f6df2ffc92`.
