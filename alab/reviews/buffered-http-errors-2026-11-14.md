<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Buffered HTTP rejection authority — 2026-11-14

Decision: extend the owned rejection-body capture machinery to active,
unconfigured buffered non-success responses. Preserve the HTTP status, headers,
header-time typed advice and image-generation request ID when diagnostic bytes
are unavailable. Keep buffered strict UTF-8 decoding and all successful response
bytes under their existing contract.

Implemented and validated at `852268f2f140b161f4277900d0b01b0544cebb51`:
452 tests passed across the API closure and real SQLite contracts, and both
scoped all-target lint runs passed without classified or set-aside findings.
The independent proper RED reproduced lost HTTP classification and unwanted
replay. This provides no evidence about the cause of the original backend 400
incident. This review was prepared from source and archived evidence, with no repository edit, Rust command,
formatting, build, service call, native operation or root-execution polling.

## Evidence and reachable consequence

The prior audit is retained in the root repository at
`.git/alab-validation/buffered-http-rejection-next-20261114/audit.md`. In the RED
source's `http-client/src/transport.rs:181–204`, `execute` samples status, headers and
`RetryAfter`, then awaits the body before branching on status. With a `None`
limit, an incomplete body becomes the original mapped Network/Timeout failure;
that error was returned before constructing `TransportError::Http`. A body held
open had no diagnostic-body deadline on this buffered path. The prior streaming
rejection fix does not cover `execute`.

This path is used by standalone image generation/edit
(`ext/image-generation/src/backend.rs:104–136` through
`codex-api/src/endpoint/images.rs:85–100`) and web search
(`ext/web-search/src/tool.rs:115–174`, `endpoint/search.rs:34–44`). The audit found
no automatic whole-call deadline on those two product routes. Provider requests
start with `timeout=None` and `response_body_limit_bytes=None`. Other consumers
have distinct caller contracts; this is not evidence for a global request
deadline or a cap on successful image/search bodies.

A known 429 with an incomplete entity illustrates the correctness gap. The
normal provider policy permits transport retries but disables 429 retries
(`model-provider-info/src/lib.rs:499–505`). `codex-client/src/retry.rs:23–38`
chooses retry eligibility from the error variant; a reclassified Network error
can therefore permit replay that the known 429 policy disallows. Network also
lacks the captured HTTP advice, so `retry.rs:101–111` selects local backoff.
`ImageRequestError` only extracts the image-generation ID from an HTTP error
(`codex-api/src/endpoint/images.rs:29–35`), so that ID is lost too.

The real HTTP RED below confirmed the classification and replay consequences. Generic tool
cancellation already aborts and awaits ordinary tool tasks; this work does not
claim cancellation is broken or replace external task cancellation.

## Committed thin implementation

The fix replaces only buffered `execute`'s `None` byte-read branch with one marked call
into the existing owned response module, passing the original URL, sampled
typed advice and the original private error-mapper function pointer. The
initial independently reviewed candidate is retained at
`.git/alab-validation/buffered-http-errors-pending-20261114/capture-candidate.rs`.

The committed `alab_buffered_bytes` wrapper delegates inactive or successful
responses to the exact original `bytes().await.map_err(original_mapper)` path.
For active rejections it shares one generic bounded byte collector with the
streaming wrapper. Complete buffered entities use an identity `Bytes` decoder
and return to the original upstream
`String::from_utf8(bytes.to_vec()).ok()` conversion. Invalid UTF-8 remains
unavailable text; buffered charset/BOM behavior is not changed to reqwest's
lossy text decoding. The streaming wrapper retains its existing charset/BOM
decoder inside the same permit and deadline.

The collector retains at most 64 KiB of entity bytes before charset decoding,
requires complete entity framing, and has the existing fixed five-second body
deadline. Cooperative yielding after 16 ready chunks keeps deadline/revocation
polling available. This is not a cap on underlying network allocation or the
charset-expanded UTF-8 string. Declared oversize, observed oversize, interrupted
or timed-out bodies never become a retained partial prefix.

The timer and decoder stay inside the response's original `NetworkPermit::run`.
Original biased revocation and native source preparation retain priority;
pending native admission is not converted into an ordinary HTTP timeout.
Buffered policy failures pass through the original mapper as policy errors.
Other unavailable active rejection bodies return the already-known
`Http { status, headers, body:None, retry_after }`, which the existing `None`
error arm propagates. This intentionally changes that narrow failure from a
generic transport error to the authoritative HTTP rejection already received.

Successful, inactive and explicit-`Some` behavior remains on its existing
paths. Advice is not reparsed after capture, and absent bytes cannot invent an
opaque envelope or provider terminal code. The shared content-free phase event
continues to log outcome, wait, complete entity bytes when known, status, typed
advice presence and safe IDs; it emits no URL, payload, raw headers or raw error.

