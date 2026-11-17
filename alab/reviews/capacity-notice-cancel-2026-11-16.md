<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Capacity cancellation before notice delivery — 2026-11-16

## Decision and reason

Exercise the actual Core capacity retry handler while its notification is still
blocked by Session's real realtime-history delivery lock. Capacity creates both
an in-flight pacing guard and a live presentation guard before awaiting that
delivery. Existing ordinary-retry cancellation releases the notification first;
existing capacity steering waits for a delivered notice. Neither demonstrated
retirement of both capacity guards at this earlier cancellation boundary.

Extend only the existing ALab-owned wake-test module, reusing its exact-child
isolation, bounded collector and runtime-drop-before-marker gate. The nine
ordinary case bodies and isolated-launch/collector contract remain unchanged;
the child dispatch adds explicit selectors. Two new outer methods
declare four private cases: cancellation and normal release for both server
overload and policy-capacity refusal. These are source/control-flow cases, not
independently measured process or wire-request totals. No production defect or
failing production baseline is claimed.

## Observable contract

Each case calls the real `handle_response_stream_error`, applies production
`preempt_for_retry` selection, and retains the caller's independent outer turn
cancellation guard. While notice delivery is held, polling must establish both
actual pacing and typed live-capacity publication with an empty real receiver.
Cancelling the representative steering token must leave this protected future
pending. This tests token selection, not full sampler input admission.

For cancellation, cancel the turn and consume/drop the owning future while the
delivery lock remains held. Require a cancelled outcome, pacing None and live
publication None before time advances or delivery unlocks. The same captured
predeadline clock is used for live checks, so expiry cannot masquerade as
retirement. Retained `SamplingRetryAdvice` is a separate fact and is not required
to disappear.

The positive control releases delivery, receives the exact notice, type and
details, and completes a positive virtual wait before checking retirement.
Both paths then await real Session warning-event barriers after unlock and past
the original advice deadline. Exact events and an otherwise empty live receiver
check responsiveness and absence of a delayed capacity notice. No physical
sleep or yield-only absence oracle is used for that conclusion.

## Qualification

Initial source `0b0916ab40cf7d230cb081ceafefa78d56710d77` passed all 12 selected
methods but failed the owned lint gate: the new helper's lint expectation was
unfulfilled. The Tippy compiler exited 0; the classifier correctly kept this
owned finding gating. Remove only that four-line annotation. Every function
body and the original ordinary helper's expectation remain byte-for-byte
unchanged. The initial run remains unqualified historical evidence.

Clean source `e9a3718c58d99bcd3ce47be019c758bf753452f3` passed all 12 focused
methods once, including both new outer methods, with zero failures, ignored or
measured tests. The filter excluded 3,261 other Core tests; this is not a full
Core suite. Scoped all-target Core Tippy had no owned, spanning, failing or
missing-script findings. Eight upstream warnings were set aside by the
maintained classifier. Unit, compiler and controller statuses were 0.

The qualified project frontend selected committed Trust 15698. Tests used the
explicit unverified library lane, one ordinary runtime round and one test
thread. Maintained preparation supplied source-matching V8 data and real native
stdio/code-mode-host helper bindings. Source and target stayed frozen through
capture and independent qualification of the Core image, native runner and two
prepared support images with three aliases. Support-image binding is not launch
evidence; V8 archive and binding are pinned build-input data. Receipts, diagnostic
identities, both Core lint profiles and 1,382 canonical artifact paths matched.
The private `capacity-notice-cancel-20261116` stage retains raw records for each
phase separately; repeated method counts are not added together.

Before publication, the expected-main guard stopped when incoming main
`a34e913c9b5b3036dc203d30af6e98abe912a6b3` changed the selected quota-recovery
dependency. Preserve its 19 incoming paths exactly and compose source
`a7c166244451912d96586eb9f3d8d2d1ddbd9b21`. The owned test and review bytes were
unchanged by this merge. Requalify the combined graph rather than transfer the
older native result to the changed dependency.

The separate `native-core-composed` phase passed the same 12 focused methods
once, with 3,261 filtered out. Scoped Core Tippy again had zero gating findings
and eight upstream warnings set aside; unit, compiler and controller statuses
were 0. Independent qualification verified the fresh four bound images, three
aliases, V8 inputs, both Core lint profiles and all 1,382 canonical artifact
paths while source and target remained frozen. The original ownership baseline
`bb5f50ba3023f630ab49778d96971889f6789d32` and the new qualification baseline remain
separate immutable records. This does not execute or qualify the peer's
standalone quota, TUI or smoke suites. Preserve the earlier phases and their
reviews; counts are not summed across runs.

## Limits and publication

The owned change is limited to tests and this review record. This does not qualify actual provider behavior, the full sampler actor
or a packaged application. The original opaque HTTP400 cause remains unknown;
the certified application-build prerequisite and human hold remain unchanged.

Publish through the maintained expected-main guard, preserve disjoint already-main
work exactly, and retain the qualified source/filter attribution through any
composition. Phase context and ownership baseline are immutable separate records;
the active source must match them before cleanup. Independent source, control,
artifact and composition reviews plus complete unbounded quiescence precede
source/ref cleanup. Preserve other branches, the pre-existing peer and private
evidence/handoff; do not substitute an unverified application delivery.
