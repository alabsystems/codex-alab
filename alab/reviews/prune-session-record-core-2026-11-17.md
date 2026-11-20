<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Unrecognized session records defer package collection

Four direct public-CLI probes at `f80894064917e6ce2d2e6aca44d34c7278e03240`
exposed a recognition gap. A minimal v1 Exec record naming an old executable
kept that package; a valid pathless Exec record allowed ordinary collection.
However, `{}` and `{"v":1}` also authorized collection while their standing
record bytes remained unchanged. Native `Record` and `record::read` require
typed core fields and schema v1. This native interpretation is source-derived:
no Rust decoder ran, and these malformed bytes do not prove a recoverable
restart or an accepted native record losing a recognized path.

Decision: recognize the stable twelve-field v1 core and the three executable
path shapes before extracting retention paths. Unknown schema, missing core,
wrong types, out-of-range unsigned integers, invalid Unicode scalar strings,
and malformed consumed path fields defer collection. Valid pathless records,
null or empty path options, relative strings, zero/maximal unsigned values and
unrestricted native String values retain their existing meaning. Unrelated
optional fields and extensions stay outside this collector's projection. This
does not claim full native optional/custom-codec or JSON-parser equivalence.

One shared twelve-key JSON fixture anchors Python deletion tests, the existing
process-retention smoke and native record contract tests. The Rust fixture
adapter preserves the existing recovery and in-flight wait payload literally.
Two added native contract tests cover the shared pathless core, required core
keys and the file reader's separate schema check. Those Rust tests remain
unexecuted; static fixture/source checks are not compilation or qualification.

The same final Python tests against an exact four-file Git-object export of the
old production code produced 70 deletion-effect assertion failures in three
parents; both valid-control parents passed. The candidate's single four-module
run passed all 81 unique methods in 29.7828 seconds, with no failures, errors,
skips or expected/unexpected successes. All 417 Python/shell/contract/fixture
input hashes, four old-export hashes and HEAD stayed equal within and between
the final paired phases. An earlier baseline is retained separately because the
existing smoke's partial record fixture was subsequently completed.

The existing smoke then passed in 2.8259 seconds. Its owned ad hoc signed copy
of `/bin/cat` proved first execution, remained protected while running and was
reaped before scratch removal. Marker, shim, owed-record, unused-package and
unreadable-state controls passed. All 417 inputs matched the candidate run,
and no smoke fixture remained. It did not execute Codex or a release gate.

Root independently verified the ordered 81 terminal OK results, strict old Git
blobs, input bookends and whole production-file reversal. A peer independently
reviewed the native core bounds and positive cases. Incoming qualification CLI
output changes are separate: all existing functions/imports/constants remain
unchanged, and these four modules do not import or execute that CLI.

Evidence: `~/.codex/.ubd/prune-record-shape-9fcydia2/` and
`~/.codex/.ubd/prune-record-core-fix-g7uycpr4/`. Candidate log SHA-256:
`e51cd29799b4f9e9897ffa904ef71684a5c7dfe0384000bcf6c44787919e5802`.
Smoke log SHA-256:
`4f0a0ea7d0cba20b1e5360cf8de43225aacabf742392e94148b716ca6f1e662b`.
No installed package was deleted. Source-selected certified native build,
package and runtime qualification remain unavailable; this change does not
qualify previously resource-blocked controller lifecycle bodies.