## Actual proper RED

`codex-rs/alab-api-tests/src/buffered_http_error_body_tests.rs` contains two
public Images regressions. Each isolated child first runs a complete 429
positive control over the same real path, before the incomplete-body case:

| Test | Contract selected |
| --- | --- |
| `truncated_buffered_rejection_retains_http_advice_and_imagegen_id` | No retry configured. One real POST must return the original 429, headers, typed advice and imagegen ID, with body omitted after EOF truncation. |
| `truncated_429_does_not_replay_images_when_only_transport_retries_are_enabled` | One retry is available, `retry_transport=true`, `retry_429=false`. Truncation must still produce one POST and preserve the original HTTP rejection. |

The actual loopback server consumes a complete public
`ImagesClient::generate` POST through direct no-proxy `ReqwestTransport` with
request logging off. It verifies the JSON and private fixture header and saves
each exact request body. The first incomplete response declares one extra byte,
sends the complete provider JSON prefix, then closes. Original 429 headers
include wire `Retry-After: 7`, safe request/ray IDs and the imagegen request ID.
If an erroneous replay occurs, a second complete benign 429 makes it observable
and lets the old caller finish without generating an image.

Assertions compare all actual POST bodies, ensure per-send telemetry accounts
for every POST/attempt, and require the selected status/ID/advice and a single
429 telemetry record. The typed advice deadline is checked against receipt
bounds around the call. Server finish/join occurs before semantic assertions,
including the intended RED failures. The bounded parent uses the exact child
name, simultaneous 64 KiB stdout/stderr capture, a 30-second joint bound,
five-second kill/reap and `kill_on_drop`; server phases and endpoint opening
also have fixture bounds.

Execution `56241` exited 101: final parent summary `0 passed; 2 failed; 0 ignored;
0 measured; 45 filtered out`, in 0.02 seconds. Both complete controls and
wire/attempt accounting reached the incomplete cases, with the real server
reaped before their intended assertions. With no retry, the caller returned
`Transport(Network("error decoding response body"))` instead of the original
HTTP rejection. With one transport retry available, the actual POST count was
two instead of one despite `retry_429=false`. Each isolated child failed its
intended assertion; no source fix had been applied to that RED.

## Fixed-source controls

The fixed source has five public buffered test functions: the two proper RED
regressions and three additional controls:

- Complete plain UTF-8, retained UTF-8 BOM, invalid UTF-8 and Windows-1252 bytes
  preserve strict buffered text semantics. Every fully framed entity remains
  `body_capture=complete` with measured original bytes, even if strict UTF-8
  conversion leaves the HTTP error body unavailable.
- Inactive complete and truncated errors preserve their old classifications
  with no phase event. A successful image JSON body containing a 64 KiB + 1
  byte encoded image remains parsed and uncapped by the rejection budget.
  Explicit limits retain immediate `ResponseTooLarge` and in-budget EOF
  status/advice behavior, without entering the new default capture.
- A held incomplete 429 reaches an observed actual response-header receipt and
  a pending body before the outer 6.5-second observation. It must return with
  `timed_out`, no entity bytes, original HTTP/advice/IDs and no restarted advice.
  The server supplies no suffix or early EOF; it observes real TCP closure
  while the client remains alive and is reaped before semantic assertions.

The held fixture observes only the existing static DEBUG receipt message for
its header barrier; it does not copy raw DEBUG URL/header fields. Phase checks
retain original caller context and safe IDs, exclude private fixture content,
and forbid invented Responses shape/local-attempt metadata for an Images call.
All five buffered functions passed in the public-contract binary. Existing
shared streaming tests cover complete terminal refusals, decoding, policy and
cancellation; no new dedicated buffered-policy/revocation or public search test
is claimed here. The full real SQLite state-contract suite also passed after
the shared helper changed.

## Actual validation and custody

The source-selected API execution `63226` and separate state-contract execution
`5213` both exited zero at the same fixed source. Runtime results were:

| Test binary | Passed | Runtime |
| --- | ---: | ---: |
| Public API contracts | 50 | 5.28 s |
| API units | 260 | 1.92 s |
| HTTP-client units | 137 | 2.10 s |
| Full SQLite state contracts | 5 | 0.44 s |
| Total | 452 | |

Every binary reported zero failed, ignored, measured and filtered tests. Both
scoped all-target Tippy classifiers have empty `owned`, `spanning`, `failing`
and `scripts` lists, with `set_aside=0` and successful build-finished records.
All 673 API and 809 state compiler artifact paths are unique within their run
and lie under their respective separate canonical lint child targets.

