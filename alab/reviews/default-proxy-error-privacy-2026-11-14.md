<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Redact malformed environment proxy errors before string conversion

The pinned default proxy parser copies a malformed environment URL into
`UrlError::InvalidProxyConfig`; truncated percent decoding can instead copy the
original password component. The explicit proxy parser already substitutes
`<redacted>`, but the default path forwards the raw error.

Responses maps this error to `TransportError::Network(other.to_string())`.
`telemetry_transport_error_message` preserves that network text, and core passes
it to `record_websocket_connect`. The telemetry implementation places it in the
common `error.message` fields sent to both log and trace events. This source
chain is separate from the owned fallback warning that was already corrected.

## Decision and reason

Apply the existing typed explicit-route redaction at the ALab-owned default
`ProxyConfig::from_env` boundary. Replace only `Url(InvalidProxyConfig(_))` using
the existing parent helper; preserve all other errors, successful configuration,
proxy selection, NO_PROXY behavior, authentication and source admission. The
error remains a configuration error at the connector and a network error at
the public API. No retry or transport policy changes.

Redacting before conversion prevents this error's private payload from entering
downstream strings. It also avoids duplicating the parser or guessing which
fragments of a malformed URL contain credentials. This is a scoped claim about
these configuration errors, not every caller-provided log field.

## Reproduction and validation

Committed RED source `2cc2a77a6cdedb62d253ebf2d2aade184fde7240` runs isolated
connector tests. Both default cases fail Display redaction assertions: malformed
URI exposes the username and truncated percent decoding exposes the password.
The existing explicit-route redaction control passes. The public Responses API
regression also fails Display redaction on its first malformed-URI case after
confirming `Network` classification and the parser-specific category; that
parent exits before its second case. Both native Cargo stages return 101.

The final fixtures use loopback targets and proxy authorities. They require the
original connector variant or public parser-specific Network error before
checking Display and Debug for complete URL and independent credential markers.
This prevents an unrelated socket failure from passing. Parent environment is
unchanged, exact children isolate proxy and CA settings, and child waits and
cleanup are bounded with retained handles and kill-on-drop cancellation fallback.
Existing real proxy/authentication controls remain in the final suite.

Fixed source `771d55d9ec5d4048e81de9e434d193150f618708` passes all 460 targeted
library tests: 53 WebSocket, 137 HTTP client, 258 API and 12 API contracts.
The public API contract completes both malformed-URI and password-decoding
cases with their parser-specific Network category and no private markers in
Display or Debug. Both default connector cases and the explicit baseline pass.
The existing real proxy authentication, IPv6 address, TLS identity/certificate,
source-admission and request-diagnostic controls also pass.

Source-selected Trust 15698 executes these in the explicit unverified test lane.
Scoped all-target Tippy for the same four crates reports no findings; committed
ownership validates 86 hook topics across 13 features. Cross-reviews accepted
the fixtures and actual mapping, including unchanged UnsupportedProxyScheme
fallback, NO_PROXY bypass, successful authentication and other error variants.
Upstream production files, dependency identities and compiler pin are unchanged;
the API contract crate explicitly enables its existing Tokio process feature.
Exact-source logs, hashes, RED failures and the source-chain review are retained
under the common Git directory's `alab-validation` records.

The telemetry exposure is established by source data flow; these regressions
exercise the real connector and public API rather than a separate telemetry
implementation. These are runtime checks, not application certification.
Certified application delivery remains deferred at the recorded
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
