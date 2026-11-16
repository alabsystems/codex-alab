<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Bind delta policy manifests to the measured source

Review after `09a96ca9ff` found that summary and patch reports compared committed
trees but loaded derived-file exclusions and allowed deletions from the current
working tree. An uncommitted manifest edit, or checking out newer manifests, could
change a report for an unchanged historical source.

Decision: resolve the requested source to one commit before selecting its baseline
or reading trees. Read `alab/merge-attributes` and `alab/removed-paths` from that
commit. An absent optional manifest supplies no entries; a read failure supplies
no report. Reject symlinks and directories as source manifests rather than reading
their target names or tree listings as policy. Keep the existing manifest syntax
and the separate committed hook-topic reader unchanged.

The original regression failed in four subcases before implementation: summary
and patch reports changed after dirty manifest edits and after a later manifest
commit. All four now remain identical to the original selected-source report.
Additional cases cover untracked manifests when source policy is absent,
nonregular source entries, and an injected Git blob-read failure that must not
become an empty successful patch.

Validation with atpkg Python 3.14: 25 delta-tool tests and 27 feature-inventory tests
passed. `bash -n alab/delta.sh` and `git diff --check` passed. Changes are confined
to ALab-owned tooling, tests and this record; no application build is claimed.
