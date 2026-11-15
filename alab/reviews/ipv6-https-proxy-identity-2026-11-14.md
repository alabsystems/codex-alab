<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Verify an IPv6 HTTPS proxy as an IP address

The pinned proxy parser preserves URI brackets in `ProxyConfig.host`. That is
correct for the proxy's TCP authority, but the explicit HTTPS proxy path passes
the same string to Rustls `ServerName::try_from`. Rustls accepts a bare IP
identity, so `[::1]` fails with `Tls(InvalidDnsName)` after TCP connection and
before ClientHello. Existing HTTPS proxy controls use DNS names or IPv4 and do
not exercise this boundary.

## Decision and reason

Convert only a valid bracketed IPv6 literal at the proxy TLS-name boundary.
Leave the stored host and bracketed TCP authority intact. Return the ordinary
Rustls `ServerName`, preserve the existing error mapping, and keep DNS, IPv4,
invalid names, routing, credentials, source admission and certificate verification
unchanged. This corrects an identity representation error without a retry or a
verifier exception. The helper stays ALab-owned; upstream gets marked import
and call hooks.

## Reproduction and validation

An IPv6 loopback bind/listen probe succeeded on this Mac. Committed RED source
`7db39938f38322134d0ffca4643ead52165b10a4` runs two real proxy contracts; both
fail with the original `Tls(InvalidDnsName)` in 0.04 seconds. The positive case
uses a trusted certificate with IP SAN `::1`, preserves HTTP/1.1 ALPN, requires
no DNS SNI, and completes CONNECT, WebSocket Upgrade and echoed text after TLS.
The negative case trusts the presented certificate for `::2` but connects to
`::1`; it requires the ordinary verifier's typed certificate-name rejection and
cannot pass on the earlier name-parsing error. It also detects a name-verifier
bypass. No external destination socket or DNS lookup is used.

Protocol phases and joins have deadlines, server ownership aborts on drop, and
negative cleanup joins the quiet server-side TLS failure. These are proxy TLS
controls with a plain WebSocket destination; they do not establish target TLS
behavior or credential revocation. IPv6 unavailability must be reported as an
unexercised fixture, never silently treated as a successful contract.

Fixed source `97c002ea6e91b923399a5ad1972922d2180d5d30` passes all 456 targeted
library tests: 50 WebSocket, 137 HTTP client, 258 API and 11 API contracts.
Both new controls pass, alongside the existing DNS/IPv4 proxy, TLS, framing,
source-admission and diagnostic controls. Source-selected Trust 15698 executes
these in the explicit unverified test lane. Scoped all-target Tippy for the
same four crates reports no findings. Committed ownership validates all 86 hook
topics across 13 features.

Independent source reviews accepted the actual helper, hooks and both real TLS
contracts. The production delta is an eleven-line ALab-owned helper and two
marked upstream lines; dependency identities and compiler pin are unchanged.
Exact-source logs, hashes, lint classification and the RED failures are retained
under the common Git directory's `alab-validation` records.

This is separate from the original opaque backend rejection, whose exact cause
remains unknown.
Certified application delivery remains deferred at the recorded
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
