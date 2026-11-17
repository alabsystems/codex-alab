<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Qualification record and log snapshots

Decision: read qualification metadata from one bounded, stable regular-file
descriptor and stream the log hash through the same descriptor owner. Reuse the
standalone `file_hash.py` support instead of adding a smoke dependency or another
file-owner policy. The base is
`7f341fe75339ab78d51c2bd205d0b59470ebc622`.

Regular-only metadata/log inspection is a new restriction: the old inspector
refused leaf symlinks but did not reject other special file types before IO.
The observer's exclusive writers produce regular files, and the new readers
require that artifact shape. This is not an unchanged regular-file guard.

The common owner checks native no-follow/nonblocking capabilities before opening.
If either required flag is unavailable, it raises the caller's ordinary
`error_type` (default `ValueError`); there is no zero-flag, legacy-reader or
untested platform backend fallback. The optional status reader retains its
independent source observations, reports the unavailable capability and leaves
`qualified_sha` unknown. This is an explicit capability limitation, not a claim
of Windows native validation.

## Public counterexamples

The read-only baseline reused `test_qualification.QualificationTests` by module
alias: real temporary Git repositories and the fixture's finite local shell gate
that prints fixture output. These are local observation fixtures, not a native
build, application run or authenticated attestation.

Nine public `qualification.inspect` observations established the problem. Real
reads were capped at 8 KiB before finite physical rewrites of owned regular
files; bytes, hashes and stat results were not manufactured. A complete original
log and its complete replacement each failed the record's expected hash, while
the original first block plus replacement remainder was accepted. A complete
original record with status 1 and a complete replacement with `clean:false` each
failed, while the mixed record passed. Both rewrites preserved file size and
changed real modification/change times. Six finite appends also made the old
reader consume 50,234 metadata bytes from an initial 1,082-byte file and 65,536
log bytes from an initial 16,384-byte file. All instrumented handles were closed
before fixture fallback. No thread or FIFO writer was needed.

Four maintained public regression methods produce five failing controls, zero
errors and zero skips against the unchanged old production module on both Python
3.14 and 3.9. That baseline uses the maintained fixture with only its unrelated
status-tool import made local to the two tests that need it; there is no TOML
shim. Private scripts and observations are under
`.codex/.ubd/qualification-inspect-read-audit-20261116`.

## Implementation and compatibility

One private descriptor owner serves the existing hash/cache wrappers and the new
noncached byte-snapshot wrapper. It opens with no-follow/nonblocking flags,
requires a regular file, reads at most initial size plus one, and compares the
same six stat fields before and after reading. The selected read/stat/cancel
failure survives secondary close/diagnostic faults. Close-only native failures
retain identity; success-shaped close exits remain definite failures with the
exact cause. Existing hash messages, snapshot return values, cache-hit stat
checks and cache publication only after successful close are preserved. The
ownership trace test now targets the actual common FD owner.

`qualification.inspect` captures record bytes once, then decodes with
`TextIOWrapper(BytesIO(...))` using the old default text/newline semantics. It
does not use `json.loads(bytes)`, which would add BOM/UTF-16/UTF-32 detection.
Both the inspector and the observer's closed-log writer use streaming hash
snapshots. Existing schema, exact integer status, source/Git identity, log hash,
output fields and explanatory limits remain unchanged.

There is no new absolute byte cap, timestamp/expiry rule, origin-directory
binding, receipt or required field. Minimal records, arbitrary record names,
relocated/copied directories and parent aliases still work; final leaf symlinks
remain refused. Large regular metadata is accepted. Exact sibling loading adds
the helper to each isolated recorder-copy fixture. The release gate already pins
`file_hash.py`, and source observation covers the whole committed tree.

## Selected checks

The new public IO module has 10 methods, including both mixed-file refusals,
initial-size growth bounds, the observer writer, decoding, minimal/copy/alias
compatibility, leaf-symlink swaps and metadata larger than 1 MiB. The new byte
owner module has 14 methods and 52 finite controls with actual read/stat/close
failures, optional hostile diagnostics and cancellation at the real owner.
Returned FDs are observed closed with `EBADF` before owned fallback.

The 50-method component selection (`test_file_bytes_snapshot.py`,
`test_qualification_io.py`, `test_runtime_file_hash.py`,
`test_renewal_image_digest.py`, `test_renewal_matrix_digest.py`) passes on both
managed Python 3.14 and system Python 3.9 with no skips. Existing qualification
checks pass 38 methods on 3.14 and 35 on 3.9: both omit the raw-self-signal test;
3.9 additionally omits the three status-tool tests that require Python 3.11.
The existing 41-method cleanup module passes on 3.14. Its 30 note-dependent
failures on 3.9 reproduce unchanged on the base, so it is not included in the
supported 3.9 selection and is not changed by this topic.

Five targeted copied-recorder fixture cases pass on 3.14: the daily linked-tree
copy, actual sync/cache registration and three upstream-admission cases. The
lint-step fixture cannot start without the actual `codex-alab-unit-runner`
artifact; that prerequisite was respected, with no fake artifact or build
override. These are component checks, not a full Python workflow or native gate.
They use local temporary Git and finite owned shell/Python fixture commands;
no actual native application/compiler/guardian, remote operation or raw PID
signal was run.

The descriptor bookends describe each opened file separately. They do not bind
future pathname contents, atomically freeze record and log together, authenticate
the record, or certify the application/native gate. Existing local-observation
limits continue to apply.

Final review: the hostile reviewer independently approved all 11 paths and
passed 88 unique selected methods on 3.14 (50 core plus 38 qualifier) and 85 on
3.9 (50 core plus 35 qualifier), without failures, errors or skips. The
integration reviewer independently reviewed production, public IO/status,
copied-input and trace changes, and this note; that reviewer passed the full
134-method component selection on 3.14 and the supported 85-method selection on
3.9, also without failures, errors or skips. The integration reviewer contributed
the 14-method/52-control byte module and does not claim independent review of
it; root and the hostile reviewer covered that module. Frozen source hashes
matched before and after independent runs. Root reviewed all paths and approved
the commit; only this review attribution changed after the final freeze.

## Composed validation

Root tested clean composition `fefbdafe7b2d233262ee747b966d0c40cdeadb98`,
which preserves incoming main `f31682dda8f0f126107df3ac62f115d6d30adb0c`.
All 615 selected Python methods passed in 68.443 seconds on managed Python
3.14 under `-I -B`, with zero failures, errors or skips. Source HEAD bookends
match and the worktree remained clean. The selection includes the prior smoke,
reader, output-owner and runtime controls; the new byte/IO modules; 38
qualification methods; eight qualification CLI methods; 41 cleanup methods;
two pre-promotion helper-binding methods; and five copied-recorder cases. It
excludes the actual self-SIGTERM method and four capacity-process guardian
controls. This is a selected component result, not a complete Python workflow
or native/application qualification. The retained private log is
`~/.codex/.ubd/qualification-io-composition-tests-20261116.log`.

Separate discovery at the same source found 2,102 unique IDs and zero import
errors; no test bodies or native prerequisites ran during discovery. Fresh main
`4a160d8320ff63958e13ff56317ab68da2644be8` adds only two documentation paths
and is preserved without changing selected Python inputs. Its separately
recorded complete workflow at `f42fc6b92cec1664ae1e5340b10916608b6f8cb6`
retains that source attribution and does not qualify these new reader changes.
The actual native resource query still reports `CPU slot file identity changed`;
no application build, package or installed adoption was performed for this topic.
