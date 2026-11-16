<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Exercise simulated pidfd exit success and early-exit refusal

The widest-check follow-up distinguishes declared ignores and unavailable lanes
from the repaired historical cleanup failures. One available test gap remained:
the portable exit watcher's simulated pidfd controls exercised invalid events,
open/registration errors and close failures, but no valid exit notification.

Decision: add two test methods in the existing observer-errors suite, without
changing the watcher or production guardian. Each covers POLLIN, POLLHUP and
their combination. A pending constructor and pending observation precede the
valid event. Repeated observations before and after close retain that exit fact;
the fake poller's exhausted frames make an extra poll fail. Closing twice must
invoke the fake native close exactly once. An initially ready pidfd must instead
refuse the constructor's live-original readiness contract and close exactly once.
All descriptor operations stay inside the existing fake module OS namespace.

On base `c8339b136d81e8ff603f1632d10fc25868193a4a` plus this test-only patch,
managed Python `-I -B` passes all 15 observer methods in 0.731s and the 47
existing guardian, lifeline, post-fork and receipt/result methods in 1.314s.
Both invocations fully exit zero, with no errors or skips; regression handle
`56502` was retained through its terminal result. Logs:

- `<evidence>/codex-pidfd-exit-observer.log`
- `<evidence>/codex-pidfd-exit-regressions.log`

Syntax and `git diff --check` pass. An independent read-only reviewer found no
blocker in the exact two-method patch. These are simulated Linux API controls
and existing macOS process regressions, not native Linux execution or a new
application, certification, permission or full-gate result. Existing legitimate
ignores and proof/permission deferrals are unchanged.
