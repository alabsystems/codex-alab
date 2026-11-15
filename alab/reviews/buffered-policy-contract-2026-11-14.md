<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Buffered managed-policy coverage — 2026-11-14

Decision: add one owned public Images contract for managed current-account
policy revocation during an active buffered HTTP rejection. This is expected
GREEN coverage of the existing policy-preserving match arm, not a new
production defect or a claimed RED reproduction.

The frozen validation source is `fa43d0de150f6860f87eb5ccc3934dd359daa388`, branch
`alab/buffered-policy-contract-20261114`. Native execution `21332` completed
with exit zero: all 448 tests across the three API library binaries passed,
including the new managed-policy contract, and scoped all-target lint passed
in the canonical lint child target. 

## Why this contract

The audit retained at the root repository's
`.git/alab-validation/buffered-policy-next-20261114/audit.md` found no test that
reached the active, buffered non-2xx `None` wrapper's `HttpError::Policy` branch.
The existing buffered fixtures were unmanaged, and explicit-limit tests bypass
the default wrapper. Streaming rejection tests already covered the shared
collector's managed revocation, while lower-level policy tests covered permits
and routing. They did not establish buffered policy-versus-HTTP selection.

The test-only change adds
`managed_policy_revocation_ends_buffered_rejection_without_retrying` in
`codex-rs/alab-api-tests/src/buffered_http_error_body_tests.rs:920–1049`. It also
factors the existing Images constructor, adds a prefix-written oneshot to the
shared fixture, and records the feature scenario in `alab/features.toml:35`.
Production code, dependencies, lockfile and compiler pins are unchanged.

## Public request and authority checks

The isolated child activates ALab and publishes an unrestricted
`NetworkPolicyController` revision. It binds a clone of that policy with
`for_current_account()` through the real `HttpClientFactory` and
`RouteAwareClientPool`, with a direct proxy route. The public
`ImagesClient::generate` uses actual Reqwest HTTP and the existing `NoAuth`
fixture, not a mock transport or a native credential source.

The loopback server consumes the actual complete JSON POST, then returns a
known 429 with wire `Retry-After: 7`, safe request/ray IDs and an incomplete
Content-Length entity. It writes the full diagnostic prefix and holds TCP open
without supplying a suffix. Revocation requires both:

- A oneshot acknowledgment sent only after the first response headers and body
  prefix writes finish, preventing cancellation from racing unfinished writes.
- Exactly one existing static response-header receipt event, with that same
  owning boxed public-call poll returning Pending.

The DEBUG barrier observes only the static message field. The global DEBUG
ceiling keeps payload TRACE disabled. Before revocation, the test records that
there is no selected per-request telemetry and no completed capture event.

After invalidating the policy, the caller must select
`ApiError::Transport(TransportError::Policy(NetworkPolicyDenied::Revoked))`
within two seconds. The five-second diagnostic-body budget must not turn this
into an HTTP rejection or an ordinary timeout. The imagegen ID is absent, and
the mapped selected error has neither retry delay nor provider advice.

All three retry classes are enabled with two retries available. The contract
requires one actual POST with the original JSON and exact telemetry
`[(0, None, true)]`. The telemetry assertion matters because an incorrectly
downgraded policy failure could cause a second attempt that is denied before a
second wire POST. One POST alone would not reject that mistaken retry.

The separate capture event retains the already received status 429, typed
advice presence, safe IDs and original thread/turn context. It must record
`policy_denied`, omit unknown entity byte count, and preserve the existing
privacy and field-omission assertions. These diagnostic facts do not attach
HTTP metadata or retry authority to the selected Policy error.

## Readiness and cleanup custody

Readiness is bounded by five seconds. Early completion is stored instead of
asserted immediately; readiness failure or timeout follows the same cleanup
path. After selection or the two-second observation bound, the test drops the
actual boxed body owner, joins the fixture and only then drops the pooled
client and checks semantic assertions. The fixture must observe peer EOF/reset
while that client remains alive.

