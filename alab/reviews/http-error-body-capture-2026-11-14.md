<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Bound ordinary HTTP rejection capture

Decision source: `895fd3a21049622b2d77f88380f8581374388d9e`, following test-only
RED `1043dccf676c393b5d684682a4dc470d8bd8e63c` on base
`f0cbc7b09c724d19603c6b18dbdb83e67049e928`. Final runtime and scoped Tippy results are recorded below; independent source
review and execution evidence remain separate.

Ordinary Responses requests leave request timeout and response-body limit unset.
The non-2xx transport branch awaited the entire diagnostic text before returning
the HTTP error; SSE idle timeout starts only after that opening operation returns.
A peer could send HTTP400 headers and the complete opaque JSON prefix, declare
one additional byte, and keep the connection open indefinitely. Status, IDs and
header-time advice existed but could not reach selected-failure recovery or logs.
This reproduction does not establish why the original incident emitted HTTP400.

In active enhanced mode, only the unconfigured (`None`) non-2xx streaming branch
now uses one five-second deadline and a 64 KiB entity budget. Complete framed
entities retain existing reqwest charset/BOM/replacement decoding. Unavailable
entities produce the existing `Http { body: None }` disposition, preserving actual
status, original headers and already-captured typed retry advice. Explicit caller
limits keep their deliberate `ResponseTooLarge` contract; inactive mode, buffered
execute and successful SSE are unchanged. Retry and provider-error parsers are
unchanged. An unavailable body provides no proof of a provider terminal code,
and an incomplete opaque prefix never gains replay authority.

The original NetworkPermit.run encloses declaration checks, reads and decoding,
preserving source preparation and policy/revocation priority. The fixed deadline
is anchored before preparation; independently pending native admission is never
reclassified as ordinary HTTP capture timeout. Once admitted, timer-first selection
prefers an expired deadline even if body data is ready. Explicit boxed Send erasure
matches the existing source-aware transport boundary. Yielding every 16 accepted
chunks, including empty chunks, keeps deadline and revocation polls cooperative.
The byte cap measures retained entity bytes before charset conversion, not every
transport allocation or the possibly expanded UTF-8 string.

All production behavior lives in the 176-line ALab-owned
`alab-core/glue/http_error_body_capture.rs`. Upstream surface is a marked child
module declaration in `http-client/src/response.rs` and one marked replacement
call in `transport.rs`; committed delta snapshots were inspected. No Cargo or
dependency change. INFO capture evidence contains static phase/outcome, actual
status, numeric wait, complete bytes only when measured, sanitized IDs and typed
advice presence. It excludes content, URLs, arbitrary headers and source errors.

Actual RED exited 101: interrupted-entity and inactive/explicit-limit/large-SSE
controls passed; only the final 6.5-second opening assertion and default oversize
omission assertion failed, after their real-wire and cleanup controls. Evidence:
the private common Git validation archive for the exact RED source.

Final source adds eight public test functions in total: the four RED functions
plus complete terminal/exact-budget, charset/BOM/replacement, managed-policy
revocation and cancellation controls. They use bounded real loopback fixtures,
original context/metadata/privacy assertions and client-alive TCP EOF/reset
observations. The DEBUG header-receipt barrier consumes only the static message;
there is no raw-event/fmt collector and payload TRACE is disabled. Managed policy
coverage proves application/current-account revocation, not native credential
admission. At the checked source, all 45 public API contracts, 260 API units and 137
HTTP-client units pass: 442 tests, zero failures or ignored tests. The eight new
HTTP functions pass their complete/held/size/codec/authority/cleanup assertions.
All three packages pass scoped all-target Tippy with zero owned, spanning,
failing or script diagnostics and zero set aside. Artifact paths were checked
under the canonical separate Tippy cache. Committed ownership remains complete
for 86 hook topics and 13 features. Exact-source logs, diff, delta snapshots,
verdict and hashes are retained privately under
`.git/alab-validation/895fd3a21049622b2d77f88380f8581374388d9e/http-error-body-capture/`.
These are explicitly unverified runtime tests, not application proof. The
new INFO target is admitted by the installed filter by source inspection;
this topic does not claim a new SQLite persistence run.

Application certification remains deferred on the immutable Trust 15698
[macro-graph admission limitation](targo-macro-admission-2026-11-14.md). This topic makes no application-build, installation, proof-pin
change or certification claim.
