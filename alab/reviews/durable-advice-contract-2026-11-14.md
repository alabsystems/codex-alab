<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Durable request-advice evidence contract

Source review of `2b9f0d7c02df5bd2ccc8078b55fa5831845d090a` through
`8ddd890c52f6c71aa4b267398281cfd73d99f023`. The delta changes only
`codex-rs/alab-state-contracts/src/endpoint_diagnostic_tests.rs`; production
logging, schemas, filtering, error classification, and retry policy are unchanged.
No required source corrections were found.

The public Responses client and scripted transport drive seven independent
temporary SQLite databases through the installed filter and real background
writer. Expected durable diagnostic rows are:

| Case | Selected failure kind | `retry_advised` | Rows |
| --- | --- | --- | --- |
| Inactive, with live INFO sentinel | — | — | 0 |
| Opaque HTTP 400 without typed advice | `http` | `false` | 2 |
| Invalid access program with wire advice | `invalid_request` | omitted | 2 |
| Deferred advised error followed by completion | — | — | 1 shape |
| Overload with typed advice | `server_overloaded` | `true` | 2 |
| Overload without typed advice | `server_overloaded` | `false` | 2 |
| Quota rejection | `quota_exceeded` | omitted | 2 |

These fields persist as formatted tokens in `feedback_log_body`, returned by
`query_logs` as `LogRow.message`. They are not separate structured SQL columns.
The contract checks plain boolean tokens, omission for representations without
an advice slot, and representative semantic labels rather than mirroring all
18 recently added labels. Shape rows must omit failure kind, status, and advice.

The response gate holds semantic failures until the caller's scoped subscriber
and span handles have dropped. The fixture flushes and checks the shape before
release, consumes the terminal channel, then flushes before querying stored
rows. It preserves the original typed failure/message and advice presence,
original thread/turn context, process identity, local-attempt pairing, sanitized
server IDs, status/opaque distinction, encoded request size, and input-count
omission rules. It does not newly assert an exact retry deadline.

An unfiltered query includes threadless rows, so lost attribution cannot hide
behind the original-thread query. Exact row counts, matching original-thread
row IDs, a live INFO sentinel, and an empty writer-failure reporter guard against
disabled logging and incomplete insertion. Private input/tool/provider content,
URL/header sentinels, raw header names, and raw provider codes remain excluded
from the owned diagnostic text. The writer/runtime remain owned through the
flush and queries, and the database is closed afterward.

This is an expected-green storage contract: the production formatter and writer
already retain these fields, so no artificial RED or production fix is claimed.
Checked source `8ddd890c52f6c71aa4b267398281cfd73d99f023` passes both state-contract
test functions: the seven endpoint databases above and the existing opaque
recovery-to-SQLite contract. Scoped all-target Tippy reports zero diagnostics;
all 809 artifact paths use the canonical separate Tippy cache. The committed
inventory reports complete ownership for 86 hook topics and 13 features.
Independent review found no required corrections. Exact-source logs, diff,
lint verdict and hashes remain in the private common Git validation archive.

Runtime tests use the explicitly unverified test lane under the source-selected
native execution context. They do not certify the application. Delivery remains
deferred at the confirmed [Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
