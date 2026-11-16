<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve standalone invalid-prompt diagnosis — 2026-11-15

Decision: admit exact named-object `invalid_prompt` events through the existing
shared `response.failed` parser, alongside `invalid_request` and the existing
capacity codes. Reuse a positive typed request-refusal predicate for admission
and immediate emission. Preserve every other existing early-emission type.

`response.failed` already knows this terminal validation result; standalone
`error` does not. A pending stream can erase the diagnosis with an idle timeout,
a later completion can hide it, and a statusless WebSocket can replace it with a
close error. A precise refusal must keep its type and message, without provider
retry advice or a replay of the same invalid request. Existing upstream message
semantics remain authoritative: missing/null means `Invalid request.`, while
empty and whitespace strings remain unchanged.

Keep the finite wire gate before the full parser. The parser can capture retry
clock state even for a result a caller subsequently discards. A parse-everything
adapter would change ignored paths. Require a named object and string code;
positional arrays, type-only, near-code and excluded policy/quota/rate events
keep their prior path. Full typed decoding still rejects malformed fields.

Valid WebSocket status wrappers and special/terminal provider handling remain
first. A malformed wrapper with `status:401,status_code:"bad"` supplies no
validated HTTP identity. A valid `invalid_prompt` body may diagnose that request,
but it does not gain HTTP status, Login/account authority or retry advice.
InvalidPrompt is already Never/Fatal in the unchanged protocol/actor layers.
Policy admission was considered and rejected for this topic because malformed
status plus a policy code could authorize independent work. No such admission
is implemented.

Tests-only commit 8fce91fda5137d0e9716dabcc0c9645929e31255 was composed
with remote c6740c92251d29724dbf8142b5f928a791c28ad8 before execution, because
that peer change affects the unit-result collector. RED source
d5ca9896866ac326ba8a5cb45401cba726de1d4b passes 334 outer methods and fails
exactly three new semantic cases, with zero ignored. The established failed
response passes all 18 message/tail combinations before the first standalone
Pending case becomes an idle-timeout Stream. WebSocket HTTP400/401/503 controls
pass before the statusless case becomes a server-close Stream. The separate
malformed-401 case also becomes a close Stream. The 42 negative and three inactive
SSE cases pass. Later standalone positive rows are GREEN coverage, not reached
RED evidence. Nested child summaries do not count as additional outer methods.

GREEN runtime source c30347958190f208589cd3d269e5f57fc91d885e passes 71 public
contracts plus 266 API units: 337 passed, zero failed/ignored/measured/filtered.
All five added method identities pass exactly once. Full source loops with
strict child completion markers cover 36 paired Failed/Error message/tail
cases, 42 malformed/type-only/near-code/positional/excluded-code controls and
three inactive tails. Refusals retain exact typed messages, no retry/advice,
closed channels and dropped bodies without polling a contradictory tail. SSE
uses one observed custom endpoint send; it is not a real TCP inventory. Captured
SSE diagnostics retain invalid_prompt, safe existing IDs and matching attempt,
with no HTTP error status or provider/request content.

Real loopback WebSocket cases observe the first response.create before releasing
the failure frame and join the bounded peer. Valid400/401/503 preserve raw HTTP
status, body and nested seven-second advice; statusless and malformed401+bad
alias yield exact InvalidPrompt with no mapped retry/advice. This asserts the
401 transport boundary, not a live Login transition. First-request observation
is not a full wire count. No new WebSocket diagnostic-log or correlation-ID
fallback behavior is claimed; the guide retains its InvalidRequest-only limit.

Scoped all-target Tippy succeeds with zero owned, spanning, failed-target,
missing-script or set-aside findings. Independent static reviews accept the
request-only authority boundary, test custody and exact proposal; independent
native-result review is retained with source bindings, artifact snapshots and
lint scope. Both phases use the same source-selected Trust15698 and native
project frontend/runner. Capacity waits retain their actual logs; no resource
reservation or compiler admission is bypassed.

Private exact-source evidence is retained in the shared Git directory under
alab-validation/standalone-prompt-refusal-20261115. The operator diagnostics guide
records the admitted request codes and their status, message and advice limits.

No upstream file, error schema, Core/Autopilot actor or recovery authority is
changed. Tests use the source-selected Trust compiler, native project frontend
and explicit unverified runtime lane. This is source/API verification, with no
new application certificate, package or installation claim. The recorded
application-certification prerequisite remains unchanged. This finding does
not identify the original opaque HTTP400 backend reason.
