<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep an edited goal's redecision when quota evidence changes

An automatic resume that loses its goal snapshot can still owe a new decision
for the same native point. Its original quota allowance is a dispatch input.
If that observation expires during the rejected-resume path, it must not
cancel the new decision. Use the same retry authority before and after the
follow-up read and when scheduling. It keeps native point and policy
ownership. Confirmed stops remain quiet and the new decision reads quota
again. Independent source review covered all three guards.

Seven focused tests passed on merged revision `7c3fe6a3682846af114cdc76f7543530eefa2411` with Trust 15698.
The native fixture makes an early resume due through quota, edits the
objective through GoalService and reaches the actual rejection. It invalidates
the same account's quota after the healthy follow-up goal read, directly
exercising the scheduling check. The timer restores the original future reset
without spending a try or sending a provider request. A fresh observation
then wakes one native continuation before that reset, with the same goal ID
and edited objective. The existing controls also cover explicit goal stops
and replaced runtime or binding ownership.

The test runner, collector, coverage observer, source comparison and command
guardian returned zero. All 12,635 source and tool records match before and
after. Verification covered the focused Autopilot tests.

- Unit log SHA-256 `bd0016a044c3482451ec44925fc0582de8ae97f7b964c161b4a135db10a3b095`
- Source record SHA-256 `2d886632da38c8b5d18089ccd12b86d84ab5db6e11f5bcc77e767f262259baa5`
