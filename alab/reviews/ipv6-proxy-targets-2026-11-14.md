<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Encode IPv6 targets at the proxy protocol boundary

The default WebSocket path obtains a bare IPv6 target for target TLS and passes
it to the pinned proxy helper, which renders HTTP CONNECT by joining the host
and port directly. It sends `CONNECT ::1:8080` and `Host: ::1:8080`. The explicit
route instead obtains a bracketed URI host; the pinned SOCKS helper cannot parse
`[::1]` as an IP address and sends it as a domain name. Both defects predate the
bounded rejection reader.

## Decision and reason

Normalize valid IPv6 literals only at the shared proxy connection boundary.
HTTP receives a bracketed literal; SOCKS5 and SOCKS5h receive a bare literal so
the existing library emits address type 4 and 16 address octets. Preserve the
original literal spelling. Forward proxy configuration, authentication, stream,
port and errors unchanged. Keep target TLS names, proxy selection, NO_PROXY,
source admission and outer deadlines outside this normalization.

This follows HTTP CONNECT's
[authority form](https://www.rfc-editor.org/rfc/rfc9112.html#section-3.2.3),
the URI requirement to bracket
[IPv6 literals](https://www.rfc-editor.org/rfc/rfc3986.html#section-3.2.2), and
the [SOCKS5 address representation](https://www.rfc-editor.org/rfc/rfc1928#section-5).
It removes malformed outgoing bytes instead of adding a retry for proxy errors
or copying the proxy protocol implementation.

The implementation stays ALab-owned. One marked import hook routes the explicit
dialer through the same helper as the default path. Existing proxy and WebSocket
implementations still perform the tunnel, Upgrade, validation and framing.

## Reproduction and validation

Committed source `00cd5a7b75d6f3769927cb2a847254995b5361db` runs eight isolated
real proxy contracts. The fake proxies acknowledge each tunnel, then complete
a real WebSocket Upgrade and text-frame echo before asserting captured proxy
bytes. Default HTTP IPv6 and explicit SOCKS5/5h IPv6 fail the exact wire assertion;
five controls pass for the already-correct default SOCKS paths, explicit SOCKS
IPv4/DNS and HTTP IPv4/DNS with Basic authentication.

The child processes isolate environment proxy settings. Protocol phases, client
work, child lifetime and server joins have deadlines. Child timeout cleanup
retains and explicitly terminates/reaps the process; cancellation retains
Tokio's kill-on-drop fallback. Server task ownership aborts on drop.

Checked source `cb1e368d0460dc566c4722f86dc13ffb4e72e2f1` passes all 454 targeted
library tests: 48 WebSocket, 137 HTTP client, 258 API and 11 API contracts.
All eight proxy wire contracts now pass. The API contracts include the
[truncated opaque envelope control](truncated-upgrade-recovery-2026-11-14.md).
Source-selected Trust 15698 ran these in the explicit unverified test lane.
Scoped all-target Tippy for the same four crates reports no findings, and
committed ownership validates all 86 hook topics across 13 features.

Two independent source reviews accepted the helper and wire contracts. The
upstream delta is one marked proxy import and a marked existing Tokio dev-feature
entry for child-process ownership; dependency identities and the compiler pin
are unchanged. Exact-source logs, hashes, lint classification and the RED wire
failures are retained in the common Git directory's `alab-validation` records.

This is an independently reproduced request-generation defect, not an
attribution of the original opaque backend rejection, whose wire evidence was
not retained. Certified application delivery remains deferred at selected
Targo's procedural-macro graph admission; the installed managed package is
unchanged.
