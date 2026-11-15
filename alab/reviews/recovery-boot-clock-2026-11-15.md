<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Persisted provider waits use a boot clock — 2026-11-15

The [later native process regression](native-recovery-transition-2026-11-15.md)
adds actual dead-writer import and live timer coverage. Earlier source-bound
clock/history results below retain their original scope.

Decision: retain provider waits on a boot-bound, sleep-inclusive time axis.
Epoch milliseconds label that axis; a later wall-clock correction must not
consume a normalized relative wait across a witnessed restart.

At `94ff294932ec0c01f21ae4f8c9fa20f1783e3129`, the exact test
`recovery::tests::a_forward_wall_clock_step_cannot_shorten_a_restored_provider_wait`
captured a 600-second provider deadline, round-tripped a full typed record,
selected the exact witnessed relaunch and matched its latest failed native turn.
Only 60 seconds passed on the supplied monotonic clock while the supplied wall
sample advanced 1,260 seconds. The current retry switch still denied action
when disabled. With retry enabled, only the final original-deadline assertion
failed: elapsed wall time had incorrectly spent the remaining provider wait.

RED session `11961` ran one failed test in 0.06 seconds, with 972 filtered and no
ignored/measured tests. Unit exit was 101; the expected-RED controller exited zero.
All 18 private archive hashes were independently verified; record SHA-256:
`81657ccb284ace2c8de0df6fae65fa8f18775a3f1367c0b975ef6ef45c0ad1ec`.

`alab-core::recovery_clock` binds an anchor to the current Launch's native boot
identity and the existing kernel counter that includes host sleep. Capture
reads Instant before the boot sample and rounds elapsed milliseconds upwards;
restore samples the boot clock before Instant and subtracts exact nanoseconds.
Failed-point time, provider advice and saved lower bounds use the same axis.
Checked arithmetic and timer headroom reject unrepresentable bounds. Reusing a
compatible same-thread/turn origin avoids new-anchor write churn.

Both Recovery and InFlightRetry carry the typed optional anchor. The TUI codec
keeps the enclosing wait when a clock is missing, legacy, malformed or unknown.
Conversions carry the coordinates; writer time never becomes their origin.
Both import paths normalize against this process's current Launch identity
outside native-lock seed closures. An unusable clock remains a known wait with
`clock=None`; saturated timestamps cannot become verified already-due bounds.

Unmatched history grants nothing new. A matching unverifiable wait installs a
Fatal point and a turn-bound refusal marker, clears carried/owed action and is
handled without a generic unpaced fallback. Only Restored may rearm route or
quota recovery. Exact native turn, witnessed relaunch, current policy, input
admission and ordinary carry-on bounds still govern every action.
Unverifiable timing facts survive another crash, ordinary intent retirement and
Off; Off still blocks automatic action and clears usable recovery intent.
Rereading the same terminal failure retains refusal; running/interrupted teardown
retains its wait. Genuine completion or superseding native work releases an
obsolete refusal through the existing native lifecycle.

The model-goal peer introduced another continuation entry point. Composition
moves the unchanged refusal check to the first common final guard, before
upgrade/policy overlays. Its existing decision test calls actual `refuse_clock`
and requires Attention from the goal continuation helper. No live bypass was
observed; the common guard makes the entry-point contract explicit. Peer quota
policy, including its explicit 24-hour horizon, and the goal hook remain unchanged
from their accepted sources, including goal peer `afb3a00a0c81e185e823df760dff294b2526d90e`.
The clock layer adds no unsafe call, dependency, lock/pin change or upstream hook.

Runtime evidence uses source-selected Trust 15698 explicit unverified execution.
Historical `62c30a1b63305546c9760420578e487f9fc45b7b` passed 1,952 tests with 12
ignores, but ten owned lint diagnostics left its controller at exit one; that
attempt was incomplete qualification. The distinct descriptor leak, its RED and
callback repair are recorded in the [runtime ownership review](supervisor-runtime-ownership-2026-11-15.md).

