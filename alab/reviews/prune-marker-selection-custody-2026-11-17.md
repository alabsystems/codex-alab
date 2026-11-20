<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# An unreadable selection marker cannot authorize package deletion

On source `4a12d0b8e8dbd7650610a3b41aa908dc92707144`, the maintained public
pruner deleted two older owned package fixtures after reading a marker with
duplicate `checkout` keys. The first named the older selected package; the
second named the newest package. A strict TOML parser rejected that marker,
but pruning silently took its last checkout value. Valid old-selection and
new-selection controls established the actual deletion behavior, and all
marker bytes remained unchanged. The command used only owned package/prefix
fixtures, an owned existing store lock and an empty session home, with no
aterm on its PATH or installed package-manager lookup.

Decision: reuse `promote.parse_link_marker` instead of maintaining a second
permissive marker parser. Its normal TOML parser and existing older-Python flat
fallback both reject duplicate or malformed input. Check absolute string
checkout, optional list-of-nonempty-string bins and any supplied schema/program
identity before using the paths. Existing checkout-only manual markers remain
valid. Reject invalid UTF-8 specifically for selection-marker reads; ordinary
shim/prose replacement decoding stays unchanged. An unreadable standing marker
still returns the existing unknown-selection result before any package
deletion. Store-lock, process/session custody and pruning policy are unchanged.

Two maintained public-CLI regression methods cover actual deletion effects.
Eight malformed selections must preserve every package: duplicate checkout,
invalid UTF-8, malformed TOML, string/mixed/empty bins, boolean schema and another
program's marker. Valid canonical old/new and checkout-only old controls still
protect the selected package while allowing genuinely unused packages to be
removed. With those same final test bytes and unchanged production code,
managed Python 3.14.8 reported eight assertion failures across two distinct
methods, actual exit 1, zero errors/skips. The positive controls passed.

The initial four-module candidate run executed 68 unique methods in 24.377
seconds: 67 passed and one failed, with zero errors or skips. All 44 methods in
prune, promote and promote CLI passed, including the new deletion controls.
Retention passed 23 of 24. Its existing
`test_contained_exact_revision_reuse_needs_no_git_write` failed when the real
macOS sandbox refused a private-index directory under its supplied scratch.
An isolated current-source run and a strict Git-object export of unchanged
`4a12d0b8` ALab source both reproduced that same failure with the same
TMPDIR/tempfile-parent semantics. The exported 1,062 source files and both
standalone runs' input hashes are retained. This establishes a pre-existing
failure, not a passing full suite: that historical run remains failed. No profile,
permission or native-resource rule was changed or bypassed.

The separate fixture correction in
`retention-scratch-read-admission-2026-11-17.md` admits its exact already
writable scratch for reading and proves unrelated owned HOME data remains
unreadable. With that fixture-only correction, one final four-module run
passed all 68 unique methods, actual exit 0 in 23.180 seconds, with zero
failures, errors, skips, expected failures or unexpected successes. It changes
no production profile, native-resource policy or permission grant.

HEAD, all 240 ALab Python input hashes and both shell-controller hashes matched
before and after each candidate run. The final actual 68 OK result IDs match
the frozen selection in order. An independent subagent approved the parser/test diff.
Evidence is under `~/.codex/.ubd/prune-marker-custody-review-0r6ilrt4/`:
the original public reproduction, `baseline-final.*`, `candidate.*`, `candidate-final.*`, strict
base source manifest and `retention-*-standalone.*`. Fixture bytes remain in
verified archives beside those records. No installed managed-prefix prune,
package executable, Rust compiler, certified build or release pipeline ran.

An independent reviewer verified both counterfactuals and every strict-export
file against the exact Git source objects. It also ran two actual public CLI
controls using system Python 3.9.6 with no `tomllib`: a valid selection retained
its package and pruned unused data; a duplicate marker preserved everything.
Both returned the existing exit-0 contract and left their marker bytes intact.
Their source hashes and HEAD stayed unchanged. These bounded compatibility
controls do not rerun the unsupported full test suite on Python 3.9.

Production SHA-256:
`978dcd4c5b1413a410cebb8eee533de184a14b9b1f268bad36fd9f8445bf5d19`.
Final test SHA-256:
`1bf514fba2ee265d0f05cff034c4ddb06c1daa0b145d7e01afda9cc621907523`.
Final full-run log SHA-256:
`0e6a5037e1b4310ec76a6b2d7b20f85ffc6777769d63939c53b857de636d3bac`.
