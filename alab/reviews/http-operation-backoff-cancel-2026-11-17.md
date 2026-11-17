<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Cancellation after a completed HTTP failure — 2026-11-17

Decision: add one expected-pass public contract for cancellation during positive
HTTP retry advice. Earlier coverage cancels before the first callback. This test
protects a completed 503 callback and operation INFO record while recovery sleeps.
No production change, defect or RED is claimed.

One strict isolated child cancels first, then runs a fresh live control on the
same retained client. Each creates ten-second typed advice through the existing
public debug constructor `from_delay_with_sleep_reading(10s, None)` and copies
its actual deadline before handoff. Raw `Retry-After: 7` stays contradictory.
The unavailable reading fixes the virtual-clock premise without host clock
reads or a platform sleep/wake claim.

Releasing the first gate and manually polling observes Pending without advancing
Tokio time. The copied deadline remains future; the original typed 503 callback
and one operation INFO row already exist. The owning future drops synchronously
under ambient NoSubscriber/current-span None. Beyond the deadline and four runnable
turns, its request, callback and diagnostic snapshot stay unchanged, with the
success continuation unused. Existing context, privacy, safe-ID and payload
oracles remain in force.

The retained client then receives fresh advice and a distinct request identity.
Its live call stays Pending one millisecond before that copied deadline, then
recovers after the deadline with exact stream events and channel closure. Success
clears failure, advice and IDs; the cancelled capture stays unchanged. The
cancellation observations are bounded; they do not establish permanent inactivity,
physical pacing, or permit/subscriber retirement.

Initial `e31d7ae2` inputs and reviews used a deadline-minus-one-second poll; they
were sealed unexecuted in snapshot `d8e6586b…`. A shortened 9.5-second wait could
escape that oracle. The one-constant repair recorded in `5732311b…` moves the
poll to deadline-minus-one-millisecond. That preparation produced no runtime results to transfer.

Qualified runtime `af1f14c86e0faf72d132989d4803892d3206e422` passes all 397
methods: 112 public and 285 API, with zero failed, ignored, measured or filtered.
The required outer method passes exactly once. Scoped all-target Tippy exits 0
with 657 canonical artifact paths, both packages' normal/test profiles, compiler
exit 0 and all gating findings zero. Independent accepted qualification SHA256 is
`04d047951e8213e1b0ea20d399a67a468a9c0c503589a71a9d07636ebcd04c0d`;
native capture SHA256 is
`a2ace6f56ce9bd7a91508ddac2eb150fbbcd50af9972672b09fb10a10c8cdcc7`.
The prior 396 methods supply baseline identities only; their passes are not
transferred. Strict child completion and source flow establish the two logical
calls, without retained per-call transcripts or additional outer counts.

This ADR is the sole topic change after that qualified runtime; the test path
remains byte-identical. The topic changes two paths against base `081b4a0d`.
No publication or future composition is credited. No Python, Core, provider or
application qualification is claimed. Certification/adoption and human holds
remain unchanged; the original opaque HTTP400 cause is unknown.
