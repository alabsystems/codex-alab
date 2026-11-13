<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep artifact identity observations within their inputs

Base source: `064060139ef48c5b07f30541e9ea4d45321bd1c1`.
The investigation and validation used inert temporary packages and mocked process
output. No live process identity, account registry, or credential was inspected,
and no native build ran.

## Reproduced defects

`artifact_status.py` ignored lsof's process identity and file-descriptor fields.
It also silently replaced an earlier inode or path field with a later duplicate.
Consequently, a request for PID 123 accepted records naming PID 999 and reported
PID 123 as running the fixture package. Records without a process or file header,
or with a non-text descriptor, could also supply a running source.

The package reader separately followed `alab-build.json` symlinks. With package
A's executable unchanged and its manifest linked to package B's manifest, it
reported B's source for A's image. The same reader supplies packaged, selected,
and running observations, so this affected all three stages.

## Decision

Parse the selected `lsof -Fni` stream as one process set followed by text-file
sets. The installed lsof manual's “OUTPUT FOR OTHER PROGRAMS” section defines
`p` as the process-set boundary and `f` as the file-set boundary, both always
selected. Require the requested PID, `txt` descriptors, newline framing,
absolute paths, and unique inode/path fields within each file record. Reject
unknown fields and incomplete records rather than guessing their identity.
Other text mappings may omit an unavailable inode; the selected Codex mapping
still needs its matching inode. Repeated identical Codex mappings remain one
candidate, while different candidates remain ambiguous.

Reuse `promote.regular_bytes` for the package's manifest. Its existing 128 KiB
bound, no-follow open, nonblocking open, and regular-file check reject linked,
oversized, and special-file metadata. The package root is already resolved, and
the manifest is its immediate child. Executable containment and execute-bit
checks remain in place.

These are local identity observations based on packaged metadata. They do not
authenticate package contents, prove source qualification or promotion, or run
the inspected artifact.

## Validation

Before changing production code, each command failed as intended:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -k wrong_process -v
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -k linked_manifest -v
```

The malformed-stream regression failed all ten subcases; the manifest-link
regression observed B's source where it required no identity.

After the change:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -v
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_upstream_status.py -v
/usr/bin/python3 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -v
git diff --check
```

- Artifact tests: **14 passed**, 0.268 seconds, Python 3.14.5.
- Related status tests: **15 passed**, 9.293 seconds, Python 3.14.5.
- Artifact portability check: **14 passed**, 0.133 seconds, system Python 3.9.6.
- Diff whitespace check: passed.

The FIFO regression uses a separate Python fixture reader with a five-second
hang detector, so a future blocking metadata read cannot hang the test runner.
The valid multiple-text-record case retains one unique Codex mapping without
requiring an inode from unrelated mappings. Existing tests still cover a running
older package independently of a newer selection, changed process start time,
replaced inodes, executable escapes, and non-execution of opaque shims.
