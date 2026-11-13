<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Give the loopback TLS fixture an unambiguous certificate chain

The fixture generated a CA and a different-key server certificate with the same
subject and issuer name, `CN=127.0.0.1`, without subject or authority key
identifiers. Python 3.9.6 with LibreSSL 2.8.3 accepted that chain, but Python
3.14.5 with OpenSSL 3.6.2 rejected the server certificate as self-signed. Both
clients loaded the exact generated CA; direct loopback connections reproduced
the difference without proxy routing.

A private certificate matrix identified two layers. A distinct CA subject alone
changed the modern client's refusal to `Missing Authority Key Identifier`.
Adding key identifiers resolved verification; both a same-name identified chain
and a distinct-name identified chain passed on both runtimes. Use the complete
distinct-name form: name the CA `ALab loopback fixture CA`, add its subject key
identifier, and give the server certificate subject and authority key
identifiers. Keep the existing IP SAN, CA constraints, key usage, server-auth
purpose, keys, validity period and client verification settings.

Only the two generated extension/configuration strings change. No trust flags,
hostname checks, certificate requirements or TLS protocol settings are relaxed.
The same fixture remains shared by renewal and parallel-login smokes.

## Verification

The unchanged explicit-TLS-root regression now passes under Python 3.14 and
system Python 3.9. All 28 tests in `renewal_follow_test.py` pass on Python 3.14
in 1.314 seconds. The separate 3.9 full-module attempt passed 18 tests and hit
ten existing setup uses of `unittest.TestCase.enterContext`, an API absent from
that runtime; the focused TLS case itself passes without test adaptation.

One generated server was also exercised by both runtimes' default client
contexts. Each accepted the correct CA and IP hostname, rejected the server
without its private CA, and rejected a wrong hostname with the CA present.
`CERT_REQUIRED`, hostname checks and each runtime's default verification flags
remained enabled. Independent code review and `git diff --check` found no issue.

The original cross-runtime observation is retained under
`/private/tmp/.ubd/alab-tls-runtime-ktffnbl1.noindex`; the four-variant matrix is
under `/private/tmp/.ubd/alab-tls-matrix-34objen5.noindex`. Final commands, suite
logs, certificates and positive/negative runtime results are under
`/private/tmp/.ubd/alab-tls-chain-checks-o0_791de.noindex`.
All checks use generated private fixture certificates and loopback only; no
installed trust store, native build or live account was changed.

## Standard-discovery regression and combined verification

A new provider-fixture regression enables strict verification and partial-chain
support where available, then serves a real usage response with the generated
CA. It separately rejects the same server without that CA, with another
generated CA, and with a wrong hostname. Both certificate requirements and
hostname checking remain asserted. The new test fails against the old
generator and passes against this one, so the former failure now has coverage
in the normal `alab/tests` discovery path.

The combined change passed 90 tests: 31 provider tests in 5.406 seconds, nine
rotation tests in 7.445 seconds, 16 renewal unit tests in 0.537 seconds, and
both smoke test modules' 34 tests in 1.261 seconds. There were no failures,
errors or skips in these runs. The copied runtime used for those tests was
checked byte-for-byte against the committed generator before integration.

The baseline/fixed strict-test logs, complete suite logs and tested-file
hashes are under
`$HOME/.aterm/.ubd/tls-chain-validation-20261112-e0nwj60v.noindex`.
The evidence manifest SHA-256 is
`caddf1e87956ccf5fe84916f21d6164d39b136cda143e6c388b2aa9572d0ef74`.
