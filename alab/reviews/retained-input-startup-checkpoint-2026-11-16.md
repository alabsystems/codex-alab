<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retain actual input owners and restore early startup state

This partial checkpoint connects the original request-user-input reply to its
actual queue item before queuing or deferring it. It freezes the answer before
the private AppEvent, carries the same reservation and value through the closed
checkpoint, and adopts the real replacement-local reply before Source binding
when the pending callback genuinely replays. A saved operation whose original
reply has closed can resume protected publication without a new reply.
Receipt-based retirement removes the exact local request rather than a later
request that happens to share a turn or parameters. Publication and retirement
receipts attest Source endpoint delivery and bookkeeping; they do not establish
Core consumption, effect settlement or Native/App permission.

The earliest private retained CLI entry restores the original initial-operation
checkpoint before the original startup futures. The configured startup owner
retains operation and guard custody before spawning; shutdown joins that same
task before closing its controller. The real startup widget owns confirmed input and its original
Escape/edit cancellation. Primary commit invokes the original consumer and does
not reconstruct a cancelled confirmation. The automatic goal hold stays closed
until the existing real primary-runtime and Source commit conditions hold.

## Evidence and remaining work

The 52-path input f273c233006ed00c63930124971e4dc2fcb9a847dc3d16ed4771e137c30e2f6f
on 8b1e6eeb34158c707adecdd239684b438e482b2b passes the combined CLI/TUI/backend/client
all-target check with the native-source-proof feature. The scoped runtime union
passes 569 tests with three existing ignored tests: SDK 86, core 72, ALab TUI 16,
backend 108, client 25, and TUI 262. The eight early-entry controls cover real
provisional UI/selection state, local pre-spawn admission, and original widget,
message and Escape behavior. Missing optional checkpoint fields are rejected;
explicit null remains distinct from an absent field.

The lint build exits successfully, but the official ownership classifier still
reports 125 owned findings, zero spanning/failing/missing-script entries and 15
upstream findings set aside. This is not a clean owned-lint result.

The freshly fetched main 50a63d6d6685a45fdb0ace365025d3edb0d7ed09 is merged as
1fda6c90773a181bc1d6f39b2f4d2412015abd28. Its 23 incoming paths do not overlap
the 52 component paths; all component bytes are verified unchanged after the
merge. Both incoming Rust changes are inside test modules: refresh-rejection
coverage and same-session retry-budget/request-identity coverage. Production
Rust before and after this main integration is unchanged.

The six focused incoming-main Python suites pass 81 tests with no failures,
errors or skips against exact main50a. The independent read-only review finds no
concrete merge blocker. The combined-input incoming Rust
account tests also pass 14 tests (two backend and twelve TUI), including the new
per-read budget control; no failures or ignored tests. The other four packages
in the feature-preserving union select zero tests, which is not extra coverage.

The Rust checks use source-selected Trust 15698 in the unverified component
lane, not a certified application build. The ordinary retained selector remains
inactive. This checkpoint does not complete the live-update goal: the full C1
application/history producer and early C2 graph restoration, generic approval
and delayed-approval continuation, auth/verification callback continuity,
editor handback, full Side lifetime/history,
and the original Cloud conversation worker still need completion. Repeated
execution with genuine signed Native/Source/UI builds, clean owned lint, the
certified application gate, main landing, packaging and installation remain
required. Outside drafts and the separate Native AwaitSettings change are not
part of this Source checkpoint.
