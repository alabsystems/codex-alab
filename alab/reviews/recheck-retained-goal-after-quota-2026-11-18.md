<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep the readiness retry after retained goal admission refuses

A retained goal admission refusal orders automatic work to wait. If the
quota observation used by that attempt has since expired, the same native
point still needs a fresh decision. Use the existing recheck guard at the
runtime-backed refusal and the direct retained-gate refusal. The guard keeps
the point, policy, and runtime ownership checks. Both refusals still return
before dispatch, and final admission still checks current quota authority.

Seven focused tests passed with Trust 15698 on merged revision
`9a7e79f6d20a27be7c56e51a81c1b4dd1d5cf8ba`. The new test binds a held gate
to the real thread manager before Core starts. It stages a future usage reset
and account binding, then invalidates a usable quota observation while
submission is paused before admission. The real goal runtime refuses the
submission and Autopilot arms one readiness timer. The timer restores the
original future reset, adjusted for simulated host sleep. The gate stays held,
the goal database stays empty, and no native turn or provider request starts.

The selected controls covered a stopped goal, human input during goal
mutation, a missing goal runtime, and a newer native point. The direct
retained-only fallback received source review. The runner, result collector,
coverage observer, source comparison, and command guardian all returned zero.
All 12,639 recorded source and tool entries match before and after the run.
Verification covered the focused unit tests.

- Unit log SHA-256 `f4a8877bc14e42ecb9626c908586e05dffa4293da003e3fa0d1e9d0621d5a381`
- Source record SHA-256 `8e85a301d63f8a0e45c8efc7997a82172d14a7faf18230e6c807690fa89957e3`
