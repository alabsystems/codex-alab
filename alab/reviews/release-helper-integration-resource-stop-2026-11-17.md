<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Release-helper integration and deferred resolver coverage

The SGR-boundary and runtime-retention fixes were replayed onto incoming main
`941c55f2762ff042613947a59e433ee2e29c2e67` without changing their seven blobs.
Actual post-integration validation at `068ea63b48b280559a99b8a2b97f0663596f8619`
passed 188 unique methods across 12 modules, including 19 incoming proof-build
fixture checks. Source bookends covered 240 Python and six controller inputs.
Those fixtures used inert compiler fronts and real macOS containment, with no
Rust compilation, native toolchain admission or application qualification.

Publication was then safely refused because main advanced to
`08b827b92af79a11eeb7769572564a1e1b7cc2d0`. A normal merge preserved both helper
commits and the incoming resolver-lease change, producing
`b3ed43e53cc6c0456e779390f5ed6df16ba44e43`. All seven helper blobs stayed identical.
The new 15-module run executed 207 unique methods: 206 passed, one failed, with
zero errors or skips. HEAD, 240 Python files and eight controller inputs matched
before and after. All 169 methods from the two helper topics passed in this run.

The sole failure was the unchanged incoming resolver-containment fixture. Its
public sync subprocess returned resource status 75 at 18.98 GiB free against a
20 GiB running floor. The selector witness existed, but the inert metadata
archive and permission-denial witness did not: its metadata body never ran.
The test, sync controller and library bytes exactly match incoming main.
This is not a passing 207-method run or a source verdict about that fixture.

Native capacity recovery (`await sys free>=22GiB`) reported unmet/spent with
zero bytes reclaimed. All 17 owned fixture leaves were archived, every one of
504 archive entries verified, and open holders checked before retiring only
those fixtures. Their 614,350 payload bytes were insufficient to reach the
floor. No foreign cache was manually deleted, no floor was lowered and no
authority or capacity result was substituted.

Decision: publish the independently verified helper fixes with this incoming
coverage gap explicitly deferred. Preserve the failed result and retry that
fixture only after capacity changes. This source publication is not package
qualification; authenticated native host custody and a qualified publisher
remain unavailable on this host. Concurrent model-family changes are preserved;
the session's explicit local Sol 6.1 Ultra setting was checked separately.

Evidence is under `~/.codex/.ubd/release-helper-integration-20261117/`, in the
`068ea63b48-73qjq5zj` and `b3ed43e53c-w0p3nvq6` directories. The latter retains
`integration.result.json`, ordered log outcomes, source bookends, the owned
fixture archive and `source-publication-decision.json`. Final failed-run log
SHA-256: `9dab6cdc74d73772c078b579de7e85b14ba4c2a5ab83ca3a64ccc1187f6fa9b9`.
The verified fixture archive SHA-256 is
`d4eed2db7b51e83fbb74261eeb7daaf538a8a3ddc6d9b66f4d5febf66c24a6aa`.
