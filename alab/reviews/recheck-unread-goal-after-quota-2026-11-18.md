<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep an unread goal's retry when quota evidence changes

A read failure is enough to defer automatic work. If the original quota
allowance disappears while that read is pending, requiring the same allowance
for the delayed recheck can also retire the work. A recheck keeps the original
native point and policy ownership. It reads the goal and quota again before it
can take any action.

Seven focused tests passed on `b8096ae9fba8422405953a1e10a234d21fdb46ef` with Trust 15698. The native test
creates a fresh entitlement point and lets post-wall quota observations make
the retry due. An actual SQLite decoding error is captured before the
same account's allowance is invalidated. Invalidating that observation does
not emit an evaluation wake. The delayed retry reads the repaired goal and
restores the ordinary future backoff with no provider request. While that
deadline is still in the future, a fresh allowance triggers the production
quota waker and one native continuation of the same goal ID and objective.
Simulated host sleep advances the first timer only.

The test runner, collector, coverage observer, source comparison and command
guardian returned zero. All 12,631 source and tool records match before and
after. Verification covered the focused Autopilot tests.

- Unit log SHA-256 `69881874fa4534f485b3b528643e951b182c002ee7421345718f4616fc3ff042`
- Source record SHA-256 `7b120c0614a05a4ad6f74178296d656c0fa55e59d662ff5418c9b264f24a01f9`
