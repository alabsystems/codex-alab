<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Recover safe repeated response correlation IDs — 2026-11-16

Decision: select the first safe whole correlation value per HTTP header name,
then preserve the existing primary-before-alias precedence. An invalid earlier
field must not hide a later usable service ID. This is a diagnostic receiver
policy; it grants no request, retry, routing or authentication authority.

The endpoint, HTTP rejection-body and opaque HTTP 400 normalization emitters formerly
called HeaderMap::get, validated only its first value, and stopped on rejection.
They could all lose the same usable later ID. The three owned wrappers now scan
get_all/filter_map(to_str) through one borrowed, allocation-free owned-core
selector. It accepts exactly 1..128 ASCII letters/digits/hyphen/underscore and
never trims, splits or truncates decoded values. No HTTP dependency is added to
the core crate. Existing opening-time/live-mode gates surround the same callers;
the pure selector does not inspect or change activation.

Raw returned headers remain byte-identical and ordered. Non-Retry-After JSON
case collisions retain their original normalization; this policy cannot recover
JSON fields already overwritten there. Existing primary/alias call order stays
unchanged. WebSocket nested correlation copying continues to receive only that
single-normalized-value-per-ID JSON map. No upstream hook, manifest, lockfile,
error class, retry advice or request serialization changes.

Real TCP cases require one observed fixture POST with its original three input
items and completed tool-call/output pair, a complete opaque HTTP 400 body and actual
peer closure after client drop. They retain exact ordered response header bytes,
status, body and advice. All non-ID endpoint/body-phase contracts run before the
late ID oracle. Mapping the actual API error must still select the genuine
normalized opaque Stream marker, Other protocol, retryability and identical
advice. A narrow normalization subscriber captures the emitted IDs and real test
parent identities; unexpected field names are recorded with a fixed sentinel
without formatting values, then refused by a full field-name allowlist.

Only after joined cleanup, byte/metadata/semantics/context/count/privacy checks
does one assertion compare the three actual emitter ID pairs. Positive rows
cover a healthy control, invalid internal whitespace, overlong, non-UTF8,
non-ASCII values before valid primary/ray IDs, and a valid later alias. A
separate five-row control method requires first-valid priority, alias fallback,
all-invalid omission, absence and the exact 128-byte boundary. Comma-containing
values remain whole and invalid. HTTP itself removes wire OWS before returning
HeaderMap values; no-trim behavior for decoded strings belongs to the pure
selector's unit tests. No test claims preservation of HTTP framing whitespace.

Tests-only RED source 9c47f997ac2b6b3c93ca138900ab6e4ae8004751 executes 869 outer
methods: 868 passed, one failed, zero ignored/measured/filtered. The healthy row
prints complete, then internal_whitespace reaches late_ids before the intended
assertion at line 1376. Its three actual pairs are all (req_alias, None), versus
the expected primary request/ray IDs. All five control rows complete through
their passing method. The remaining positive rows are GREEN-only. Four outer
summaries are 92/1 public, 368/0 core, 271/0 API and 137/0 HTTP-client; the nested
child failure is not an extra outer method. Independent observation captures
all four RED binary snapshots, hashes, receipts and diagnostic identities before
GREEN replaces those cache files.

Production commit 99dee7680c0fb02ad56966ac17c27c8267c3042c was prepared in an
isolated checkout while RED waited for native capacity, leaving its source
unchanged. After the intended RED completed, the commit was transferred and the extra
owned checkout/branch removed. All 25 incoming peer paths are preserved exactly.
Current runtime 0b8566951c25bb2300c1c46cf41e45568ecaadc5 includes main
5d49d5658335155cd0fafb6af20bbd57f17302cd. The wire tests remain byte-identical
to RED. All 872 outer methods pass: 93 public, 371 core, 271 API and 137 HTTP-client,
zero failed/ignored/measured/filtered. The core total includes three new pure
selector tests. Both wire methods and all three helper methods pass exactly once.
All scoped all-target Tippy classifications, including set-aside, are zero.
Both phases use source-selected Trust 15698 with the native frontend/runner and
explicit unverified runtime lane. An initial current run passed all 872 methods
but exposed one redundant closure in this new test; its direct method reference
was applied to both final RED and GREEN, keeping the wire test bytes identical.
The original phase records are retained separately, with no passing lint claim.
The concurrent publication helpers report 48 passing local/synthetic fixture
checks in three commands; individual child exit codes were not separately saved.
Only this decision record follows the final Rust execution.


The tests exercise actual TCP HTTP/API capture and mapping plus the pure core
selector, not an executed Codex Core/Autopilot retry, global socket inventory or
backend receipt of a real user request. Successful isolated-child markers are
drained rather than printed, so later positive reach follows the exact passing
method and its loop control flow. Privacy checks use actual emitted key sets
and private sentinels; they do not certify arbitrary embedding-caller spans or
other log targets. No host sleep or TLS experiment is part of these cases.

This is source-only work. The application-certification prerequisite remains
unavailable, no new package/binary is installed, and the original opaque HTTP 400
backend reason remains unknown. The already recovered tab keeps its task.
