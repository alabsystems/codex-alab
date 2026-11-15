<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Give public API context regressions their own test boundary

The two transport-context regressions exercise public HTTP and WebSocket APIs,
but lived in the Autopilot crate to reuse tracing and loopback infrastructure.
Running them built a 951-test application-side library binary. The retained
context-fix logs show 10m36s compilation for its first reproduction, then 4m00s
for the subsequent two-test run after rebuilding API dependencies. Native
admission history recorded a 4.6 GiB peak and claimed 5.6 GiB for that package.

## Decision and reason

Move the two regressions to `alab-api-tests`, an empty library with test-only
dependencies. The existing `alab-*` member glob and gate discovery include it
automatically. The public API contract belongs with its transport and tracing
dependencies; a small real WebSocket fixture supplies the one scripted request
and its independent observation/release gates.

Retain the exact child-test filters, real tracing Registry, both dropped parent
span handles, same-attempt/thread/turn assertions, content exclusion and original
error values. The WebSocket telemetry callback must still see its ordinary
subscriber. Bound each phase and join of the new WebSocket fixture; abort its
task if the test drops or cancels cleanup. Use the supported WebSocket
implementation for the handshake and frames, with no copied response parser or
sleeps. The existing SSE fixture keeps its previous lifecycle.

The remaining SQLite log-writer integration still uses Autopilot's API and
tracing dependencies. Those dependencies remain. This move adds no shipping
application edge and no upstream hook. The new crate's path is recorded in
the existing provider-recovery feature contract.

## Validation and measurement

Cargo metadata added only the new ALab package. All 1,494 existing lock entries
are unchanged, including every upstream package. Every direct dependency of
the new crate is test-only. Cargo's resolved test graph contains 492 packages
versus Autopilot's 957, and excludes core, Autopilot, `core_test_support` and
`wiremock`. Both raw dependency trees are retained with the validation record.

Both migrated context regressions passed with source-selected Trust 15698 in
the explicit unverified test lane. Focused all-target Tippy for the new crate
passed; `alab/lint.jq` reports no owned, hook-spanning, failed-target,
missing-script or upstream findings. The committed feature inventory validates
and all 27 inventory unit tests pass. Independent review found no required
corrections or lost assertions.

| Observed measure | Autopilot test target | API contract test target |
| --- | --- | --- |
| Resolved test-graph packages | 957 | 492 |
| Tests in the binary | 951 | 2 |
| Binary bytes, same debug-free profile | 674,152,520 | 79,543,368 |
| Compilation after the API rebuild / new target | 4m00s | 2m07s |

The new tests ran in 0.13s. Initial admission claimed 16 GiB for each unknown
test/lint target, waiting 1m48s and 4m40s respectively; those first-run estimates
remain under the native capacity policy. New-crate Tippy compilation took
2m27s. Compilation times describe observed runs with different cache and
machine states, not a controlled speedup benchmark. The dependency exclusion
and smaller binary are the structural result; no unconditional latency or
memory-admission reduction is claimed.

Application packaging remains deferred at the unchanged source-selected Trust
15698 procedural-macro certification boundary. Test-only validation makes no
application qualification or installation claim.
