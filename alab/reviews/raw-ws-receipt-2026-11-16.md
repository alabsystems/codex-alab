<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# One receipt for raw WebSocket fallback advice — 2026-11-16

Decision: gather all raw Retry-After candidates within one priority tier before
calling the existing shared parser once. Valid nested advice still wins over
envelope advice, including Some(0). Invalid containers cannot erase collected
values. No usable nested advice permits the existing envelope fallback.

The malformed-capacity route previously captured each field separately, then
each header container separately. Its three max_by_key reductions read dynamic
wake-adjusted deadlines. Later advice could acquire a later sleep baseline and
remain pending after the earlier receipt's wait had expired. The ordinary
HeaderMap parser already provides one common receipt for repeated fields; the
raw wrapper now supplies it one complete candidate set instead of singleton maps.
Only converter-accepted Retry-After values are appended. Non-advice fields are
ignored, malformed/unsupported values stay unrecognized, and empty sets do not
call the factory. All dynamic per-field/container comparisons are removed.

Strict parsing, equal-status-alias normalization, explicit-status recovery and
the permanent-status/capacity guard retain their order and meaning. This route
still produces the same bounded Retryable message, Stream/Other bridge, no HTTP
identity and no unbounded capacity authority. No upstream file, public clock API,
feature, dependency, manifest, lockfile, default or request serialization changes.

A private production-used parse_for_stream_with_advice seam defaults to the real
RetryAfter::from_headers. The RED retains the complete old selection algorithm.
Tests call that real shared parser, extract its original parsed deadline using
at_reading(None,T), then rebind only the receipt reading through the existing
debug constructor. The first accepted result gets MAX-120 seconds; later accepted
results get MAX. Invalid/non-advice results do not consume a capture. Those
synthetic totals exceed native counters, so old intermediate native deadline()
reads cannot normalize them. Final evaluation explicitly supplies MAX.

Every positive starts with a healthy single 7 case, then raw 3/7 duplicates at its
own layer. Actual raw classification, fixed message, original parsed duration,
bridge category, no invented HTTP status, retryability and exact copied/mapped
advice are checked before the final deadline assertion. With the old algorithm,
the selected 7 has the later origin and is still future; one accepted common
origin makes it due. Assertions use <=T rather than subtracting from a possibly
small platform Instant. There is no callback-count-only oracle or copied parser.

Four independent positive methods cover nested fields, nested header/error
containers, envelope fields and envelope containers. Both capacity codes run in
GREEN. The full matrix has 35 receipt-behavior rows and nine strict/recovered,
ignored or inactive bypass checks. Controls cover latest/reversed hints,
invalid/overflow values, nested 3/zero/past date ahead of envelope 900, invalid
nested fallback, all-invalid absence, and a whole comma-bearing HTTP date.
Only the new synthetic helpers, wrappers and child arms are debug-assertions
fixtures, matching the existing dependency constructor's availability. Production
factoring and every old release-routing test remain unconditional. No release
synthetic-clock execution or new release compilation proof is claimed.

RED source 0e7b53f19638b2a5db0fa0d0b95ecb9fb6377bbd runs 877 outer methods: 873 pass,
four fail, zero ignored/measured/filtered. Each positive prints its healthy
single 7 complete marker and then the first server-overloaded 3/7 late_deadline
marker before the expiry assertion at line 803. Later policy/nested-error rows
are GREEN-only. The control method passes. Four child failures are not extra
outer methods. Raw outer results are 93 public, 371 owned core, 272/4 API, 137 HTTP.

GREEN source d3288f6236fac5de21a7a2f1bec5f0ab4551d8bb includes production
0599c0e4e58acb762bc795c5408e2dbb05cc0d82 and main
2c14ca322044b15822eded161e0db8dcb988f520. Tests are byte-identical to RED.
All 877 outer methods pass: 93+371+276+137, zero failed/ignored/measured/filtered;
five required methods each pass once. Every scoped all-target Tippy category,
including set-aside, is zero. Both phases use source-selected Trust 15698 and the
native frontend/runner in the explicit unverified test lane. Independent native
observations retain all four preparation snapshots, current hashes, receipts
and diagnostic identities for each phase. Successful child markers are drained;
complete GREEN row reach follows each passing method and its loop control flow.

All eight concurrent peer paths remain byte-exact. Their focused CLI/promotion/
version fixture checks pass 10+5+8, with each actual status 0 recorded. Those are
local Git/synthetic failure controls, not a network or package-promotion claim.
Only this decision record follows the final Rust execution.

The new evidence proves deterministic raw-parser receipt consistency. It does
not prove physical host sleep, wire-arrival timing, actual socket/Core/Autopilot
retry execution or application delivery. The capture point is the shared parser
boundary after candidate collection. No new package/binary is installed; the
recorded application-certification prerequisite and unknown original opaque HTTP 400
backend cause remain unchanged. The recovered pre-existing tab keeps its task.
