<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# WebSocket handshake SQLite evidence — 2026-11-14

Decision: add a durable diagnostic contract for the real rejected WebSocket
Upgrade path. Qualified source `00d9e416792dbe7616e649077c0e6a7cdd016f81` is
based on `a09ae0fbb84ad967baae00bae566085b28e4fb21`. Recorded focused/full library checks and
scoped all-target lint passed. Independent static review accepted one new owned state-contract
module (517 lines) and its two-line `cfg(test)` registration. This is a
coverage addition, with no production defect or new RED claimed.
No production transport/emitter/filter/writer, shared helper, manifest, lockfile
or compiler pin changed.

The actual owner is `codex-alab-state-contracts`. Its existing standalone dev
graph supplies public `ResponsesWebsocketClient`, SQLite and required Tokio
features. The fixture calls public `connect`, receives a real rejected Upgrade,
checks the public transport result and existing API mapping, then observes the
production diagnostic through the installed dynamic filter, actual writer and
SQLite query. Only an INFO sentinel is manually emitted; no fake handshake or
recovery event substitutes for the transport.

## Matrix and custody

One isolated synchronous child owns an unpaused current-thread runtime and runs
four cases, each with a fresh temporary SQLite home:

| Case | Public result before persistence assertions | Stored handshake expectation |
| --- | --- | --- |
| Inactive complete opaque 400 with advice | Complete original body/headers/advice; opaque recovery inactive | No endpoint diagnostic; INFO sentinel still persists |
| Active complete opaque 400 with advice | Complete original body and advice; existing retry mapping preserved | One complete handshake row with measured entity length |
| Active declared-length-truncated opaque-looking 400 | Real EOF yields unavailable body while status/headers/advice survive; no opaque retry | One truncated handshake row; unknown entity length omitted |
| Active complete specific 403 without advice | Existing terminal access-program class, original status/message; no advice/retry | One complete handshake row with advice false and no provider content |

The local TCP server validates an actual bounded Upgrade GET/header block and
sends no successful upgrade. Truncation sends complete JSON but declares one
additional byte, then closes the real socket. Bind, accept, read, write,
shutdown and join each have five-second bounds. Normal finish joins the server
before public-result/mapping or persistence assertions; join expiry aborts and
awaits it, while Drop retains an abort fallback. Temporary homes remain owned
through server completion, writer flush, queries and SQLite close.

A local parent adapter preserves the existing exact-child custody protocol while
controlling only that child's proxy environment: both NO_PROXY spellings are
`*`, and HTTP/HTTPS/ALL proxy aliases are removed. It mutates no global process
environment and uses the normal ReqwestDefault factory without system-proxy
fallback. The exact CHILD guard precedes runtime creation. The parent directly
awaits the unchanged default collector's 30-second deadline and internal
five-second cleanup, then applies its strict status/one-pass/both-EOF gate.
No outer timeout cancels collector cleanup; the child owns its separate runtime.

## Durable attribution and privacy

Every case flushes the writer and checks both its failure state and reporter.
A persisted INFO sentinel proves the installed subscriber/sink remains live in
the inactive case. Queries inspect every row at the endpoint target before any
original-thread query, so a threadless or wrong-thread row cannot disappear.
The thread-filtered IDs must then match. Rows require dedicated original
`thread_id`, a nonempty process UUID matching the sentinel, and display-style
`session_loop`/`turn` ancestry containing the original thread and turn.

Active cases require exactly one INFO handshake row with transport, phase,
status, typed capture outcome, numeric body wait, advice and sanitized upgrade
IDs. The invalid primary request ID is returned unchanged publicly, while the
stored value must use its valid alternate. Complete body length is measured;
truncated length is absent rather than zero. No ordinary HTTP capture target,
prepared request-shape/failure pair, local attempt, ordinary request ID, model,
request bytes or input/tool metrics may appear.

Stored handshake text excludes request/response auth markers, unrelated and
retry headers, URL/query/private markers, raw bodies, provider message/code and
loopback address. The contract concerns this event's fields and fixed original
context; arbitrary embedding-caller span content is not a privacy claim.
Upgrade IDs identify only the handshake, not a later HTTP replay. Persisting the
event grants no recovery authority and establishes no model payload submission.

## Actual validation

Source-selected Trust 15698 explicit unverified execution `57255` completed at
the qualified source with exit zero. Standalone `codex-alab-state-contracts`
record mode ran the exact new outer name
`websocket_handshake_diagnostic_tests::public_websocket_handshake_diagnostics_reach_sqlite_with_original_context`,
then the complete library, with outer `--test-threads=1`:

| Selection | Passed | Failed/ignored/measured | Filtered | Seconds |
| --- | ---: | --- | ---: | ---: |
| New exact test | 1 | 0/0/0 | 5 | 0.41 |
| Complete state-contract library | 6 | 0/0/0 | 0 | 0.85 |

Independent checks matched every emitted identity once in its selection and
confirmed both parsed summaries. There are six distinct outer tests and seven
outer executions. The new four-case SQLite matrix ran twice; its internal cases
are not additional registered tests. The full closure also covers the existing
endpoint/HTTP capture, queue-loss and opaque-recovery persistence contracts.

Each selection records one actual library, independently of its exact pass
lines and summary. Native unit-runner selection was revalidated against current
metadata and its non-test artifact. Two current binary SHA identities cover
that runner and the actual library executable. This state-only selection
required no Core V8, Code Mode host or CLI helper preparation. Controller,
focused/full unit and lint statuses are zero.

Scoped state-contract all-target Tippy completed under the separate canonical
`target/state-contracts/tippy` target. Independent stream checks confirmed
809 unique, existing canonical artifacts, normal/test state-contract library
profiles and successful build completion. The classifier's owned, spanning,
failing and scripts lists are empty, with no upstream diagnostics set aside.
Feature inventory passed with 88 hook topics and 14 features.

SQLite rows were written, queried and asserted inside the fixture, then removed
by normal private TempDir cleanup after close. The retained evidence is test,
source and artifact attribution; it contains no retained raw SQLite databases
or row snapshots.

The complete private archive is retained at
`.git/alab-validation/00d9e416792dbe7616e649077c0e6a7cdd016f81/websocket-handshake-sqlite/`.
All 36 indexed regular-file lengths and hashes and both current binary
identities were independently checked. Its recorded-run directory contains
28 files. Raw receipts and contained environment details remain private.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `be0cd4d3bb80eeccaf6c7296b0e495dbb8916fff387b3a890a473ef53c68c346` |
| `source.diff` | `e4dd76bdc328f388e8ac116436e1738a6b091ac8ec456c1aa688bf23a50c85ec` |

This establishes synthetic local evidence through actual SQLite persistence.
It identifies no original backend cause, qualifies no accepted native actor or
application, and changes no certification boundary. Publication identity and
receipt remain in the private validation ledger.
