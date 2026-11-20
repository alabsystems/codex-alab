<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Unknown marker presence cannot authorize pruning

Three direct public-CLI probes at `c688df9a9b82156e281c1fc3cdd214e6304d821b`
confirmed a deletion bug. With an owned `links` directory at mode 000, the actual
kernel returned `EACCES` for its standing old-selection marker. `lexists` hid that
error as absence. A readable shim naming the newest package then made selection
appear known, and the pruner deleted the old package still named by the marker.
Restoring the directory to 0755 revealed the unchanged marker and shim. Readable
marker and genuinely absent-marker controls produced the expected different
deletions. No fault injection, race or store-lock bypass was involved.

Decision: inspect the marker explicitly with `lstat`. Only `FileNotFoundError`
permits absence and the existing fallback sources. Other inspection errors make
selection unknown and defer before consulting those sources. The diagnostic
says the marker cannot be inspected, without claiming it exists for every error.
Successful inspection uses the unchanged descriptor reader and strict parser.
No parent-path, inode-recheck, lock or selection policy is added.

Portable maintained public-CLI tests inject only the owned marker's `lstat`
`EACCES` and `EIO` observations, separately from the retained actual permission
proof. With the same final test bytes and unchanged production, three methods
reported two assertion failures in one parent; both readable and genuinely
absent controls passed. The error cases exposed actual unwanted deletion.
After the narrow fix, one four-module run passed all 76 unique methods in
25.706344 seconds with no failures, errors, skips, expected failures or
unexpected successes. Marker and shim bytes stay intact, and the controls still
remove genuinely unused packages.

HEAD and all 398 Python plus 12 shell input hashes matched within both phases.
Only `alab/prune.py` differs between baseline and candidate; final test bytes are
identical. Root independently reconstructed all 76 actual verbose identities and
terminal OK results in order and verified the whole production-file replacement.
A peer independently approved source and evidence. Owned fixture bytes remain
in verified archives beside raw results, logs and source bookends.

Integration preserves concurrent qualification-record publication changes. The
`checked_root`, `git` and `worktree_clean` functions used by retained-source
observation remain byte-identical. A separate run of all five qualification
test modules at `e7d4aad023181269f2b96b8d00f1fa7a94e61742` passed 109 unique
methods in 61.407754 seconds, with no failures, errors or skips. All 399 Python
plus 12 shell hashes matched before/after and current; root independently checked
the actual identities and terminal OK results. Its gates are finite owned fixture
scripts, not the application's native check. The original 76-method run retains
its actual source identity. Evidence: `~/.codex/.ubd/qualification-merge-check-e7d4-h6kj5_6a/`;
log SHA-256 `9bdc2a30587576084360586949f4682b8f607899b7e10148e9866d4399e5c995`.

Evidence: `~/.codex/.ubd/prune-marker-presence-zzeva201/` for the real kernel
proof, and `~/.codex/.ubd/prune-marker-presence-fix-j1mdundo/` for maintained
regressions, execution and reviews. Final log SHA-256:
`736f6749c28538ed33312c890ae9534b336c504f04188b237d699de82afc33cf`.
These checks used owned prefix/package/Git/kernel fixtures. No installed package
was deleted and no compiler, native application gate or release ran. Existing
resource-blocked controller coverage remains unqualified.