The server never closes the held response early to manufacture completion.
Its emergency bound remains `WAIT + OPENING + 1 second`; normal fixture join
has a five-second bound, with abort/bounded-join recovery on failure. Fixture
Drop also requests abort. The existing exact-child parent has simultaneous
64 KiB stdout/stderr caps, a 30-second joint wait/capture bound and five-second
kill/reap recovery. Those are test custody bounds, not production policy or
credential deadlines. The passing contract checks real cleanup; failure
recovery is not accepted as contract success.

## Actual validation and custody

At the same frozen source, the actual three test binaries reported:

| Test binary | Passed | Runtime |
| --- | ---: | ---: |
| Public API contracts | 51 | 5.30 s |
| API units | 260 | 1.91 s |
| HTTP-client units | 137 | 2.10 s |
| Total | 448 | |

Every binary had zero failed, ignored, measured and filtered tests. Scoped
all-target Tippy also exited zero: `owned`, `spanning`, `failing` and `scripts`
are empty, `set_aside=0`, and the build-finished record is successful. All 673
compiler artifact paths are unique and lie under the recorded canonical
`target/tippy` child.

| Evidence | State |
| --- | --- |
| Frozen validation source | `fa43d0de150f6860f87eb5ccc3934dd359daa388` |
| Actual API tests and scoped lint | Native execution `21332`, exit zero; counts, runtimes and classifier results above. |
| Complete retained archive | Root `.git/alab-validation/fa43d0de150f6860f87eb5ccc3934dd359daa388/buffered-policy/`; `checks.exit` contains `0`. |
| Checks log SHA-256 | `checks.log`, 46,385 bytes: `646c615268b9bd485131faabc5024fbd0ae487c00366241d6a37fb9c3f190270`. |
| Lint JSON SHA-256 | `lint.json`: `d69237f20a00942d8135fd80bb1ab9d949814bd693cf745db82892ff4adfdb5b`. |
| Empty classifier SHA-256 | `verdict.json`: `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300`. |
| Tested source diff SHA-256 | `source.diff`: `895ae54b2cc3dd740f304344eaca9dc51338f03ab672f90350511ae66aca8f38`. |
| Ownership inventory | Complete: 86 hook topics and 13 features at the tested source; semantic qualification remains separate. |
| Prior audit SHA-256 | `14752e0857239bd11944734ff87077fa27d4378f37025273f318b95846ad739c` |
| Buffered test source SHA-256 | `e086a5692cbace554de426064fb204a3cd32e2817b451c175a29b16934b7e0a1` |
| Feature inventory SHA-256 | `72a4eb719d6dfa83826e1a3e904f25ead4a51946e2c72d8236861b966ac9f995` |
| Unchanged shared capture SHA-256 | `09c5d2e48d4d767ccfa39889edbf838ff10f884d688d88d4af2461ce32d254eb` |
| Unchanged transport SHA-256 | `faa02e41c4c0af69ff32c109eead7180e5b579c052b78ba18e05392bfa8e5494` |
| Publication custody | Record the resulting identity separately in the private validation ledger after publication. |

This contract exercises managed application/current-account policy revocation.
It does not exercise native credential admission, pending native preparation,
accepted credential continuations or actor cancellation/lifetime. It neither
replaces those native contracts nor requests cancellation of independently
owned credential/native work. No application certification, package or install
claim follows from this test-only coverage addition. It supplies no runtime
RED or new production-defect claim.

The source-selected Trust 15698 certified-application macro-admission prerequisite
is unchanged. Packaging and installation remain deferred as recorded in
`targo-macro-admission-2026-11-14.md`.

The next coverage topic is explicit cancellation of a pending buffered rejection
before any selected result or body timeout. Existing buffered cases drop the
owner after selecting timeout or policy failure; the streaming path has a direct
cancellation contract. One equivalent buffered contract would check real TCP
closure while the client stays alive and ensure cancellation invents no selected
telemetry or completed capture event. This is a coverage gap, not a demonstrated
production defect.
