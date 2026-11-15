<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Diagnostics guide literal SQL smoke — 2026-11-14

Decision: validate the unchanged first shell block in
[REQUEST-DIAGNOSTICS.md](../REQUEST-DIAGNOSTICS.md) with a bounded synthetic
SQLite smoke. At source `cf5ee6b6ad9f43d503d1f299fb5474fb5926bc52`, the final
fresh isolated invocation passed. No guide defect, source fix, maintained test
or new RED is claimed.

The synthetic database was created by applying the two committed logs migrations
in order. Independent comparison confirmed both migration copies and the
extracted shell block match the source bytes exactly. The final table has 12
columns, including `feedback_log_body` in place of legacy `message`; all eight
columns selected by the guide exist. The literal query filters the endpoint
and HTTP-capture targets, orders by descending id and limits output to 40.
It requires no prepared-request partner or `local_attempt` field.

The qualifying run used only a fresh owned synthetic database, CODEX_HOME with
a space in its path and PATH `/usr/bin:/bin`; normal HOME was left unchanged.
An owned XDG SQLite startup file was explicitly selected by a separate marker
probe, then made empty for the guide invocation. The actual CLI was
`/usr/bin/sqlite3` 3.54.0; Python SQLite 3.53.1 constructed the database.
Verification parsed explicit `cid`/`id` record starts from colon-formatted line
output, including the PRAGMA-to-SELECT boundary.

The 55 seeded rows comprise 50 matching targets and five newer noise rows.
Independent output comparison confirms exactly the 40 newest matching IDs,
50 through 11, in descending order. Older matching controls and newer unrelated
targets are excluded. Two selected handshake rows have no `local_attempt`, one
with a NULL thread; prepared HTTP and capture rows also remain present.
The schema output contains all 12 current columns and each selected row has
the eight requested fields.

| Control | Actual result |
| --- | --- |
| Literal read-only guide block | Exit 0, expected output, empty stderr |
| Separate read-only CREATE attempt | Exit 1, read-only write refusal |
| Missing-database read | Exit 1, file remains uncreated |
| Database after all controls | Original bytes/schema unchanged; no WAL, SHM or journal sidecars |

The current synthetic database hash matches the recorded before/after hash.
Only this final fresh isolated pass supports these assurances. Preliminary
checker attempts remain separately retained as nonqualifying evidence.

The private archive is retained at
`.git/alab-validation/cf5ee6b6ad9f43d503d1f299fb5474fb5926bc52/diagnostics-sql-guide/`.
All 15 indexed regular-file lengths and hashes, literal/migration identities,
actual output, refusal diagnostics, startup isolation and synthetic database
hash were independently checked. Raw receipts and synthetic artifacts remain
private.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `6d10cfa783317506356c6a1dd67af50fd8a9d936a8711e45f43e941fceeff50e` |
| Literal `guide-example.sh` | `dadc54b7cd3be3ecefab8ed53d46b2e14453cb2795373ab0e6ff25fb8a8b7ef6` |

This is SQL/schema/CLI/path/read-only evidence. Synthetic inserts establish no
Rust, transport, emitter, writer, body-capture, redaction, recovery-authority,
installed-application or real-log availability result. No user database was
selected by the qualifying final run. Earlier runtime results retain their
original source labels; no repeat or application certification is claimed.
The original backend cause remains unknown. Publication identity and receipt
remain in the private validation ledger.
