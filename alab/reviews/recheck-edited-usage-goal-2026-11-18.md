<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retry the edited usage goal after an unread follow-up

A goal edit can leave the goal UsageLimited as the original automatic resume
is rejected. The follow-up lookup discarded errors and could leave the
original point with no retry. Retain the point with its existing delayed
recheck when the lookup fails. A confirmed usage stop or eligible wall or model
binding can still be redecided immediately. Confirmed person stops stay quiet,
and the recheck still requires current authority. An independent source review
found no blocker and confirmed that end-of-evaluation bookkeeping still runs.

Seven focused tests passed on the merged revision `7d97d277d4cf4f416a9a6b13a1112d6c4544e268` with Trust 15698.
The extended native test applies a real GoalService edit and waits for the
stale resume to be rejected before injecting a SQLite decoding failure. It
checks that the delay is armed and nothing has been sent to the provider.
Restoring the exact edited row and simulating elapsed host sleep lets the
ordinary timer start one native turn for the same goal and revised objective.
Existing controls cover confirmed stops, ownership and runtime replacement.

The test runner, collector, coverage observer, source comparison and command
guardian all returned zero. All 12,615 source inputs and tool records match
before and after. The focused test run did not certify the application.

- Unit log SHA-256 `e6a7049bef806463afa911fbcd9d861ce09629c20d0a2aeb1dcb1a33811d9ee1`
- Source record SHA-256 `2c3fe109201286daa2f4dfa298da7e0d08e18c097d5e9cb58e22f31f7b1e1ba9`
