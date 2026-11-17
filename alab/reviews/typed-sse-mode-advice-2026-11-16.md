<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve typed buffered SSE pacing across live mode changes — 2026-11-16

Decision: when an ordinary SSE tail failure is selected in enhanced mode, retain
captured advice from pending RateLimitExceeded, ServerOverloaded and
PolicyCapacityUnavailable as well as generic Retryable. Keep the selected tail
message and ordinary Retryable/coreOther category. Never reemit a pending rate
or capacity category, recapture its clock or alter terminal selection.

The parser creates typed rate/capacity errors with advice even in vanilla mode.
Immediate delivery is enhanced-only, so vanilla buffers those values. Enabling
enhanced mode after buffering makes the tail helper active, but its former
pending-Retryable-only pattern loses the typed advice on idle/network failure.
EOF retains the original typed error and deadline. The owned-only pattern
extension closes that live-mode gap without changing the upstream consumer.
The selected Policy path, completion, EOF, absent advice, newer unadvised failure
and continuing vanilla keep their existing ownership. Expired `Some(0)` stays present.

Tests extend the existing in-memory SSE transport with a second one-shot gate.
The first full frame ends with the SSE delimiter. The locked eventsource parser
returns that frame before polling the tail; its consumer stores the error before
requesting that next chunk. Tail entry signals the caller, which checks the result
queue has no terminal outcome, mode is vanilla and virtual time has not advanced.
Only then does it switch mode and release the tail. Empty barrier bytes create no
event. This proves the tested mode change follows buffering instead of racing parsing.

The request opens vanilla, so no RequestDiagnostic exists. The fixture now checks
logging against the captured opening mode; activation cannot fabricate historical
shape/failure rows. Existing initially-active cases keep their logging assertions.
The guide qualifies logging by opening-time capture. Selected cause/protocol,
body drop, channel close and one encoded custom send remain checked before the
late retained-advice assertion; new mode cases explicitly require zero diagnostic rows.

RED source 8dd84ba2d06e9201f5fd496c95f5db0de3363a9b runs 498 outer methods: 496 passed,
two failed, zero ignored/measured/filtered. The two new positive methods reach
the late missing-advice assertion at line 670. Source order establishes that generic
seven-second and zero-advice healthy controls complete before the first typed rate-limit failure with seven-second advice; those
rows have no separate raw markers. Remaining typed positive rows and exact
retained-deadline/copy checks after the missing-advice assertion are GREEN-only.
The 18 ownership controls complete, including matching EOF typed identity and seven-second advice.
Nested exact-child failures are not additional outer tests.

GREEN source f48862aa25d35fc5aa9c0dfc7d81a1fda919c220 passes 90 public contracts,
271 API units and 137 HTTP-client units: 498 passed, zero failed/ignored/measured/
filtered. All three required methods pass once. The positive matrix covers four
buffered variants with 7-second and zero advice on idle and delayed network tails.
Controls cover three typed categories across EOF, completion, injected Policy,
staying vanilla, missing advice and a newer unadvised error. Tests remain
byte-identical to RED. Scoped all-target Tippy reports zero in all classifications,
including set-aside. At the premerge reviewed candidate, only guide clarification and this ADR follow
that runtime execution.

No upstream file, manifest, lockfile, default or actor policy changes. The existing
enhanced guard remains live, so the fix grants pacing only when enhanced mode is
selected; it introduces no binary-lifetime policy or new retry authority.
Independent source/native reviews retain actual receipt/snapshot and exact-method
evidence under shared Git alab-validation/typed-sse-mode-advice-20261116.
The source-selected Trust 15698 compiler and native frontend/runner execute the explicit
unverified runtime lane. These are paused, in-memory HTTP 200/SSE tests, not real
TCP, actual Core/Autopilot execution or new parent-span preservation. The typed
Policy control is injected, not actual permission revocation. No application
certificate, package or installation is claimed; the recorded application-build
prerequisite and original opaque400 backend cause remain unchanged.

Main advanced to 50cccc460d375c963f699a09f20d2fad14bced9b with a separate HTTPS
recovery change, including owned-core dependency inputs. Preserve all 34
nonshared incoming paths byte-for-byte; the shared guide retains both the peer's
32 added lines and this topic's mode/opening-capture qualification. The owned
helper and tests remain byte-identical to the premerge candidate.
Merged source d6eabd5f74ee42a0e0e080253473d929cf09cc78 reruns the three selected
libraries: 90+271+137, all 498 passed, zero failed/ignored/measured/filtered.
The three required methods pass once and all scoped Tippy classifications,
including set-aside, are zero. Independent reviews bind the composed source and
current native receipts/snapshots. The peer Core/Autopilot suites are outside
this selection. Only this decision record changes after the merged execution.
