<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep channel status writes in the admitted directory

Base source: `84004fdead048821a1c470fca6e7cf08b82c5768`.
The reproductions and checks used temporary package/channel fixtures only.
No live update, fetch, build, installation, or account operation ran.

## Reproduced problem

The display context validated a selected package and retained only its channel
pathname. A later status write followed that pathname again. Renaming the
admitted channel aside and replacing it with a symlink to an unrelated directory
made `record_channel_status` overwrite that directory's sidecar and return true.
Replacing the path with another ordinary directory had the same result.

The new destination regression failed for both replacements on the base source:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_local_update_job.py -k test_channel_display_refuses_replaced_destination -v
```

## Decision

Retain the admitted channel's device and inode alongside the existing display
context. Check that it remains a directory and that admission did not replace
it. Directory timestamps and size are deliberately not identity: normal package
publication changes the entries while this context remains valid.

Each write opens that directory without following its final symlink and checks
the opened identity. The temporary file is created exclusively, mode 0600,
relative to the held directory descriptor. Publication and temporary cleanup use
that same descriptor. The named directory is checked before and after publication;
observed replacement returns false. No descriptor survives a single write.

Review also caught a temporary-file ownership error in the first implementation:
assigning the cleanup name before exclusive creation could remove an existing
file on collision. The writer now claims cleanup ownership only after the open
succeeds. A deterministic collision regression failed before this correction
and confirms that the existing file remains unchanged afterward.

Status remains a best-effort, nonsecret observation. A failed or redirected
display write does not change transaction success or selection authority. A
rename after publication can still move the already written file; this code does
not lease the directory or claim an atomic snapshot across the whole update.

## Validation

Final commands:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_local_update_job.py -v
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_local_update.py -v
/usr/bin/python3 -I -B -m unittest discover -s alab/tests -p test_local_update_job.py -k channel_display -v
git diff --check
```

- Python 3.14.5: all 44 public-job fixture tests passed in 60.446 seconds.
- Python 3.14.5: all 18 local-update tests passed in 12.529 seconds.
- System Python 3.9.6: all six display tests passed in 2.540 seconds.
- Diff whitespace check passed.

The display cases cover symlink and ordinary-directory replacement, rename
during writing, repeated valid writes across new package publication, temporary
name collisions, failed stream construction, and descriptor/temporary cleanup.
The existing failure/interruption case still verifies that display failures do
not replace transaction outcomes or publish a partial record. The broader
fixtures use inert builders and selectors and local temporary Git repositories.
