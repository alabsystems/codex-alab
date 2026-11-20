<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve the first model stop through a failed chronology read

An exact model-tool transition from Active to Blocked can commit before
Autopilot reads the current chronology. If that read fails, Autopilot now
persists the newly observed source as passive pending provenance. Resume
still requires fresh proof of the same goal and chronology. A later host
hold, a changed generation and a duplicate callback with unreadable
chronology retain their existing protections. An independent source review
confirmed the new branch only admits exact fresh callbacks.

Six focused Autopilot tests passed on `7d297c455047eae5312b2b681d548cd92e5b9d83` with Trust 15698.
The new test injects the failure in the native model-tool callback and reads
its persisted source before the tool output is released. It reconstructs the
goal service and Autopilot runtime from the fixture and supplies that saved
source through the carry path. With matching chronology it observes exactly
one original-goal continuation. A real intervening host hold rejects the
saved source even when the final goal fields match. The fixture exercises
reconstruction within the test process. The existing restart contention
case separately exercises an actual held journal lock.

The maintained test runner, collector, coverage observer, source comparison
and command guardian all returned zero. The 12,595 source inputs and tool
records match before and after. The test lane did not run application
certification.

- Unit log SHA-256 `2879da23dfd455697ec408a5639b3acdc1788e0ac3e1b4df88faf46b5264cce8`
- Source record SHA-256 `f08b79202cc302cdecf29620f2964eca53b77ad4e5ca1bb62409364fcd1b089b`
