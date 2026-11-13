<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Continue observing account reconciliation

## Finding and decision

At `1ab704c65a`, the account pane stops observing an accepted assignment
when its progress becomes `Reconciling`. The native coordinator uses that
state while retrying transient failures; it retains the same operation and
continues until a terminal result. The pane can therefore remain on its
paused message after the assignment has succeeded.

The mismatch is between `alab-tui/glue/assignment_pool.rs` and the retry
loops in `alab-core/glue/assignment_coordinator.rs`, both under `codex-rs/`.

Continue observing `Reconciling` for that exact operation. Stop only for
`Applied`, `Cancelled`, `Refused`, or disposal of the view. Keep the current
paused wording while reconciliation is actually in progress. This changes
presentation observation, not assignment authorization, retry ownership,
or the accepted operation itself.

The regression exercises the production observation loop with a controlled
consumer and the real account pane. It must show reconciliation followed
by success without submitting another account request or model operation.
Closing the view must still end its observer without cancelling the
coordinator's work.

## Regression evidence

The new observation regression failed with the original terminal-state
match: no later account observation arrived. Its log is
`$HOME/.aterm/.ubd/codex-assignment-reconciliation-before-20261112.log`.
The final implementation removes only `Reconciling` from that match.
The controlled consumer first presents another operation's result, then
reconciliation and application for the requested operation. The test
applies the actual local account events to the pane and checks its text,
preserved draft, single accepted request, and lack of model operations.
The separate close case uses a paused clock to avoid racing the next poll.

With the corrected match, both new cases and all 25 tests in the account
details group passed, with no failures or ignored cases. The log is
`$HOME/.aterm/.ubd/codex-assignment-reconciliation-after-20261112.log`.

Remote main then advanced to `737cc4a267` with terminal-mode custody work.
This account change is integrated with that source before final validation.

The same 25 account-details tests passed again on the combined source, with
no failures or ignored cases. The final log is
`$HOME/.aterm/.ubd/codex-assignment-reconciliation-integrated-20261112.log`.
Both runs used Trust's unverified lane, `--locked --offline`, the existing
8 MiB test stack, and four test threads. Independent review found no change
to operation ownership or lifecycle guards. These tests use a controlled
consumer; they do not change or authenticate a real account.

All implementation and regression changes are in ALab-owned paths. No
upstream hook or lockfile entry changed.

## Review context

GitHub issue discovery was unavailable: Agh's issue-list read returned
HTTP 403 with `issues=read` required. No alternate identity or transport
was used. Local source review found the reconciliation defect and work
continued independently of that permission.
