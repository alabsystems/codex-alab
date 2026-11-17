<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Memories HTTP-success/decode-failure contract — 2026-11-17

Decision: add one ALab-owned public contract for `MemoriesClient.summarize_input`.
This expected-pass test protects unary HTTP completion before schema decoding;
no production defect or RED counterexample is claimed.

One strict isolated child runs valid output and malformed output with numeric
`memory_summary`. Both return HTTP200 with raw `Retry-After: 7`, while every retry
flag is enabled. Each requires one auth call, one unary execute and one successful
callback `(0, 200, None)`, with no stream or replay. Independent literal typed
output checks the valid result; malformed output retains nonempty original
`ApiError::Stream` without matching serde wording. Later decoding cannot rewrite
HTTP callback success or replay the completed operation inside this client.

Literal JSON checks cover `traces`, metadata, items and present reasoning.
Provider, caller and auth headers each require their exact literal value and
cardinality one; unrelated derived headers are permitted. Public body preparation
checks JSON semantics without requiring Responses' encoded-body representation.

Initial source `3ccc9e07` passed 396 methods and scoped Tippy (qualification
`2f989…`), retained in snapshot `89d0eeee…`. Root/Marker review `64adb…` then
identified a test overconstraint: whole-map equality would reject legal derived
`Content-Type` headers. Source `cb7e6492` replaces only that header oracle; all
other test bytes remain unchanged. Initial passes do not qualify repaired source.
Repaired `cb7e6492` subsequently passed 396 methods and scoped Tippy
(qualification `e113…`); this phase also remains historical.

Before an ADR was committed, main `60eb626e` changed 45 peer paths, including
executed `unit-tests.sh`. Selected Rust source, both owned tests, Cargo inputs
and other helpers were unchanged; the executed-helper change nevertheless
required fresh qualification. Integration runtime
`01d01ec3ddca827144eac68d5cb8318e9ab5f526` preserves all 45 peer paths exactly
and both owned test files byte-identically to `cb7e6492`; no guard was bypassed.
Earlier 395/396 results remain phase-local. Both previous native phases are sealed
as unpublished history in snapshot SHA256
`551d59ea23acdc0c692be38f2f725326d336f599465726ad6c412a92c5538970`.

Fresh integration qualification passes all 396 methods: 111 public and 285 API,
with zero failed, ignored, measured or filtered. Scoped all-target Tippy exits 0
with 657 canonical artifact paths, both packages' normal/test profiles, compiler
exit 0 and all gating findings zero. Accepted independent qualification SHA256 is
`c999b024d1b79b8201147e1870f875e6940127bde58d9cb70bfc1ae6d908adbf`;
native capture SHA256 is
`a74d5058a7d3b6dd5bb9f1be367677b4ab7a068ce404f106890ac97e61406f5e`.
The current executed helper was included in this fresh run under independent
pre-use review `f0f097e7`. Two source rows are one outer method; strict child
completion does not retain per-row transcripts. This record is the sole topic
change after qualified runtime `01d01ec3`; both test files remain byte-identical.
The topic changes three paths against main `60eb626e`. No publication or future
composition is credited.

This scripted transport proves no wire, pacing, cancellation, provider or native
source-admission behavior. Certification/adoption and human holds remain unchanged;
the original opaque HTTP400 cause remains unknown.
