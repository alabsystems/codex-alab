<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve repeated provider retry advice — 2026-11-16

Decision: in enhanced mode, retain repeated Retry-After values and select the
latest valid timer-safe deadline from one wall/monotonic receipt. Invalid advice
must not erase a valid wait; shorter advice must not shorten another valid wait
from the same header container. Capture the sleep-aware origin once, after
selection. Keep nested-over-outer advice precedence and existing retry authority.

The HTTP HeaderMap parser previously read only the first field. The JSON-to-HTTP
converter previously overwrote case-colliding keys, so {"Retry-After":"7",
"retry-after":"3"} selected three seconds and a later invalid value discarded
seven seconds. This could make recovery retry before valid provider advice.

RFC 9110 defines Retry-After as one HTTP date or delay, and restricts repeated
non-list fields. Longest-valid selection is an explicit ALab receiver recovery
policy for ambiguous/nonconforming metadata, not an HTTP requirement. Keep each
value whole: a date contains a comma, so never split comma-separated strings.
See [field order](https://www.rfc-editor.org/rfc/rfc9110.html#section-5.3) and
[Retry-After](https://www.rfc-editor.org/rfc/rfc9110.html#section-10.2.3).

One HTTP parser hook calls owned glue. It samples wall time before monotonic
time once, parses each complete value, rejects invalid UTF-8, invalid grammar,
overflow and timer-unsafe candidates individually, then chooses the maximum
Instant before a single wake capture. Calling from_header for every candidate
would resample receipt clocks; its upstream implementation remains unchanged.
The owned parser preserves that method's whole-value grammar.

One API insertion hook appends only normalized Retry-After fields in enhanced
mode. Every other header retains its existing insertion/collision behavior;
inactive clients retain first-field HTTP parsing and last-value JSON conversion.
The ordinary JSON converter still validates names and supported value types.
Exact duplicate JSON keys already collapsed by Value cannot be recovered here.
No new retryable error kind, raw-content diagnostic or mode-independent policy
is introduced. Both upstream changes are thin marked hooks; neither file is formatted.

RED source 96d1565bccc792ffbdff1ac68002a6c4fa91e673 executes 490 outer methods:
488 pass, two fail, zero ignored/measured/filtered. The HTTP public HeaderMap
method fails at its first 3/7 lower-deadline bound. The real WebSocket method
completes its 401/3/7 control, then fails at 401/7/3 after receiving the selected
HTTP error and joining its fixture. Later rows/assertions are GREEN-only. Two
nested child failure summaries are not additional outer methods. Inactive HTTP
and JSON collision controls pass. After RED, one later invalid-UTF8 assertion
uses an absolute receipt bracket instead of a scheduling-sensitive remaining wait.

GREEN source 7a2a596eb3e3b9985e9e415a5102aa4d0b6e0817 passes all 490 methods
and scoped all-target lint. Compose peer31f0af2668691a466f0dd6476c3a0889b89194c0
unchanged, including unit-result custody fixes; merged source
eb8d52f9af7be16213968d62bfdfa71946866d75 passes the same checks again.

Source review requested a direct active-parser preservation oracle for dates
and expired advice. Add exact already-locked httpdate1.0.3 only to the owned API
test crate; only its ALab lock entry gains a dependency line. A new isolated
public method checks a dynamically formatted future date alone and beside a
numeric wait in both orders using wall/monotonic receipt brackets. Zero and a
past date remain Some with zero remaining wait; absent headers remain None.
This additional control is GREEN-only, not part of the original RED inventory.

Final runtime source d00db3320a6b0d80b1a99bdbb4844499c14c8504 passes 83 public
API contracts, 271 API units and 137 HTTP-client units: 491 passed, zero failed,
ignored, measured or filtered. All four required method identities pass once.
Scoped all-target Tippy has zero findings in every classification, including
set-aside. The native project frontend/runner and source-selected Trust15698 run
in the explicit unverified runtime lane. The final post-run change is this ADR.

Coverage: active HTTP HeaderMap combinations, invalid UTF-8/overflow/whole-list
values, future-date mixtures, zero/expired-only presence and inactive selections;
20 real WebSocket nested-collision cases cover 401/400/403/429/503, with unchanged
raw body, selected advice, outer headers, safe IDs, diagnostics privacy and cleanup.
The fixture observes its first response.create, not a complete wire inventory.
This is not a new actual-HTTP-wire duplicate test or an executed Core/Autopilot
retry action; the shared parser/bridge retain their existing contracts. No new
application certification, package or installation is claimed. The recorded
application-certification prerequisite and original opaque400 backend reason
remain unchanged. Private evidence is retained under the shared Git directory's
alab-validation/repeated-retry-advice-20261115 path.
