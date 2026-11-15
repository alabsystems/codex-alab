<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Explicit response-body-limit audit — 2026-11-14

Decision: preserve the explicit response-body-limit contract. The reviewed tree has
no production caller that selects `Some` for `Request::response_body_limit_bytes`.
The public API permits an explicit byte limit without a deadline; that capability
alone does not establish an uncovered in-tree production wait or justify a global
timeout, status-classification change, or policy/admission fallback.

This is a source review at `1db2ab7cf33332b50be03c9c0b3d0b4f0365c1db` in
`~/.ubd/codex-sse-diagnostics-20261113.noindex`. The detached worktree
HEAD was read directly. No tests, Rust commands, builds, network requests, or
native admission operations were run for this audit. Test descriptions below are
coverage observed in source, not claims that those tests passed in this audit.

## Production inventory

Repository-wide Rust searches for `response_body_limit_bytes`,
`bounded_response_bytes`, and `list_models_raw` found:

| Path | Production behavior | Deadline/cancellation owner |
| --- | --- | --- |
| `http-client/src/request.rs:83–98` | Request exposes the optional limit and initializes it to `None`. | Request also exposes an independent optional timeout. |
| `codex-client/src/provider.rs:79–88` | Provider requests start with both limit and timeout `None`. | Higher-level caller or configured transport must supply any deadline. |
| `codex-api/src/endpoint/models.rs:70–121` | `list_models` forwards its argument to public `list_models_raw`, which installs that argument on the request. No production literal `Some` occurs here. | Neither method installs a deadline. The raw API performs buffered execution before returning bytes/ETag. |
| `model-provider/src/models_endpoint.rs:245–250` | The concrete production model-discovery call passes `None`. | `list_models` wraps the complete `fetch_models` future in `MODELS_REFRESH_TIMEOUT` (five seconds), at lines 146–158. This includes auth, transport selection, response body and decoding. |
| `login/src/oauth/device.rs:85–93` | Separate direct use of `bounded_response_bytes`, with a 64 KiB limit on OAuth responses. This is not a `Request::response_body_limit_bytes=Some` caller. | Request uses `request_timeout`; `oauth_session_login.rs:375` supplies the 20-second `HTTP_TIMEOUT` defined in `oauth_session.rs:60`. Login selects caller cancellation against authorization at `oauth_session_login.rs:419–427`. |
| `login/src/oauth_session.rs:503–508` and `login/src/oauth/error.rs:75–89` | A separate gateway token-rejection collector limits diagnostics to 8 KiB and omits oversize diagnostics entirely. | Token endpoint uses the same 20-second request timeout. `oauth/client.rs:121–143` applies it before sending and then reads the rejection. |

All other `Some` field assignments or method arguments found by these searches
are tests. The only direct production consumer of exported
`bounded_response_bytes` outside its own transport implementation is device OAuth.
The OAuth error collector is included because it independently awaits a
byte-limited body under the same HTTP response policy machinery.

## What explicit limits guarantee

`http-client/src/transport.rs:265–313` checks an oversized declared content length,
then counts entity bytes across chunks. Neither helper starts a timer. Streaming
successes expose the bounded stream; buffered responses and streaming HTTP
rejections collect it before returning. Thus a peer that supplies an incomplete,
in-budget body can leave the collector pending when its caller supplies no
deadline or cancellation and policy remains valid.

Existing classification is deliberate at `transport.rs:186–204,227–249`:
oversize results remain `ResponseTooLarge`, including for a known HTTP rejection;
policy failures remain `Policy`; other body-read failures within an explicit
limit retain the known HTTP status/headers with `body=None`. Complete streaming
rejection bodies retain reqwest charset/BOM/replacement decoding after the byte
check. The new default rejection capture does not redefine these explicit limits.

The concrete OAuth request timeouts also cover ordinary body reads. Workspace
`Cargo.toml:478` selects reqwest 0.12; retained local reqwest 0.12.28 source transfers
the request's total-timeout `Sleep` into `Response::new` and then the response body
(`src/async_impl/client.rs:3087–3092`, `response.rs:37–46`, `body.rs:388–402`).
Route-aware execution reduces remaining request time across selection and hops
(`http-client/src/route_aware_client_pool/execution.rs:89–92,119,191–196`).

These are not unconditional native-admission wall-clock guarantees.
`HttpResponse::chunk` retains the original permit (`response.rs:55–57`), and
`NetworkPermit::run` races revocation first while native source preparation runs
before the body operation (`network_policy.rs:390–400`,
`alab-core/glue/http_native_source.rs:394–410`). This audit proposes no admission
bypass or timeout relaxation. Device polling also intentionally spans multiple
requests while awaiting approval; its individual request timeout is not a global
sign-in deadline.

## Existing source coverage and limits

- `http-client/src/transport_limit_tests.rs` covers exact-limit complete bodies
  under length, EOF and chunked framing; declared and observed oversize bodies;
  multi-chunk accounting; interrupted successes; interrupted 401/429 status and
  headers; decoding; independent request limits; and account revocation. The
  shared real HTTP fixture sets its own five-second request timeout at line 429.
  Revocation has a one-second outer assertion. These are caller-owned bounds,
  not a standalone timer installed by the byte collector.
- `codex-api/src/endpoint/models.rs:267` tests that the explicit limit survives
  an auth retry and does not limit another model request. Its capturing transport
  does not prove real HTTP body liveness.
- `model-provider/src/models_endpoint_timeout_tests.rs:11` verifies the model
  deadline against a delayed server response. Runtime-auth tests also verify that
  catalog timeout can leave registration alive for the next request, and refresh
  tests preserve rotated tokens after caller timeout/cancellation. No dedicated
  model fixture with headers plus a stalled entity was found; the encompassing
  source timeout is still explicit.
- `login/src/oauth_session_device_tests.rs:80,134` covers policy denial and
  cancellation while approval is pending. No device-specific stalled-body or
  oversize-body regression was found.
- `login/src/oauth/client_tests.rs:129,211,252` covers finite oversized rejection
  diagnostics, status retention after premature EOF, and success-body revocation.
  Its shared OAuth test helper uses `timeout=None`; these tests do not validate
  the production 20-second stalled-body deadline.

## Separate model-manager lifecycle opportunity

The endpoint's five-second fetch deadline does not bound every manager operation.
The explicit-provider refresh mutex (`models-manager/src/manager.rs:364,787`),
cache load (`1030`), store (`926`), and TTL refresh (`753`) are awaited outside it.
`refresh_after_auth_change` explicitly wraps auth/cache/lock work in another
five-second deadline (`644`); several other manager methods do not install their
own outer deadline. Caller adapters may provide cancellation or bounds. These
source observations are not evidence of an actual stalled cache or production
incident, and they do not concern a reachable production explicit-`Some` caller.

The owned live polling adapter separately awaits
`model_catalog.list_models(strategy)` without observing its shutdown token
(`alab-core/glue/models_refresh_live.rs:40`). Racing only that observer future
against shutdown, with shutdown selected first, is an appropriate next lifecycle
fix. `app-server/src/models_refresh_worker.rs:31` already routes every ALab
binary to this adapter, including Vanilla mode, so it can observe later mode
changes. Cleanup must apply in either live mode. It need not change strategy,
cadence, the live receiver, upstream `spawn_polling`, or credential/native actor
ownership. This is a proposed source change, with no runtime result claimed here.
