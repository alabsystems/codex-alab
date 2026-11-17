<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Explicit request-log database encoding admission — 2026-11-16

## Problem and decision

The metadata-only timeline accepted a current-shape UTF-16 database, matched its
canonical process/thread scope in SQL, then returned successful JSON with no
rows and one invalid-identity omission. Its bounded identity projection casts
TEXT to BLOB before ASCII validation. SQLite documents that BLOB casts can vary
with the database encoding ([casting rules](https://www.sqlite.org/lang_expr.html#castexpr)).
The existing schema gate checked columns but not this representation assumption.

Require exact UTF-8 database encoding at the start of `_schema`, with a read-only
`PRAGMA encoding` query. Otherwise raise the fixed
`unsupported_database_encoding` category before selecting metadata. Keep the
existing byte caps, BLOB projection and strict canonical identity validation.
This gives an explicit unsupported-input failure without adding alternate
encoding compatibility or repairing invalid identity tokens. The guide now
states the supported encoding.

The check runs after read-only connection setup, limits and BEGIN, under the
existing progress budget and operation-owned cleanup path. Authorizer rules,
SQL scopes, active-WAL behavior and atomic CLI output remain unchanged. No
conversion or write is attempted. SQLite distinguishes the query form from
encoding changes and notes that creation defaults depend on the open API
([encoding pragma](https://www.sqlite.org/pragma.html#pragma_encoding)); this
work does not assume every existing Codex database is UTF-8.

## Controlled reproduction and regression

At baseline `26b1956a73808493fd5d21e8552f6bebbf7764c2`, create UTF-8, UTF-16LE
and UTF-16BE databases using the two committed log migrations. Verify actual
encoding, all 12 columns and identical logical canonical metadata. Four matching
scopes per encoding cover PID, exact process, thread and their intersection.
UTF-8 returns one valid row with no omission. All eight UTF-16 matching cases
return zero rows and one omission, with CLI exit 0. Scope-miss and unscoped
refusal controls remain safe. The 18 API and 18 CLI invocations are controlled
probe calls, not unittest method counts. Synthetic bodies never enter output;
fixture databases are removed and no user database is accessed.

Commit tests before implementation at
`3500471db6259028aeb8fc7df3f99835c314fa23`. The actual RED run executes 34
outer methods: 32 pass and two fail, including all 31 original methods passing.
There are 12 failure entries inside those two methods: ten API cases returned
without a TimelineError, and two CLI cases returned 0 instead of 1. Later
category/no-SELECT assertions were not reached in those failed cases. A separate
UTF-8 control confirms embedded-NUL identities stay unavailable and never form
selected-row gaps. These counts are not 12 failing outer tests.

Fix source `072adaae4ea22696705121aa3b37dcca6219711f` adds only the two-line
encoding gate and guide clarification. Test bytes remain identical to RED.
The actual GREEN run passes all 34 methods once, with zero failures, errors,
skips, expected failures or unexpected successes. The new tests verify correct
UTF-8 metadata, explicit UTF-16 refusal before metadata SELECT, nonzero CLI
failure with empty stdout and the fixed safe stderr, unchanged read-only files,
and retained NUL nonrepair behavior. Existing WAL, bounds, budget, schema,
cleanup-precedence and output tests also pass.

Managed Python runs with isolation, bytecode writes disabled and ResourceWarning
as an error. The committed-source test inventory, five source/migration inputs,
interpreter identities, raw status/output and before/after control/context
snapshots are retained in the private `request-timeline-encoding-20261116` stage.
Independent reproduction, RED, implementation/control and GREEN evidence reviews
accepted these scoped results. RED and GREEN records stay separate; counts are
not aggregated. Failed verification attempts cannot become passing records.

## Publication and limits

The ownership and qualification baseline remains
`26b1956a73808493fd5d21e8552f6bebbf7764c2`. Publish through the maintained
expected-main guard, preserving disjoint already-main work and the tested-source
attribution. Independent final source, artifact and composition reviews precede
strict branch/worktree/process cleanup. The change belongs only to the owned
Python reader, its tests, the guide and this review record.

This confirms a diagnostic reporting bug for unsupported database inputs. It
does not establish a normal Codex-writer UTF-16 path or the cause of the original
opaque HTTP400. No Rust/application build, provider exchange, user-database or
installed-process qualification is claimed. Existing application certification
prerequisites and human holds remain unchanged.
