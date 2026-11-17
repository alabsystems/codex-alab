<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Request timeline path-error normalization — 2026-11-16

## Problem and decision

The diagnostic reader documents Python 3.11 or newer, but its path preflight
caught only OSError. Python 3.11 documents RuntimeError for a symlink loop;
Python 3.13 changed strict resolution to raise OSError
([3.11 resolve](https://docs.python.org/3.11/library/pathlib.html#pathlib.Path.resolve),
[3.14 resolve](https://docs.python.org/3.14/library/pathlib.html#pathlib.Path.resolve)).
The older documented exception could escape direct API callers with its path
text. The CLI already sanitized it as operation_failed and returned nonzero.

Catch RuntimeError alongside OSError only in the existing path preflight and
raise the fixed database_unavailable category with displayed chaining suppressed.
This keeps database failures consistent across the documented resolution behavior
without a version branch or a wider catch. Scope validation still precedes path
resolution; nonregular files retain database_not_regular. Cancellation and later
unexpected errors retain their existing ownership. SQLite is not opened after
failed path preparation. Exception object context is not claimed to be erased.

## Reproduction and verification

At baseline 5a6c49b39db8093349bf6ce1580063ce0114e201, an actual private two-link
cycle on managed Python 3.14.8 raises OSError and is already classified correctly.
Injecting the documented legacy RuntimeError at Path.resolve reproduces the
API escape and the CLI category difference, with zero SQLite opens. Healthy and
missing-path controls, scope precedence and later-error cleanup remain sound.
Actual direct CLI output agrees with an observation-only SQLite-open audit.
These are controlled probes; no user database is accessed.

Tests-only commit 2390c80376d38c857cacc1e10468c8be76114a0e preserves all 34
existing test bodies and adds three methods. The actual RED run executes 37
outer methods: 36 pass and one fails in two subcases. The API subcase receives
the original RuntimeError; the main subcase receives operation_failed instead
of database_unavailable. All original methods pass, with zero unittest errors.

Fix source 538d010c99dc6cefe7dc6d06b2b1f7d843a7df7e changes only the preflight
exception tuple and guide sentence. The same test bytes then pass all 37 methods
once, with no failures, errors, skips, expected failures or unexpected successes.
The new controls cover a real loop, fixed API/CLI diagnostics before SQLite,
validation priority, cancellation identity and later RuntimeError ownership.
Existing healthy, missing-file, directory, read-only, WAL, schema, budget,
metadata, encoding and cleanup tests remain passing.

Both runs use isolated managed Python with bytecode writes disabled and
ResourceWarning treated as an error. Actual SQLite is 3.53.1. The private
request-timeline-path-errors-20261116 stage retains exact test inventory,
source/migration/interpreter hashes, raw outputs, separate RED/GREEN records and
before/after control/context checks. Independent reviews accepted reproduction,
source boundaries, actual failure reasons and passing evidence. Only Python
3.14 executed: injected documented behavior does not qualify a real 3.11 runtime.

## Publication and limits

Use the maintained expected-main publication guard, preserving disjoint peer
work and attributing runtime results to the tested source above. The final review
record is the only source change after qualification. This topic owns the reader,
its test file, diagnostics guide and this decision record.

This is a diagnostic API/error-category correction. It does not identify the
cause of the original opaque HTTP400, demonstrate a provider exchange, or qualify
an application build or installed session. Existing application certification
prerequisites and the human hold remain unchanged; no installation is attempted.
