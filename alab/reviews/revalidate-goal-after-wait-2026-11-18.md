<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Revalidate the goal after its former wait clears

Clearing or editing a goal can leave the native point current while Autopilot
is clearing a displayed wait. A status read before that wait can then report
a goal that has already been cleared. After the wait retires, read the goal
under the native goal-mutation permit. Keep the permit through local notice
insertion and release it before terminal transport. The insertion checks the
same runtime, point, policy and Off epoch. A changed status starts a fresh
decision, and a read or permit failure keeps a delayed recheck. The original
attempt stays refused until that new decision, which reads quota afresh.

Eight focused tests passed on merged revision `77ee0fb255261895c67f2a4030af2bb2ffa8e0c6` with Trust 15698.
The new native fixture displays a wait, gates its clear, and removes the goal
through GoalService while the clear is pending. It verifies the same point,
no stale local or terminal notice, and entry into a timer-driven evaluation.
It holds the evaluation permit to isolate that assertion from provider
submission. An unreadable scratch row proves the delayed recheck, and an
unchanged Blocked row still displays its notice. Other controls cover native
admission, goal retries, quota and wait ownership. Independent source review
confirmed that the final admission path drops its earlier permits first.

The test runner, collector, coverage observer, source comparison and command
guardian returned zero. All 12,627 source and tool records match before and
after. Verification covered the focused Autopilot tests.

- Unit log SHA-256 `9df448f11eb9b22e01050f49e5635b2afce639cc011589a1d880750869ea12f0`
- Source record SHA-256 `9619df83ed14153394f911a3777d571bf2e3a661d11e986a6fa240e0645a4f9c`
