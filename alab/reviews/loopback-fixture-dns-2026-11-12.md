<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep loopback fixture startup independent of reverse DNS

The wide Python run's exec-fault smoke failed its 30-second elapsed assertion
after 36.118 seconds, although it raised the expected fixture-fault error.
A private replay failed after 35.878 seconds. Instrumentation localized 35.007
seconds before TLS initialization, inside `ThreadingHTTPServer` construction.
The fixture fault itself reached the controller in about 97 ms, and the guarded
command returned about 20 ms later. A separate constructor probe measured
`socket.getfqdn("127.0.0.1")` taking 35.003702 seconds of its 35.003771-second
construction. TLS generation took 0.183 seconds in the replay.

The smokes bind known numeric loopback addresses and do not use a resolved
server name. Add one shared `LoopbackHTTPServer` that retains the standard TCP
bind, actual port, HTTP threading and close behavior, but labels the server with
its numeric bound address. It refuses hostnames and non-loopback addresses
before binding. Route all five direct smoke constructors and the parallel-login
subclass through it; the latter retains its request-queue override. The renewal
matrix already pins the shared provider module. Standalone capacity-test loaders
now make the new sibling dependency explicit.

No timeout, TLS setting, response, verdict or fault-detection assertion changes.
The original exec-fault test now passes in 0.918 seconds. This is a local startup
measurement, not a general latency guarantee. The deterministic new regression
makes `getfqdn` raise and still serves a validated reply over loopback. Replacing
the shared class with the former standard constructor makes that regression fail
immediately. A second check verifies numeric server labels and refusal of other
addresses.

## Verification

The following modules passed, including both new regressions:

| Module | Tests | Seconds reported by unittest |
| --- | ---: | ---: |
| `test_rotation_smoke_runs.py` | 9 | 8.599 |
| `test_provider_fixture.py` | 30 | 5.324 |
| `test_capacity_exec.py` | 2 | 0.018 |
| `test_capacity_guard_admission.py` | 11 | 1.410 |
| `test_capacity_process.py` | 4 | 1.128 |
| `weekly_reset_test.py` | 6 | 0.071 |

The additional `renewal_follow_test.py` run passed 27 of 28 tests. Its existing
`test_usage_server_requires_its_explicit_tls_root` failed with a self-signed
certificate verification error at the explicit-CA request. A controlled replay
fails at the same assertion with both the former constructor and the new one.
Both observed connections target loopback; the proxy map is empty, and disabling
proxy discovery also leaves the failure unchanged. TLS verification remains
enabled. This separate fixture failure is retained, not treated as a DNS
regression or a reason to weaken TLS.

The first direct isolated invocation of `weekly_reset_test.py` failed before
tests because that existing script imports a sibling module. Both smoke-module
tests were then run through an explicit adjacent-path `runpy` launcher under
isolated Python; exact commands and all outcomes remain recorded.

Independent and parent reviews found no blocker; `git diff --check` passed.
No Rust build or live account service was used. All test commands have ended.
The original phase timing and constructor measurements are under
`/private/tmp/.ubd/alab-rotation-replay-jvxy3ehc.noindex`; suite logs, commands,
negative control and TLS controls are under
`/private/tmp/.ubd/alab-loopback-no-dns-rjwo363o.noindex`.

## Final composition

After integration with the ident source-validation changes, all nine rotation
smoke tests passed in 7.781 seconds and all 30 provider-fixture tests passed in
5.089 seconds. The ident runtime files are byte-identical to those used by its
15 passing affected suites. These are additional checks of the combined tree,
not an increase to the distinct-test count reported above.

The two logs and tested-file hashes are retained under
`$HOME/.aterm/.ubd/loopback-composition-20261112-k3gwgyqa.noindex`.
The manifest SHA-256 is
`eeca21b2703cb7f11c3a238c83a4a857e1dce18dcde54b2e4581c704d4b92554`.

The separately recorded TLS failure was subsequently resolved by correcting
the generated certificate chain, with strict trust and hostname checks kept
enabled: [fixture TLS chain review](fixture-tls-chain-2026-11-12.md).
