<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Observe one package and one filesystem identity

Base source: `2aec2b1a0cea1d82b2034bb9eda0f933e4a32c6d`.
All reproductions and tests used inert package files and mocked process output.
No live process identity, account, or credential was inspected; no native build
ran.

## Reproduced gaps

The package reader opened its manifest before resolving and stating its image.
A fixture that renamed package A aside and moved package B onto the same path
after the manifest read produced A's source with B's executable inode and no
error. The individual file reads were valid, but their combined identity was not.

The running-image reader compared only the inode from lsof. Inodes identify files
within a device; the same number on another device does not identify the loaded
file. A deterministic substitution model with equal inode numbers and different
devices was accepted. This was a fixture model, not an observed live mount or
process failure.

Both new regressions failed against the base implementation before the change:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -k package_changes -v
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -k different_device -v
```

## Decision

Hold descriptors for the resolved package root, its `bin` directory, manifest,
and image. Open each child relative to its held parent without following links.
Record device, inode, mode, size, modification time and change time, then compare
both the held descriptors and their named directory entries before returning.
This keeps child lookups in the same observed directory tree and rejects detected
root, directory, file, and content changes. Every descriptor closes on success or
failure.

The shared bounded metadata reader now also exposes a descriptor-based reader.
It reads from offset zero without consuming the caller's descriptor, checks the
actual opened regular file before and after reading, and retains the existing
128 KiB limit. `promote.regular_bytes` keeps its existing path-based API and uses
that same reader; promotion transaction behavior is otherwise unchanged.

Request lsof's `D` field explicitly. The local manual's “OUTPUT FOR OTHER
PROGRAMS” section defines this as a hexadecimal device number. Parse it as an
integer, reject malformed or duplicate values, and require the selected image's
device and inode to match the opened package image. Other text mappings may omit
these fields; they cannot supply the selected Codex identity.

These checks remain local observations. They do not provide an atomic filesystem
snapshot or a lease, prevent changes after inspection, authenticate executable
bytes against the manifest's source, or prove source qualification. The report's
limits explicitly retain that distinction.

## Validation

Final focused commands:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -v
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_promote.py -v
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_upstream_status.py -v
/usr/bin/python3 -I -B -m unittest discover -s alab/tests -p test_artifact_status.py -v
/usr/bin/python3 -I -B -m unittest discover -s alab/tests -p test_promote.py -v
git diff --check
```

- Python 3.14.5: 18 artifact, 20 promotion, and 15 related status tests passed.
- System Python 3.9.6: 18 artifact and 20 promotion tests passed.
- Diff whitespace check passed; independent code review found no blocker.

New cases cover root, `bin`, manifest and image replacement; image and manifest
content changes; same inode on a different device; absent, malformed and duplicate
device fields. Existing multiple-mapping, older-running-package, no-follow,
bounded FIFO, and non-execution assertions remain in the passing artifact suite.
