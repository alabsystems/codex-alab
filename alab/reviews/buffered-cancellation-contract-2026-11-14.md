<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Buffered rejection cancellation — 2026-11-14

Decision: add one expected-GREEN public Images contract for dropping a pending
buffered rejection before a failure is selected. This is test-only coverage
of body ownership and cancellation, with no new production-defect or RED claim.

The frozen source is `d093b3152c3e9f0a4bd89115948cb4fd47ee8fb6`, branch
`alab/buffered-cancellation-contract-20261114`. Root retains native execution
`90306` completed the full three-package API library closure and canonical
scoped all-target lint with exit zero. All 449 tests passed, including the new
direct cancellation contract and the refactored managed-policy contract.
This review was prepared from source and archived evidence without repository edits, Rust commands, formatting, builds,
native operations or execution polling.

## Coverage gap and scope

The audit at the root repository's
`.git/alab-validation/buffered-cancellation-next-20261114/audit.md` distinguishes
pre-selection cancellation from the existing timeout and policy controls.
The timeout test awaited a selected HTTP timeout/body-capture result before
dropping its future. The managed-policy test selected a Policy error and capture
phase before dropping. A separate streaming Responses cancellation contract
already existed; neither it nor fake Images endpoint fixtures established this
public buffered ownership boundary.

The change adds
`dropping_buffered_rejection_future_closes_socket_without_selecting_failure`
in `codex-rs/alab-api-tests/src/buffered_http_error_body_tests.rs:1062–1129`
and factors the managed-policy readiness pattern into a shared borrowed helper
at lines 583–620. The recovery feature scenario is recorded in
`alab/features.toml:35`. Production, dependencies, Cargo manifests, lockfile and
compiler pins are unchanged.

## Owning future and readiness

The active isolated child uses a real public `ImagesClient::generate` call
over a direct no-proxy Reqwest fixture client. The loopback server validates
the actual complete private JSON POST, returns known 429 headers and a body
prefix with one byte missing from its declared length, and holds TCP open.
It supplies no suffix. The call is owned by
`Box::pin(client.generate(...).instrument(turn))`, with the existing production
thread/turn span names, Capture, per-request Telemetry and HeaderReceipt.

`buffered_body_ready` borrows that original Box. It requires both a oneshot
acknowledgment after the headers/prefix writes finish and exactly one actual
response-header receipt with the same call poll returning Pending. The helper
keeps an unexpected Ready result outside the bounded async block, so timeout
cannot discard it. It neither creates a replacement future nor owns/drops the
call when its five-second readiness observation ends. The static DEBUG barrier
observes only its receipt message; payload TRACE remains disabled.

After readiness, the test synchronously snapshots empty telemetry and capture,
one header receipt, and a server task that has not finished. It then drops the
actual owning Box before any subsequent await or time advance. This is not a
Pin reborrow cancellation and does not first wait for the five-second body
budget to select a result. The pooled client remains alive.

## Cleanup and selected-outcome checks

The test joins the real fixture while the client still lives, then drops the
client and performs every semantic assertion. Readiness failure, unexpected
completion and timeout use that same drop/join path before assertions.

The expected contract is one validated original Images POST, real peer EOF/reset,
and exactly one header receipt. The saved result must remain unselected;
request telemetry must be empty both before and after cancellation, and Capture
must remain empty. Thus cancellation must not fabricate a selected failure,
timeout/read-error/Policy completion, completed body phase or Responses request
shape. This does not prohibit the already observed DEBUG header receipt.

The fixture's emergency held-read bound remains
`WAIT + OPENING + 1 second` (12.5 seconds), with no early server EOF or missing
suffix that could complete the entity. Normal join is bounded to five seconds,
with abort/bounded-join recovery on failure; fixture Drop also requests abort.
The exact-child parent retains simultaneous 64 KiB stdout/stderr caps, a
30-second joint wait/capture bound and five-second kill/reap recovery. These
are test custody bounds. Readiness, server-pending and no-selection assertions
must still pass after cleanup; abort recovery cannot establish contract success.

The source supports this expectation: the owned response wrapper awaits capture
inline and emits its completion record after the permit operation returns;
request telemetry runs after the public send future returns. Neither wrapper
spawns a detached body task. The passing public contract adds observed real TCP
closure while the client remains alive, with no selected result, request
telemetry or completed capture event.

## Actual validation and custody

The full library closure at the same validated source reported:

| Test binary | Passed | Runtime |
| --- | ---: | ---: |
| Public API contracts | 52 | 5.30 s |
| API units | 260 | 2.03 s |
| HTTP-client units | 137 | 2.11 s |
| Total | 449 | |

Every binary reported zero failed, ignored, measured and filtered tests. Scoped
all-target Tippy has empty `owned`, `spanning`, `failing` and `scripts` lists,
`set_aside=0`, and a successful build-finished record. Its 673 compiler artifact
paths are unique and all lie under the recorded canonical `target/tippy` child.

| Evidence | State |
| --- | --- |
| Frozen validation source | `d093b3152c3e9f0a4bd89115948cb4fd47ee8fb6` |
| Actual API tests and scoped lint | Native execution `90306`, exit zero; counts, runtimes and classifier results above. |
| Complete retained archive | Root `.git/alab-validation/d093b3152c3e9f0a4bd89115948cb4fd47ee8fb6/buffered-cancellation/`; `checks.exit` contains `0`. |
| Checks log SHA-256 | `checks.log`, 46,504 bytes: `15faf6440ea5801a953d9835c2302e10cdfaee0c6ad61e60d1a09a4c234158cf`. |
| Lint JSON SHA-256 | `lint.json`: `12a6a1fb1a60c38a9eedaf0548b8082c606cf9c6f0877c860559c7163c92b365`. |
| Empty classifier SHA-256 | `verdict.json`: `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300`. |
| Tested source diff SHA-256 | `source.diff`: `3a1d5b655323f42ddf501bdbae60f0a0c7924f1fa10d7ecc0dd8649f4e331c0d`. |
| Ownership inventory | Complete: 86 hook topics and 13 features at the tested source; semantic qualification remains separate. |
| Prior audit SHA-256 | `2eff1309abbeb5f0f33d4894ff7d67b0742bf7635a6c42c40a66d501952ff36f` |
| Buffered test source SHA-256 | `d4fc6f385a4d9e36199c3626ee87102280c94649bcab62427bbbf032f70a4b2e` |
| Feature inventory SHA-256 | `6b0bfd7d3b6030422a647e809189981c30285c4aec06d5d1b8c612f593a3e500` |
| Unchanged shared capture SHA-256 | `09c5d2e48d4d767ccfa39889edbf838ff10f884d688d88d4af2461ce32d254eb` |
| Unchanged transport SHA-256 | `faa02e41c4c0af69ff32c109eead7180e5b579c052b78ba18e05392bfa8e5494` |
| Publication custody | Record the resulting identity separately in the private validation ledger after publication. |

This is a direct local fixture with `NoAuth`, not a native credential source or
managed-policy revocation test. It does not prove pending native admission,
accepted credential continuations or cancellation/lifetime of independently
owned credential/native actors. It adds no cancellation mechanism or new
production deadline. No application certification, package or installation
claim follows from this test-only expected-GREEN coverage, and no runtime RED
or new production defect is claimed.

The source-selected Trust 15698 certified-application macro-admission prerequisite
is unchanged. Packaging and installation remain deferred as recorded in
`targo-macro-admission-2026-11-14.md`.
