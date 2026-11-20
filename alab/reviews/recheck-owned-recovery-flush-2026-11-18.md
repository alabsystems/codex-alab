<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep recovery write retries with the native point that owns them

A failed provider-recovery write can finish after a newer native turn has
already canceled the old timer. The held-wall and ordinary recovery paths
now check the original runtime, native point, and policy before arming
another retry. An unchanged point keeps the five-second delay and reevaluates
with fresh quota evidence.

The paired regression starts a real Core turn that reaches a usage limit and
injects a definitive failure for that turn's recovery flush. One case starts a
new Core turn and observes its provider request before the flush returns. The
other keeps the original point and measures one readiness timer with the
existing delay and no automatic dispatch. The test exercises the ordinary
recovery write. The held-wall caller received source review.

On baseline `78ed9b411c7535cfc8cafb88a23ec7e3041c9e22`, the stale case failed
because the old flush advanced the timer generation from 1 to 2. The
current-point case passed. The positive coverage observer rejected the
printed child harness nested inside the failing test's diagnostic. The
maintained collector identified just the stale case, and inspection
confirmed the two outer test results and the exact failing assertion.

Both cases and five controls passed on merged revision
`a302a762bafc877a716e3247936e3c8d817f4227` with Trust 15698. The test file was
identical in both runs. The fixed run's test runner, result collector,
coverage observer, source comparison, and command guardian all returned
zero. All 12,644 recorded source and tool entries match before and after
each run. Verification covered the focused unit tests.

- Baseline log SHA-256 `bc35e6b226bb7d1e803dd281b448b619f0691fa8e88a1d43d46f71885b220f1e`
- Fixed log SHA-256 `ba8d745df21adcc4e8fa6971a2f896a94f827cd86aa36fbff6e83c02542d4d23`
- Fixed source record SHA-256 `fb6eccd7bc76a2ac32420edd5218c6fd4ba6f3c19b7c9cbe909e81e92c337023`