| Evidence | State |
| --- | --- |
| Actual RED source | `1af66f077a8febb664e56f60d9c42a03d649de72`; production unchanged. |
| Actual RED exit/counts | Native execution `56241`, exit 101; two intended failures after complete controls, actual wire/telemetry accounting and server reaping, as detailed above. |
| Complete RED archive | Root `.git/alab-validation/1af66f077a8febb664e56f60d9c42a03d649de72/buffered-http-red/`; `checks.exit` contains `101`. |
| RED log SHA-256 | `checks.log`, 16,028 bytes: `78bf744a8aa985263dc0cc43b4e0af5157a8a4221ba7917a8d5c7934df540989`. |
| RED source diff SHA-256 | `source.diff`: `2cd52172a154fb3ec6bc9cc105f0c84da26db7962e58d9a1f731cb1beca907fe`. |
| Applied production source and hook | `852268f2f140b161f4277900d0b01b0544cebb51`; one `execute` None-arm hook, shared owned collector/decoder, corrected owned controls and the recovery feature scenario. |
| Complete GREEN archive | Root `.git/alab-validation/852268f2f140b161f4277900d0b01b0544cebb51/buffered-http/`; both `api-checks.exit` and `state-checks.exit` contain `0`. |
| API tests/lint log SHA-256 | `api-checks.log`: `9e105f3ed566e57cbfbf7a98ae7c94a148dbca6921903ae1cce77a771cd36c15`. |
| API lint JSON SHA-256 | `api-lint.json`: `c4312b9a8ab159cf4e59aea27fc92de5183b2ba356fdfbc8cbced8d9c3545b81`. |
| State tests/lint log SHA-256 | `state-checks.log`: `e8b1046ae8c7f8605df0e3d8fadaa71d6d3b6c1740c7a6cd591e298259d4a6d4`. |
| State lint JSON SHA-256 | `state-lint.json`: `18d97d12acdd56dc7dc945ca83221e6ba10fad4f03eac350f5780f0f00cdb111`. |
| Empty classifier verdicts SHA-256 | Both `api-verdict.json` and `state-verdict.json`: `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300`. |
| Fixed source diff SHA-256 | `source.diff`: `66441b147cc0e31ce166847fd40f9f71ede9ea0bef30a16dc3e4e9ab450e00f8`. |
| Ownership inventory | Complete: 86 hook topics and 13 features at the fixed source; semantic qualification is separate. |
| Publication custody | Record the resulting identity separately in the private validation ledger; no self-referential document commit/hash is required here. |
| Prior audit SHA-256 | `4ad72200e208571e45064f0d6ea068e147492982e555388d7b045410f2c05ddb` |
| Initial candidate and fixed shared glue SHA-256 | Both retained candidate and `codex-rs/alab-core/glue/http_error_body_capture.rs` at the fixed source: `09c5d2e48d4d767ccfa39889edbf838ff10f884d688d88d4af2461ce32d254eb`. |
| Frozen RED test source SHA-256 | `df367420e85ec12c83e5b371a5db5265cf2090f6357754d6f6e983818a74fa81` |
| RED transport source SHA-256 | `3402b1a60579dc882b23650c1ef37bf8cd34cea446a928ed9feb42c0ddf922e3` |
| Fixed transport source SHA-256 | `faa02e41c4c0af69ff32c109eead7180e5b579c052b78ba18e05392bfa8e5494` |
| Fixed buffered tests SHA-256 | `57515cea933dd441e9d9c93f998aea62c52d18dfb4b7d67bcc7179c7e93857b5` |
| Fixed feature inventory SHA-256 | `7a63075f8df1edb28a23830194c04cb86903a1e20823f9b423f13d86e903a737` |

The source change preserves the original native admission authority and does
not grant fallback authorization or relax resource/toolchain admission. Local
synthetic HTTP fixtures and source tests do not prove native credential custody
or application qualification. The existing application certification blocker is
unchanged and has not been re-evaluated by this work.

## Remote main composition

Remote main advanced to `b60cb2f11d786fffd1fc9d0b6181351250c7628a` while
validation ran. Its ten changed paths belong to independent quota code, tests and
documentation. Neither its changes nor the resulting merge modify any of the
26 workspace package roots observed in the API/state lint artifacts. The
source-selected compiler, workspace manifests/lock, build configuration, native
selection helper and this feature inventory are also unchanged. An exact Git
diff confirms the merged trees for those inputs equal the tested source.
The private `remote-integration-review.json` retains the inspected path sets.
The recorded validation therefore continues to cover this fix; no quota runtime
or application result is claimed by this review.
