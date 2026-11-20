<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Recheck quota recovery after an unread goal

The entitlement retry inspected goal status twice at admission. A failed
first read lost the quota exception. If the second read found UsageLimited,
Autopilot treated that goal as holding the pace and retired the original retry.
Use the same fallible read for the quota exception and the pace decision. An
unread goal takes the existing paced recheck. Confirmed stops and later native
admission checks keep their authority. An independent source review found no
blocker in this change.

Seven focused tests passed on the merged revision `4454b956e1de9d207a2309729a7d6c410247ad87` using
Trust 15698. The new test reaches a due entitlement retry, observes a real
SQLite decoding error, and restores the exact row before releasing admission.
It verifies that the original retry remains available and that the ordinary
timer produces one native provider turn for the same goal. The fixture advances
the timer using a simulated host sleep reading. A healthy Paused goal leaves
both the goal and the provider idle.

Two earlier runs passed the five selected tests but were rejected by coverage
because two filters used the wrong module path. The final run used the actual
libtest paths and observed every expected result. The test runner, collector,
coverage observer, source comparison and command guardian all returned zero.
All 12,601 source inputs and tool records match before and after. Application
certification was outside this focused test run.

- Unit log SHA-256 `a3a412e02931ce0085a89967014f8dd59fbb5f9ce264ea504e2c47c2dd6fb822`
- Source record SHA-256 `b39a8270691917ec63e62123209710beebc993b93e7aeaaf3872ee97b98713d9`
