<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep a Plan refusal notice with its native point

Checking Plan mode can leave an automatic submission refused while the prior
wait clears. A newer turn can start and finish on the same runtime before the
clear returns. Attach the Plan notice using the original point, policy and Off
epoch at local insertion. The same guard applies to a final Core PlanMode
rejection. Both still refuse the automatic submission and preserve the
existing go-ahead restoration. The decision that still owns its point can
show the person's Plan notice.

Seven focused tests passed on `ed673881e00f37e31f75555a42e6de936f0fd4f1` with Trust 15698. The native fixture
sets actual Core Plan mode, displays a wait and gates its clear during the
early refusal. Direct native start and stop hooks register a successor point
on the same runtime without submitting provider work. After the clear, the
stale notice stays absent from both desired state and the terminal, and the
successor's exact point and turn ID remain. The unchanged-point control
raises the notice, and the original test still verifies Default-mode dispatch.
The late Core PlanMode callsite was checked in source review. The regression
exercises the early refusal.

The test runner, collector, coverage observer, source comparison and command
guardian returned zero. All 12,630 source and tool records match before and
after. Verification covered the focused Autopilot tests.

- Unit log SHA-256 `c46f58626fff64c58a2acb18230586de64cba5b9572f8a711b2aa22377d567bc`
- Source record SHA-256 `8aa45e1b9a306a8c74e4356a49369dbaa0a38909889884d5bc36f9d55318bb82`
