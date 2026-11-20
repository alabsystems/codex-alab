<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep the small-file decision attached to the descriptor read

At `a27fb81de5e7302c0285b7ae234931587d101481`, the maintained public pruner
accepted a marker replaced after its regular-file `lstat` check. A symlink
replacement was followed; an oversized replacement was partially read and
parsed as valid TOML. Both allowed deletion of owned old package fixtures.
An already-present symlink correctly deferred deletion, and a valid regular
selection protected its package while allowing unused packages to be removed.
The genuine store lock remained held: these controlled mutations explicitly
ignore its protocol, rather than bypassing a cooperating atpkg writer's lock.

Decision: follow the existing session-record reader's descriptor pattern.
Open with `O_NOFOLLOW | O_NONBLOCK`, validate the opened descriptor with `fstat`,
read at most the existing limit plus one byte, and reject a result beyond that
limit. A replacement symlink is refused; a FIFO can be rejected as nonregular
without waiting for a writer. The descriptor's size and the returned byte count
cover both replacement and growth. NUL handling, strict marker UTF-8 and ordinary
shim decoding remain unchanged. No inode-recheck, parent-path or selection
policy is introduced.

The same final public-CLI tests against unchanged production code reported four
assertion failures among six selected methods, with two passing controls and no
errors or skips. They expose symlink replacement, oversized replacement, growth
after the size observation and an owned no-writer FIFO. The FIFO child was
bounded to two seconds and cleaned up by its owning subprocess call.

After the narrow fix, one four-module run passed all 73 unique methods in
23.791616 seconds with no failures, errors, skips, expected failures or unexpected
successes. Actual verbose OK identities match the frozen selection in order.
HEAD and all 398 tracked Python plus 12 shell input hashes matched before/after
each phase. The sole baseline-to-candidate input change is `alab/prune.py`;
final regression test bytes are identical. Root independently checked all 410
current hashes, logs and identities; a peer independently reviewed the reader.

The first publication was refused before writing because main advanced. A normal
merge preserved that work. Exactly three unselected Python fixtures changed,
each adding `sequential_output=False` to its direct API arguments; the tested
prune inputs and shell controllers stayed identical. A separate run at
`e551c5d3c7ec598853a6d201a724af8b7c15696a` passed all 40 methods in those
three modules, with no failures, errors or skips, in 0.556333 seconds. All 410
Python/shell hashes matched before/after and current; root verified all actual
OK identities in order. Evidence: `~/.codex/.ubd/unit-fixture-integration-ybe6zyug/`;
log SHA-256 `981bd92bfd35c880a66576175f93b07e1df0c0d313361e1685d099e9eab60c4b`.
The original 73-method run is retained with its actual source identity.

Evidence: `~/.codex/.ubd/prune-small-text-audit-_c5s_3ju/` and
`~/.codex/.ubd/prune-small-text-fix-75wh08mz/`, including original controls,
baseline failures, final results, source bookends and root review. Final log
SHA-256: `d329c618e45a6a935f93cd8f22f9f2662f5e8cc75aab2e163eec0e4bd7e09995`.
Retain those records and verified archives when retiring explicitly owned test
worlds. These checks used owned fixtures and real process/containment observations;
no installed package deletion, compiler, native application gate or release ran.
