<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Bind qualification to its execution root and child sources

## Findings and decision

Hostile review of the fresh-index observer found two further ways that a clean
Git observation could differ from the files the gate reads.

First, local `core.worktree` could direct Git to a clean mirror while the
requested directory held a changed `alab/check.sh`. The recorder inspected the
mirror, executed the changed gate, and accepted its zero exit as qualification
of the original commit. Require the canonical requested root to equal Git's
canonical worktree root before inspecting or running its gate. A symlink alias
of the same actual root remains valid; a subdirectory or redirected root does
not. Do not override the configuration silently, since the gate's own Git
commands could still follow it.

Second, parent status delegated submodule inspection to each child's real
index. An `assume-unchanged` entry in a child or nested child concealed changed
dependency bytes even though the parent used a fresh index. Check each committed
gitlink independently: its contained directory must be an initialized worktree
with the expected HEAD, and its staged and working state must pass the same
fresh-index checks recursively. Parent status no longer delegates those checks.
Missing, unbound and escaping children cannot provide clean evidence. No child
is initialized, fetched, reset or checked out by this observer.

The shared read-only Git helper disables optional index refreshes. Parent and
child real indices remain unchanged. Schema 3 identifies observations with
these checks; schemas 1 and 2 remain historical and cannot populate
`qualified_sha`. This is still local Git-state evidence under Git's existing
filter and ignored-file semantics, not authenticated attestation or a new gate.

## Validation

The original implementation reproduced both false qualifications. The new
root regression failed on the redirected directory, and both child and nested
child regressions failed on hidden edits. The corrected source passed:

| Module | Passed | Seconds |
| --- | ---: | ---: |
| `test_qualification` | 29 | 24.860 |
| `test_upstream_intake` | 9 | 3.675 |
| `test_upstream_status` | 15 | 10.465 |
| `test_upstream_gate` | 18 | 19.144 |

Positive controls retain ordinary and aliased root execution, clean initialized
children, and unchanged real child index bytes. Further tests reject a child's
different HEAD and an unbound directory. Independent probes also rejected
missing and escaping child paths, retained parent/child index bytes in every
case, and found no blocker. No compiler, full native gate or installation ran.

The eight-file log and probe bundle is retained at
`$HOME/.aterm/.ubd/qualification-root-child-20261112-rhb5ytwu.noindex`.
Its `evidence-manifest.json` SHA-256 is
`7ca1c3283b900052c3d8c4e2e64b09d98bdbfb5bd2ff5d2f89a793bd79588f35`.