At `e3758046432f67e032b5dbaa66062ca8ab48c835`, session `97555` completed with
unit, lint and controller exits zero. Five full libraries passed 1,969 tests:
Aterm 223, Autopilot 998, Core 291, TUI 383 and Exec 74. The 12 exact declared
ignores remain separate; all 41 required identities passed once, with no failures,
measured or filtered tests. Seven executable identities and both host aliases
matched the recorded runtime evidence and the during-run binary hash sample.
Six-package all-target Tippy completed successfully with 1,469 unique canonical
artifacts across its compiled graph, normal/test profiles for all six packages,
no owned/spanning/failing/script diagnostics and four diagnostics set aside.
Goal metadata declares no unit harness; compiled profiles are not a Goal runtime pass.
All 35 immutable archive hashes were independently verified; record SHA-256:
`2f0c2462d3491db8ee566ca62a19745e2235d8348925460a9289277dc8327713`.
Archive: `.git/alab-validation/e3758046432f67e032b5dbaa66062ca8ab48c835/recovery-boot-clock`.

The later tested composition `acd8435633ae8ef7af26437b571203eae8264d0a` includes peer
`209e1b33e189a8cf5de682810dfc241ef4b24622` guarded Pace publication and resume-race
changes. It retains the first common refusal guard and conditional intent clearing;
resume restoration prepares its boot sample outside the held thread/native
boundary, then uses a later Instant conservatively. Existing anchor capture and
native/current-policy authority remain intact.
Session `12107` completed with unit, lint and controller exits zero. With the same
five-package graph, 16 filters selected 478 passes and zero ignored tests:
Aterm 7/219 filtered, Autopilot 296/711, Core 69/226 and TUI 106/286. Exec selected
zero/74 filtered and makes no executed-test pass claim. All 29 required identities
passed once; all 16 filters selected tests. Five record-mode receipts, both host
aliases and all seven current executable hashes matched the post-runtime sample.
Six-package all-target Tippy again produced 1,469 canonical artifacts across its
graph, all six normal/test profiles, successful build, zero owned/spanning/failing/
script diagnostics and four set aside. All 33 immutable archive hashes were
independently verified; record SHA-256:
`e1d16c04b5b9b751a316d6c0d6b801240666419ae7ea617a1ed4705b2e6a2e47`.
Archive: `.git/alab-validation/acd8435633ae8ef7af26437b571203eae8264d0a/recovery-latest-composition`.
The full 1,969 passes remain evidence for e375; acd has the distinct focused result.

At acd, the changed publication Python modules separately passed 26 mock/local-Git
tests in 10.409 seconds, exit zero, with no skips. The three indexed archive files
were independently verified; record SHA-256:
`b3f6c88382da2776093fbf807d9bdb7ae6cd6ff8bf8aebca5a1bd11399ef366e`.
This establishes no network publication. Inventory remains 88 topics/14 features.

Publication also includes source composition
`36fb3144fd3a0d9d87ca8f0ca0efd1ff50cf3771` with main
`55e23f738049693b16f02fd2ff266e4c4c27ac3d`. Independent review verified unchanged
clock/refusal/retention/runtime-owner/publisher/pin blobs and the native test body.
The fixture constructor keeps the peer's hooks support. Peer cancellation,
startup-routing and compiler-probe changes retain their own validation sources.
This final composition received static review; the executed results above remain
at e375 and acd. No additional runtime result is implied.

The regression supplies native-history, clock and witness inputs to real code;
it changes no host clock and proves no accepted external actor or actual
application crash/relaunch. The backend's original HTTP 400 cause remains unknown.
Raw receipts remain private. Runtime/lint qualification supplies no application
certificate or installed validation. Application certification remains deferred
at Trust 15698's macro-admission boundary, source `24b01cc6a552f3066e3b87ccf9b34ff7237a9abb`.
The November 15 read-only observation of managed compiler 20882 found the same
relevant restriction; it was source evidence, not a new executed gate result.
No compiler substitution, installation, update or restart is claimed.
