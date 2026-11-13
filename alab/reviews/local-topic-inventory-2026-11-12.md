<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Inventory local hook topics without an upstream baseline

## Finding and decision

The normal feature inventory could not run in this checkout because
`delta.sh --topics` required an `upstream` remote before listing hooks. Its
topic path never used the computed upstream merge base. Keep that requirement
on summary and patch reports, which actually compare history; remove it from
topic inventory.

Resolve the requested revision to one local commit before extraction. The
default remains committed `HEAD`, and annotated tags and revision expressions
remain supported. Dirty working-tree contents do not enter the report. Reject
invalid revisions and non-commit objects instead of reporting an empty result.

Only `git grep`'s no-match status is a successful empty inventory. Capture its
output before formatting, propagate other errors, and never print partial
topic output after a failure. Force uncolored output so Git's display settings
cannot change the extracted topics.

Review also found that one source line can contain two different topic markers:
`exec/src/lib.rs` carries both `exec-ephemeral` and `exec-review` on one hook.
List that line under every distinct topic it contains. Preserve tabs in the
source text; the previous final field extraction truncated them. Repeated
markers for one topic on a line produce one entry for that topic.

The existing owned-path exclusions, untagged group, and precedence of
`--topics` over `--patch` remain. This inventories source ownership only; it
does not infer upstream ancestry, qualification, or deployment authority.

## Validation

The old script failed the no-remote regression with its upstream-remote error.
Nine real scratch-repository tests now pass using macOS's `/bin/bash`:

- no-remote inventory with owned paths excluded;
- exact older commit, annotated tag, and revision expression selection;
- dirty working-tree edits excluded;
- multiple and repeated markers with source tabs preserved and Git colors on;
- empty success without a synthetic untagged topic;
- invalid and non-commit revisions rejected;
- injected fatal grep failure with partial output suppressed;
- original summary/patch prerequisites and combined-option precedence;
- upstream comparison still working with a local upstream ref, and the actual
  feature-inventory CLI accepting owned hooks and rejecting an unowned hook.

The existing inventory unit test also passes, and `bash -n alab/delta.sh`
accepts the script. The canonical inventory now runs in this checkout:
79 hook topics, 11 feature owners, no ownership errors. The real shared hook
line appears under both of its topics. The output is retained as
`$HOME/.aterm/.ubd/codex-local-topic-inventory-20261112.txt`.

No Rust code, lockfile, remote configuration, or history-dependent gate was
changed. No release or compiler-proof claim is made by these checks.

After composition with remote `4f63d6154a`, the same checks passed again at
`1b5e6bf4d5bb55d8c53cde26e09d26bd6276d76f`: nine CLI regressions, the existing
inventory unit test, shell syntax, and the actual 79-topic inventory. The topic
tooling diff did not overlap the incoming account/quota implementation changes.
