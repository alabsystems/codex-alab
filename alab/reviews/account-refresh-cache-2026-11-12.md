<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep the last account observations visible after a failed refresh

## Finding and decision

At `59c750fca6`, the account fetch path converts success and failure to the
same text update. A current refresh failure therefore removes every row and
observation time from a previously successful account view.

Keep success and failure distinct through the local snapshot event. After a
successful read, retain that snapshot separately from the rendered failure
notice. A later failed refresh shows its sanitized error and explicitly labels
all retained account information as potentially out of date. Without a prior
success, show only the error. Repeated failures replace the notice; a successful
refresh replaces the saved observations and clears it.

The account design keeps prior observations visible while refreshing, and
the adjacent account/status interfaces retain cached observations during
connection failures. This change extends that behavior to the subscription
pool view without making cached rows authoritative for account selection.

## Context boundary

Check the snapshot token before changing either the display or the saved
observations. An assignment observation clears the saved snapshot and pending
token. A failed refresh after that boundary must not restore pre-assignment
rows. A new view starts without any saved observations, and the existing thread
and weak-view guards still apply. Selection and enrollment retain their native
preflight and authorization paths.

The regression cases use the actual snapshot scheduler, local events, and
account pane with controlled fetch results. They cover a failed refresh,
repeated errors, recovery, initial failure, and assignment invalidation, while
retaining the previous ordering, duplicate, pane, thread, and draft checks.
The ordering checks compare the entire pane after stale replies, so a stale
error cannot append a warning while leaving the newer rows present.

## Regression evidence

With the saved snapshot deliberately discarded on an error, the new test
failed because the previously displayed account row disappeared. The
counterexample log is
`$HOME/.aterm/.ubd/codex-account-cache-discard-20261112.log`.
That temporary mutation was removed for final validation.

All 32 account-details tests passed with no failures or ignored cases,
including the strengthened whole-pane ordering checks. The final log is
`$HOME/.aterm/.ubd/codex-account-cache-final-20261112.log`.
The run used Trust's unverified lane with `--locked --offline`, the repository's
8 MiB test stack, and four test threads. Formatting and whitespace checks pass.

Independent review confirmed that only successful reads populate the saved
snapshot, failed reads retain only sanitized errors, and observations clear
the cache before another context can use it. All changes are in ALab-owned
paths; no upstream hook or lockfile entry changed. The tests use controlled
fetch results and do not authenticate or change a real account.

Remote main advanced to `f8ebf23aa3` before publication with terminal-custody
changes. The guarded publish refused the stale base without writing. The cache
change was rebased without conflicts and its account-pane suite is rerun on
that combined source before publication.

All 32 account-details tests passed again on the combined source, with no
failures or ignored cases. The retained log is
`$HOME/.aterm/.ubd/codex-account-cache-integrated-20261112.log`.

A second publication race advanced main to `249c52d1a2` with Cargo checkout
attribution and additional upgrade tests. The rebase was clean. A direct Git
comparison confirmed that the tested account implementation, account tests,
assignment integration, account detail view, and event declarations remained
byte-identical after that rebase. Those disjoint additions do not require
repeating the account test run; this record makes no full-tree qualification
claim for them.
