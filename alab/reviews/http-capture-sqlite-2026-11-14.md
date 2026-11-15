<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# HTTP rejection capture through SQLite — 2026-11-14

Decision: extend the owned persistence contracts to cover the actual HTTP
rejection-body capture event through the installed SQLite filter and writer.
This closes a coverage gap after the HTTP capture implementation; it does not
change production behavior or establish a new production bug. The complete
owned state-contract suite and scoped lint passed with the new coverage. This
was an expected GREEN coverage addition, with no RED failure claimed.

The frozen source reviewed is
`f880dbf20548516e9e34aeaae50d321b02e3ffea` in the cached worktree. Root retains
native execution `26037`, configured with selected Trust 15698, for the full
`codex-alab-state-contracts --lib` suite and scoped all-target lint using the
canonical state-contracts lint target. The execution completed with exit zero.

## Coverage and implementation scope

`codex-rs/alab-state-contracts/src/endpoint_diagnostic_tests.rs:510–967` adds
`public_http_capture_phase_reaches_sqlite_with_original_context`. The test runs
four cases, each with its own temporary home and real SQLite runtime. The child
runs the inactive case first, then activates ALab for the remaining three. A
persisted INFO sentinel proves that the installed writer remains live even
when the diagnostic emitters are disabled.

| Case | Original public HTTP result checked before persistence assertions | Expected capture row |
| --- | --- | --- |
| Inactive | Complete opaque 400 body and typed `Some` retry advice from wire `Retry-After: 7`; inactive recovery behavior remains disabled. | No shape, capture or failure rows; INFO sentinel persists. |
| Complete opaque | Complete opaque 400 body; original status, URL, headers and typed `Some` retry advice from wire `7` survive. | `complete`, measured complete entity length, advice `true`; selected failure remains opaque and retryable under the existing mapper. |
| Truncated | Declared length is one byte longer than the full opaque-looking prefix, followed by real TCP EOF; status/headers/advice survive with `body=None`. | `read_error`, unknown entity bytes omitted, advice `true`; no opaque classification or replay delay is invented. |
| Known terminal refusal | Complete 403 JSON with code `access_program_not_enabled`; no retry header/advice. Mapper detail discriminant matches `ApiError::AccessProgramNotEnabled`, with original 403 and provider message retained. | `complete`, measured complete entity length, advice `false`; the refusal remains terminal. |

The 403 assertion deliberately compares the mapped detail discriminant against
the existing typed `AccessProgramNotEnabled` variant, rather than assuming a
generic access-denied variant. Advised cases check both the original typed
advice and the exact wire header value `7`; persistence only records the typed
presence boolean, not raw retry headers or a restarted deadline.

Each case uses the public `ResponsesClient` with a real direct
`ReqwestTransport` from `HttpClientBuilder::build_direct`, with request logging
off. Provider retries are disabled. The loopback TCP fixture consumes and
validates the complete actual JSON POST before returning the selected rejection;
it does not substitute an artificial body transport. It checks the request's
declared body length against the same prepared JSON and returns the measured
length for comparison with the persisted request-shape field.

The patch stays in owned test/process files: the new contract and a small
factoring of the existing persisted-context assertion, the `net` feature on the
existing Tokio dev dependency, and the recovery feature scenario in
`alab/features.toml:35`. There is no production emitter, classifier, filter,
writer, transport, policy, or authority change.

## Persisted contract

Active cases query all rows before comparing the original-thread query. This
ensures a lost thread attribution cannot hide a row by filtering it away. Each
active case expects exactly one shape event, one capture phase, and one selected
HTTP failure, in that order. They retain INFO level, the original thread and
turn span text, and the same nonempty process UUID. Shape and selected failure
share a positive local attempt; the capture phase must not invent an attempt,
request shape, model or request byte count.

The phase target is
`codex_http_client::response::alab_error_body_capture`. Assertions cover
`phase="rejection_body"`, the case's outcome, a numeric `body_wait_ms`, complete
entity length only when known, original HTTP status, typed advice presence and
safe request/ray IDs. An invalid primary request ID is skipped in favor of the
valid existing alias. Original errors retain raw response metadata, while
persisted records exclude the fixture's private request/body/URL/header markers,
invalid ID, raw authorization/unrelated/retry headers, and provider error text
or codes. This is emitter-field coverage under controlled caller spans, not a
blanket assertion that arbitrary caller-provided span fields are private.

