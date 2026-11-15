<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Publication cancellation test deadlines

The broader Autopilot library run after the opaque-400 recovery changes had
914 passing tests, four ignored tests, and one failure in
`a_point_left_to_a_person_is_raised_again_when_acting_resumes`.
The cancellation assertion received a completed evaluation instead of a
cancelled task. The test passed in isolation, but adding a controlled pause
longer than the five-second client deadline reproduced that same assertion.

The fixture held a real metadata request behind a manual fake-server gate
while retaining the client's production timeout. Use the existing one-minute
gated-test timeout so cancellation can be tested independently of that
transport deadline. Do not change production timeout or lease behavior.

A delayed experiment with that change exposed the distinct, correct server
outcome: its five-second publication lease expires, so no act is recorded.
Require an explicit expired-lease diagnostic before accepting that outcome;
otherwise require the original single recorded act. In either case, native
turn admission waits for publication settlement, and another evaluation
cannot publish the old point over the running turn.

Add an explicit delayed cancellation case to retain coverage of lease expiry,
alongside the original acknowledged-write, cancelled-write, other-root, and
other-registry cases. This tests both protocol outcomes without assuming that
the scheduler always resumes the test within the server's lease.

Validation: the complete Autopilot library suite passed with 915 tests and
four existing ignored tests using source-selected Trust's unverified test
lane. Focused Tippy over the library and tests reported no ALab diagnostics;
its three upstream `fetch_update` deprecation warnings are unchanged. No
application binary was built or installed by this test-only change.
