<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Read the smoke package's own bounded metadata — 2026-11-16

Decision: enforce the smoke package reader's existing regular-file, no-symlink
and 1 MiB contracts on its opened metadata and bytes read. Use the existing
shared `capacity_exec` reader from `weekly_reset` too, after the early isolated
exec-wrapper branch. This removes the duplicate and preserves the runtime and
matrix input lists that already bind `capacity_exec.py`.

At source `08d0e3934a`, both readers checked paths and sizes, then called
unbounded `read_text()`. Deterministic real-file mutations at the subsequent
open boundary demonstrate two false acceptances: replacing package A's build
record with a symlink to B's record reports B's source for A's inert executable;
growing a checked record to 1,048,681 bytes still accepts its JSON. Four initial
regression assertions fail against those readers. The repaired reader refuses
both retained artifacts and both open-boundary mutations.

Opening uses `O_NOFOLLOW | O_NONBLOCK`; the held descriptor must be regular,
the read stops at the limit plus one, and mutation fields must stay unchanged
through the read. The descriptor closes on success and failure. These are
statement-reader guarantees: schemas, trial/native acceptance, source proof,
signatures and executable provenance retain their existing meaning.

All eight new test methods pass on managed Python 3.14 with `-I -B` and macOS
Python 3.9 with `-B`. Controls cover exact-limit acceptance, post-open growth,
changed bytes, failed reads, closed descriptors, static symlinks and FIFOs.
Managed Python also passes 14 trial-package, nine package-standing and 29
weekly-reset fixture methods (60 total). `git diff --check` passes. No compiler,
native Codex binary, provider account or release package was exercised.
