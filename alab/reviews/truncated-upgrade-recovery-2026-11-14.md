<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# An incomplete opaque prefix is not a recoverable envelope

The rejection reader suppresses incomplete entities and the API bridge retries
the exact opaque 400 envelope. Existing tests exercise those boundaries
separately. The public diagnostic regressions cover only complete bodies, so
they do not prove that the reader, mapper and logger retain the incomplete-body
distinction together.

## Decision and reason

Send a real rejected upgrade whose body contains the entire opaque JSON but
whose Content-Length declares one extra byte, then close. Require the public
API to preserve status, headers, URL and provider advice while returning no body.
The mapper must select no opaque retry. The structured diagnostic must report
`truncated`, retain original context and safe IDs, and omit the unavailable
body byte count rather than inventing zero. Complete controls also require the
exact measured entity size.

This tests recovery selection and observability across the real transport/API
boundary without changing production policy or adding a log parser.

## Validation

Checked source `cb1e368d0460dc566c4722f86dc13ffb4e72e2f1` passes the real truncated
400 contract and all four complete/split/delayed controls. All 11 API contract
tests pass within the 454-test HTTP/WebSocket/API scope. Scoped all-target Tippy
for those four crates reports no findings, and committed ownership validates.
The source-selected Trust 15698 execution used the explicit unverified test
lane; this is runtime evidence, not application certification.

The structured event reports `truncated` with a numeric wait, retains original
context and safe IDs, and omits unavailable body size. The public error retains
headers, URL and header-time advice while selecting no opaque retry. Complete
controls still preserve the exact body and measured size. Exact-source logs and
hashes are retained with the
[proxy contract validation](ipv6-proxy-targets-2026-11-14.md).

Application delivery remains separately deferred at selected Targo's
macro-graph certification boundary.
