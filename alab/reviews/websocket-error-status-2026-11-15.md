<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve WebSocket HTTP status across optional error-body drift — 2026-11-15

Decision: recover an independently valid, unambiguous non-success WebSocket HTTP
wrapper after strict optional-error decoding fails. Keep ordinary strict parsing,
including valid special reconnect-code precedence, first. Put the fallback in
ALab's existing parse_for_stream extension; no upstream hook or schema change.

A numeric optional error.code or error.message rejects the strict wrapper even
when its reported 401/400/403 is valid. The capacity-only ambiguity guard does not
handle these ordinary refusals. The generic stream parser can then ignore the
malformed error and replace the reported HTTP failure with a later close/Stream.
That erases status and sends recovery through the wrong route.

Admission requires decoded unique root names, exact type:error and one valid
non-success status alias or two equal valid aliases. The original field map is
kept intact; equal-alias strict normalization uses a separate copy. Named error
body fields must also be unique. During recovery, nonobjects contribute no nested
metadata, and array positions never become headers. Parse only optional named headers after
status admission, clear code/message in the recovery copy, and decode the rest
with the existing wrapper type. The caller retains the exact original body.

Clearing malformed code/message prevents a malformed special-socket code from
obtaining the reconnect route. Valid typed special codes keep their existing
precedence. The parent mapper still owns nested-then-envelope provider advice,
outer correlation precedence and validated nested correlation merging. It never
copies nested authentication or arbitrary recovery headers. The unchanged HTTP
bridge can still classify the original body and decides retry policy; generic 401
is not newly promised Never, and no live Login/Autopilot transition is claimed.

Inactive, statusless, successful, conflicting, malformed/null-alias and repeated
field paths gain no fallback. The existing malformed-capacity guard and maximum
validated-advice logic remain unchanged. No full failed-response parser, deadline
parser or clock sampling is added before validated HTTP admission. This is a
reported WebSocket status, not evidence of a physical HTTP response on the wire.

Tests-only source a21e1be0c826c3bd82358ff925749cd59183a3ce reproduces the
first 401 with a primary status field and numeric code case as a server-close Stream. The fixture
first observes the outgoing response.create, releases the frame after caller
context exits, and verifies channel closure and peer join before the HTTP-type
assertion fails. Advice, returned-header, protocol and diagnostic/privacy checks
in that positive method are not reached in RED. Of 346 outer methods,345 pass,
one fails and none are ignored; the 15 negative/control exchanges and one inactive
exchange pass. Nested child failure summaries are not extra outer methods.

GREEN source 677a68a6eba05212d9ba6cf4ec3b106a0f7584c2 passes 78 public contracts
plus 268 API units: 346 passed, zero failed/ignored/measured/filtered. All three
new method identities pass exactly once. Scoped all-target Tippy succeeds with
zero owned, spanning, failed-target, missing-script or set-aside findings.

The positive child executes 60 matrix exchanges across 401/400/403/503, primary,
alias and equal aliases, and numeric code, numeric message, scalar, six-field
array or bad nested-header shapes, then one malformed special-code401 control.
These 61 exchanges establish exact Transport HTTP status and original body,
nested seven-second advice or outer eleven-second fallback, safe ID precedence,
and closed-channel/server cleanup. Generic 401/403 protocol identity is checked
against literal wire values, without assuming Never. Mapped advice remains the
same captured deadline; existing HTTP bridge policy is not replaced.

Returned HTTP metadata preserves original outer auth, unrelated and retry-after
values, while distinct nested-only auth/identity/recovery fields do not propagate.
Captured shape/failure diagnostics keep the original attempt and thread/turn,
HTTP category/status and safe correlation IDs, excluding request/provider text
and arbitrary headers. These are assertions over the diagnostic target, not all
application logs. The fixture observes its first real request; no complete wire
count, reused-socket measurement or end-to-end Login action is claimed.

The negative child checks 11 raw ignored/ambiguous/duplicate frames, one existing
malformed-capacity maximum-seven-second advice case, two valid special reconnect
codes and an existing three-field positional special-code path. The inactive
child retains strict decoding. Together the public children execute 77 scripted
exchanges; these scenario counts are not additional outer test methods.
Raw fixture support preserves duplicate names verbatim. Existing Value callers
serialize once and use the same observed-request/release/cleanup sequence.

Both runs use source-selected Trust 15698 with the qualified native project
frontend/protected profile and native unit runner in the explicit unverified
runtime lane. Independent source/test and native-evidence reviews retain source
bindings, current library/runner snapshots and separate-target lint scope.
Private exact-source evidence lives under the shared Git directory's
alab-validation/websocket-error-status-20261115. Earlier static proposals and
RED claims retain their source identities; only GREEN passes runtime and lint.

The operator guide records admission and authority limits. Source-selected
Trust 15698 and the qualified native project frontend/runner remain in use for
explicit unverified runtime tests and scoped lint. No application certificate,
new package, installation, actor or compiler change is claimed. The recorded
application-certification prerequisite and original unknown opaque HTTP 400 backend
cause remain unchanged.
