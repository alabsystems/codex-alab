<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Cached descriptor snapshots for renewal evidence

Decision: renewal's `image_digest` delegates to the shared bounded descriptor reader,
which now exposes `file_sha256_snapshot` returning metadata and digest from the same
opened descriptor. The existing `file_sha256` API is a thin digest-only wrapper;
weekly, history and runtime keep their existing deliberate error types and close
policies. The descriptor owner remains guarded before acquisition, and the immediate
post-open trace control now explicitly observes that owner.

At base `cc1dbc8930427cdb89baf364da26c68acbfbb52c`, an actual public
`process_evidence` call on inert record data returned a cross-version digest. A real
600,000-byte A file was overwritten with B after the first 262,144-byte read. The
returned SHA matched A's first chunk followed by B's remaining bytes, matching neither
complete file version. Metadata changed and the cache correctly stayed empty; the
defect was returning the unverified uncached digest. A second real-file control grew
an initial 100-byte file after every read and reached a four-call guard after reading
102 bytes. The final regressions were run against the unchanged base functions and
both failed for these exact reasons, without test errors. No native process evidence
or complete passing application run is claimed by these controls.

On a cache miss the shared reader retains its initial-size-plus-one byte bound and
six-field stability check. On a hit it avoids content reads but still checks regular
descriptor metadata twice. Keys include device, inode, mode, size, mtime and ctime.
The private cache receives a new snapshot only after successful descriptor close.
Read, validation and close failures or cancellations do not publish new cache entries.
Existing active-primary,
nonthrowing close-note and close-only successful-exit refusal semantics are unchanged.
`process_evidence` retains its exact schema and process-birth handling.

The helper is loaded from its exact source path. Renewal's matrix input manifest now
includes it, and an inert copied-controller control mutates its actual file during the
case boundary and requires manifest refusal. The old CLI-helper substitution control
is preserved. Existing runtime and pre-promote helper bindings remain intact. The TLS
listener topic changes different renewal source regions and no capacity source is
changed here.

Validation: a 123-method selected suite passed without skips on managed Python 3.14
and macOS Python 3.9 with `-I -B`: `test_renewal_image_digest`,
`renewal_follow_test.ImageDigest`, `test_renewal_tail_cleanup`, `test_history_file_hash`,
`test_runtime_file_hash`, `test_check_runtime`, `test_history_cli_status`,
`test_history_main_cleanup`, `weekly_reset_test`, and the two explicit pre-promote
helper methods of `test_version_ledger.ReleaseRecordTests`. After adding the same-FD
metadata control, the final ten-method `test_renewal_image_digest` suite also passed
on both interpreters. `git diff --check` is clean. Independent integration/custody
review approves the exact-path dependency and source bindings. Independent hostile
review approves the implementation and passed 46 focused methods on each interpreter
without skips. Root reviewed and approved the production changes and decision note.

Limits: cache entries are trusted snapshots previously produced by this reader, keyed
by filesystem metadata; a hit is not a new content hash. They depend on that metadata
accurately reflecting writes, and the cache remains private to the caller. Neither a
cache hit nor a fresh digest establishes pathname continuity, kernel executed-image
identity, process custody or build certification. A finite replacement control
explicitly shows metadata and digest can consistently describe the opened file after
its former pathname names another file. All new controls use owned temporary files
or inert controller/listener seams; no application, PTY, socket, provider or compiler
was launched. Genuine native qualification boundaries are unchanged.

Root composition `59fc806fca6ef96379b163c881da04f26d05f2d9` also includes the
independently reviewed plain/TLS startup topic and concurrent main's diagnostic
test changes. All 386 selected Python methods pass in 29.543 seconds, with no
failures, errors or skips, on managed Python 3.14 with `-I -B`. The exact source
remains clean and unchanged. Discovery alone finds 1,933 unique IDs, no discovery
errors, seven new listener and ten new image-digest identities; it executes no
test body or native prerequisite. The selected scope retains the two named
pre-promote fixture controls and does not replace native-required qualification.