The installed production SQLite filter and writer are used. The test flushes
the layer, checks its failure state and reporter, queries actual database rows,
and closes the runtime for each case. It does not infer persistence solely from
an in-memory tracing collector.

## Bounded fixture and child custody

The existing shared `test_process::parent_handled` owns the exact child test,
with a caller-derived exact test name and strict child marker, one test thread,
simultaneous capped stdout/stderr capture (64 KiB per stream), a 30-second
wait/capture bound, five-second kill/reap recovery, and `kill_on_drop`. Child
activation occurs after child detection; the parent process remains inactive.

The actual TCP fixture bounds bind, accept, complete POST read, response write,
socket shutdown and normal task join to five seconds per phase. Request headers
are limited to 16 KiB, body to 64 KiB, and total fixture request to 80 KiB. The
server sends complete responses or an intentional EOF truncation; there is no
five-second body stall case here. The public opening has its own five-second
fixture bound. Before endpoint or persistence assertions, the test drops the
client and joins the server. Abnormal server join failure aborts/reaps its task
under a bound; fixture `Drop` also requests abort. Database awaits additionally
remain within the parent child's overall custody bound.

## Actual validation

| Evidence | Result |
| --- | --- |
| Frozen source | `f880dbf20548516e9e34aeaae50d321b02e3ffea` |
| Full state-contracts library tests | Native execution `26037`, exit zero: 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48 seconds. This is the new public HTTP capture test plus four existing contracts. |
| Scoped all-target lint | Same execution, exit zero. `owned`, `spanning`, `failing`, and `scripts` classifier lists are empty; `set_aside=0`. Build-finished success is true. All 809 compiler artifact paths are unique and under the canonical `state-contracts/tippy` target. |
| Complete tests/lint archive | Root repository `.git/alab-validation/f880dbf20548516e9e34aeaae50d321b02e3ffea/http-capture-sqlite/`. |
| Checks log SHA-256 | `checks.log`, 4,290 bytes: `fa6709c019f3b853b7afaba1b9b1fd2fb9168f5a3b477f7c39a0add1178099b7`. Archived `checks.exit` contains `0`. |
| Lint JSON SHA-256 | `lint.json`: `ec330762222b4047143cc772909c0282c95e463c3d137acbcb3463686eed7916`. |
| Lint verdict SHA-256 | `verdict.json`: `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300`. |
| Tested source diff SHA-256 | `source.diff`: `d75832ceae655d0517dc4d7dc49df3053d758d5d931d172cd1f5104730cf52d4`. |
| Ownership inventory | Complete: 86 hook topics and 13 features at the tested source; semantic qualification remains separate. `inventory.txt` SHA-256 `0f0c68d74e89dbe91bef53b9b464f31fc10ded7b0f7ec5dcbab399e120e6ade5`. |
| Endpoint test source SHA-256 | `9068786d78e46494c9bfe13c187ca607f2730160ae5532959e5fe8b08dd01bfa` |
| Crate manifest SHA-256 | `96346d06f5333d223a975a99032e2c6ec74db78da0519ab9e5a1216b74d4b916` |
| Shared child helper SHA-256 | `ff6b56c424621690b836f6829b2a6c2ca53364e5ce610f77bdcc4ee21eb96a2e` |
| Feature inventory SHA-256 | `9bcc5038a1656a30418a31208fb57ba08271f64a3eda487ae72cb8a77c8752cc` |
| Publication custody | Publication receipts belong in the separate private validation ledger. |

This test uses a local unauthenticated fixture with synthetic auth-header
markers, not real credentials, native source admission, or a provider service.
It adds SQLite persistence coverage for four finite HTTP cases. Existing API
tests cover stalled bodies, cancellation/revocation, limits, decoding, and
successful streaming controls. These source-suite results do not qualify the
application or prove native authorization.

The selected Trust 15698 certified-build macro-admission prerequisite is
unchanged. Application qualification, packaging and installation remain deferred
as recorded in `targo-macro-admission-2026-11-14.md`; this coverage does not bypass
that requirement or substitute another compiler.
