<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Duplicate retention fields cannot disappear during JSON decoding

Four direct public-CLI probes at `6c83a3eccfd5960273b516c1dea5e7b0d38af2b0`
confirmed a second record-recognition gap. A valid core with its sole old `exe`
kept that package; a valid pathless core collected it. A raw duplicate `exe`
changing old-path to null and duplicate `v` changing 2 to 1 also collected old
and unused packages. Ordinary JSON decoding discarded the earlier member before
core recognition. All CLI exits were zero, and the canonical 0600 records kept
their exact bytes, identity and timestamps. All 258 declared ALab Python/shell,
shared-fixture and native-record input hashes matched before/after/current.

Decision: use the standard JSON pair hook to retain duplicate decoded keys as
an attribute of a dictionary subclass. Preserve ordinary last-value map behavior,
then reject repeated root members only for the existing twelve core and three
consumed executable-path names. Identical values and escaped-equivalent names
are repetitions too. Nested String environment-map keys, unknown nested objects
and unknown root members retain their existing behavior. Attribute metadata is
separate from input dictionary keys. The core value recognizer and bounded file
reader stay unchanged; no lexer, dependencies or full schema registry is added.

Native `Record` derives `Deserialize`, and its file reader directly decodes that
struct. Duplicate known struct-field rejection is source-derived here; no native
codec ran. This change does not claim complete optional/custom-field or JSON
syntax equivalence, nor a recoverable restart from the malformed proof bytes.

Two raw public-CLI test methods cover 30 identical/escaped repetitions across
all fifteen fields, seven destructive path/version transitions, and five positive
variants. With the same final test bytes, an exact four-file Git-object export
of the old production code failed all 37 deletion-effect assertions in one
parent; the positive parent passed. The sole candidate run passed all 83 unique
methods across pruning, promotion, promotion CLI and retention in 31.5947 seconds,
with no failures, errors, skips, expected failures or unexpected successes.

HEAD, all 420 declared inputs (402 tracked Python, twelve ALab shell controllers
and six explicit contract/data/license files) and four old-export hashes were
stable within both phases. Only production `prune.py` differs between phases;
test bytes are identical. Root independently reconstructed the actual ordered
terminal OK results, verified old Git blobs and reversed the whole production
file change. Peer source review approved the narrow root projection and controls.
Shared fixture, Rust test adapter and existing process-smoke source are unchanged.

Evidence: `~/.codex/.ubd/prune-duplicate-record-proof-20261117-kx45zkb8/` and
`~/.codex/.ubd/prune-root-duplicates-0f1al0w_/`. Candidate log SHA-256:
`20a6fe3ad675fe478c01082e4a55d64096c6ba6cc2ddb10ceb28360ecf07fa35`.
Original proof worlds were entry/mode/content/no-holder/identity verified and
retired into archive SHA-256
`33aa63845e19bb431e49e387d95a2cebbb98f0f82fc3668b04e065d74447fa52`;
raw evidence and source snapshots remain outside the archive too. No installed
package, Rust/compiler, Codex application, native gate or release was exercised.
Previously resource-blocked controller bodies remain unqualified.
