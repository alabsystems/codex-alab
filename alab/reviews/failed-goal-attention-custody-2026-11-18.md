<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep a goal failure notice with the point that failed

A failed resume clears any displayed wait before publishing its failure. A
newer accepted turn can replace that point during the clear, even on the same
registered runtime. Check the accepted turn and point under the registration
lock. Check policy, acting state and Off under the supervisor lock that adds
the notice. Transport starts after those locks are released. A current
failure can still report and schedule a fresh retry when the earlier quota
observation has aged out. An independent review confirmed the lock ordering.

Eight focused tests passed on merged revision `214907f9c878c2b326bed8d2e6f080f75f128294` with Trust 15698.
The extended fixture displays a wait, forces a real GoalService refusal and
holds the fake host's wait-clear request before the failure is inserted. The
same runtime accepts and ends a newer turn through its lifecycle hooks while
the clear is pending. The old failure adds neither a desired notice nor a
transport notice or retry. The unchanged-point control displays the failure
and arms the retry. Running these hooks in the fixture does not submit a new
provider turn. Existing coverage includes runtime replacement, quota loss and
wait transport sequencing.

The test runner, collector, coverage observer, source comparison and command
guardian returned zero. All 12,622 source inputs and tool records match before
and after. Verification covered the focused Autopilot tests.

- Unit log SHA-256 `20a90c59a81e4c040b664b51e2b5a75dfc7e9783fc7aa890543c1e90b4a19f84`
- Source record SHA-256 `d7ba7980123bfff12fb132e18ec0f066dae9efe1c093d0fdfa586b631c95bbf4`
