<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Stable feature metadata reads

Decision: use one strict descriptor snapshot and the existing default text JSON
decoder for feature metadata. Resolve only CLI inputs that previously followed
aliases; retain the gate's direct path and its existing guards. The base is
published `df22c89174965502d4125de11168909a711d33b6`.

## Public proof and source identity

The bounded audit used the existing `FeatureAdmissionTests.ready()` fixture:
real local Git/catalog/committed-review identities and a portable bundle, with
synthetic declared command status and test-log data. It ran no declared test
command, compiler, application or native gate.

Two component controls made eight public invocations including baselines. Both
`feature_evidence.main(validate, ...)` and `feature_gate.admission` accepted a
healthy record, rejected complete image A (integer schema 1, failed check status
1) and complete image B (boolean schema, check status 0), but accepted their
physical read mixture. Main returned 0 and its success message; gate admission
returned no errors for the fixture source. After the first real 8 KiB text read,
a finite same-size rewrite supplied the remaining bytes. Real size remained
33,896 bytes while modification/change metadata changed. The mixed record had
integer schema 1 and status 0 although neither complete image was admissible.
Its one real raw handle was closed with `EBADF` before fixture fallback.

The loaded source-file SHA-256 bookends were:

- `feature_evidence.py`:
  `66b27aa9102d44f94e578c382816be9f3b1257f3ed7a138af9f676ce0620abff`.
- `feature_gate.py`:
  `dac7c75b00d01ff8d13c966d0c4a60a75dfe3c92922d6bfd66fd17c851774c6f`.

These match the actual df22 Git objects, including the published log-snapshot
change. The private JSON's older `metadata_base:302f...` label is stale context
for the unchanged metadata paths, not the tested whole module or a whole-tree
freeze claim. Artifacts are under
`.codex/.ubd/feature-metadata-read-audit-20261116`.

The two maintained mixed-read/growth regression methods produce four expected
failed controls, zero errors and zero skips against those exact old source
hashes on managed Python 3.14. The growth controls use finite appended JSON
whitespace and establish the missing initial-size read bound, not a separate
semantic execution verdict. No record bytes, stat results or hashes are
manufactured by the physical-read seams.

## Interface and limits

`feature_evidence.read_record` captures one direct no-follow byte snapshot, then
passes `TextIOWrapper(BytesIO(raw)).read()` to `json.loads`. This preserves the
old `Path.read_text` default encoding/newline semantics and general JSON values.
It does not adopt raw-log JSON's BOM/UTF-16/UTF-32 autodetection. Raw log hashing
and parsing remain unchanged.

Only the CLI's existing input-load branch resolves its record first. This stays
after the exclusive snapshot-writing branch, preserving pending record creation,
relative inputs, arbitrary names, external files and final/parent aliases. Only
the resolve call maps older pathlib `RuntimeError` to `ValueError` with the
exact cause; reader errors and cancellation authority remain distinct.

The gate imports the shared reader and passes `root / 'record.json'` directly
after its unchanged root/record symlink and `is_file` guards. It does not resolve
that final path to follow a replacement link. No-follow opening refuses an
actual final-leaf swap after the prechecks. Absolute explicit bundle overrides,
copied external directories and ancestor aliases remain allowed; gate bundle
root and record-leaf aliases remain refused, and the fixed record name stays.

Regular-only metadata is a new CLI input restriction matching its exclusive
regular-file producer. The gate already had its regular-file precheck. Both
readers retain the shared initial-size-plus-one bound, six stable stat fields,
close policy and ordinary missing-capability refusal, with no weaker flags or
fallback. There is no new absolute byte cap, location binding, schema, command
status, timestamp/expiry rule, receipt or required field. No Windows native
validation or executed Python 3.11 result is claimed; the feature tools require
Python 3.11 TOML support and are tested here with managed Python 3.14.

The new helper symbol uses the gate's existing feature-module dependency. The
exact sibling file helper was already pinned by the release gate and present
in isolated gate fixture copies. No copied-source closure change was required.

## Verification

The new 12-method module follows the actual entry reader's globals, including
the function imported into the gate; it does not instrument another fixture's
separate SPEC-loaded feature module. Physical reads and file rewrites remain
real, every selected metadata file has one owner, and closure is observed before
owned fallback. Positive tests retain default text semantics, independent
temporary external paths, aliases/copies, pending exclusive writes and large
regular metadata. Negative tests retain exact nonobject validation results,
gate exclusions, capability refusal and post-precheck leaf-swap refusal. Local
Git is the only child command allowed by the new fixture; raw signals, sockets,
threads and other actor attempts are independently checked.

Managed Python 3.14 passes 103 unique selected methods in 45.187 seconds with
zero failures, errors or skips under `-I -B`. Standard loader patterns are
`test_feature_metadata_snapshot.py`, `test_feature_log_snapshot.py`,
`test_feature_evidence.py`, `test_feature_gate.py`, `test_feature_bundle_custody.py`,
`test_feature_lifecycle_contract.py`, `test_feature_scenario_contract.py`,
`test_feature_snapshot_ids.py` and `test_focused_results.py`. All selected IDs
are checked for uniqueness. Existing component fixtures use local temporary Git
and finite Python/shell commands, with no actual native application/compiler,
guardian, remote operation or raw PID signal. This is a selected component run,
not the full Python workflow or native gate.

Both independent reviewers approved all four paths, including the complete new
12-method module, production and note. Each independently passed the same 103
unique methods on managed Python 3.14, in 42.429 and 41.102 seconds respectively,
with zero failures, errors or skips and all frozen hashes unchanged before and
after execution. Neither reviewer authored this metadata topic. Root reviewed
all paths and approved the commit. Only this attribution changed after freeze.

These are per-file descriptor snapshots, not atomic metadata/log capture,
pathname continuity, authenticated execution declarations, semantic scenario
proof or full native qualification. Existing feature-admission limits remain.

## Composed validation

Root tested clean committed source
`65025be4bb79426a8ea7a31cf3a45658cd7f6965` with managed Python 3.14 under
`-I -B`. All 733 selected methods passed in 119.746 seconds with zero failures,
errors or skips. Source HEAD bookends matched and the worktree remained clean.
The selection adds the 12 metadata methods to the prior 721-method scope,
including all nine feature patterns above, the full upstream-admission module,
reader/output/runtime controls and two pre-promotion helper-binding controls.
The actual self-SIGTERM method and four capacity-process guardian controls remain
outside this selection. The retained private log is
`~/.codex/.ubd/feature-metadata-composition-tests-20261116.log`.

Separate discovery at the same source found 2,125 unique IDs and zero import
errors; no test bodies or native prerequisites ran during discovery. Fresh main
`45741dd67b0819128e5a2c9046b29416936f7658` adds two Rust paths and two review
notes. All incoming work is preserved, with selected Python inputs unchanged;
this Python result makes no qualification claim about those Rust changes.
The actual 04:59 UTC resource query still reports `CPU slot file identity changed`;
no qualified frontend or native unit runner resolves through the current PATH
or configured environment. No complete Python workflow, application certificate,
package or installed adoption was performed for this topic.
