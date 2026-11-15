<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retain normal guardian command completion

The completed Python workflow retained three successful native-context tests
and its untouched foreign-build sentinel, but the original execution handle
was unavailable after context compaction. Its outer controller exit status
therefore remains unknown. Preserve that record; this change does not fill it
in retrospect.

Decision: extend the existing command guardian's optional `--result-file` to
normal, non-cache commands. Reuse `run()` and its surviving custodian unchanged,
rather than introduce another process supervisor or Python workflow framework.
The caller continues to choose its authorized command, source selection,
protected profile and persistent evidence directory. No lane, permission,
timeout, cache policy or application qualification is granted here.

The normal mode claims a fresh regular file exclusively before launching,
with mode `0600` and noninheritable receipt/directory descriptors. Only after
`run()` returns through its existing command-group retirement does it write a
`guardian-command-completion` record with `completed: true` and the observed
`normalized_exit_status`. File and parent-directory fsync and both closes stay
inside the publication failure guard. Caught publication failures truncate the
still-owned descriptor and unlink only the originally claimed inode. Replacing
that path cannot make the guardian overwrite or remove another owner's file.

The receipt observes the **supervised command**, not the eventual exit of its
receipt-writing guardian. Killing the guardian after a persisted completion
does not undo the already observed command outcome. The status uses the
existing normalization: ordinary exit 143 and a command killed by SIGTERM both
produce 143; the normal record invents neither a raw signal nor a timeout.
The existing inherited-timeout result format and cleanup contract are unchanged.
Cache guardians still refuse the new combination before launching or locking.

A reader needs the exact record kind/schema, completed flag and integer
normalized status; a complete nonzero status is a failed command. An absent,
empty, truncated or malformed file supplies no completed outcome. Uncatchable
termination can leave an incomplete claim, and arbitrary hostile/permanent
filesystem failure can prevent invalidation; caught cleanup errors retain the
original failure rather than return success. The receipt is a local observation,
not authenticated evidence, source admission, proof, or an observation of
descendants which deliberately escape the existing owned group.

On base `16c715bbc39a51b1558fbbd592326852c40d779d` plus this isolated topic,
managed Python with `-I -B` passes:

- `test_guard_result.py`: 14 tests in 0.439s, exit 0, no skips or errors.
- `test_command_guard*.py`: 31 tests in 0.703s, exit 0, no skips or errors.

The new cases use actual inert commands, a live lingering writer and real
guardian cancellation. They cover normal/nonzero/signal and timeout-shaped
statuses, the empty prelaunch claim, writer retirement, launch/observer failure,
existing file/symlink/directory refusal, inherited caller descriptors, claim
replacement and cache rejection. Controlled file/directory fsync and close
faults first witness fully written successful JSON, then require its
invalidation while preserving the original failure. Existing tests retain raw
inherited timeout/signal attribution, default/cache cleanup, post-fork failure
and guardian-lifeline coverage.

Logs are `~/.ubd/codex-guardian-receipt-result-tests.log` and
`~/.ubd/codex-guardian-receipt-existing-tests.log`. Python syntax
and `git diff --check` pass. No Rust, native application build, certification,
installation, push or merge was performed for this topic's validation.

## Protected controller integration

Composed source `f44bf4b62df285e85ad6c17ff4fc4690ea2b29bc` merges the
topic with the completed workflow record on main. Through the new normal
receipt option, run the source-selected protected native-context controller
with the authenticated adapter, source-pinned Trust, original 60-second native
deadline and foreign-build sentinel. All three native tests pass in 1.332s,
including process/FD/signal context and the Python startup negative; the
foreign-build control remains untouched.

Original exec 67519 exits 0. Independently retained
`guardian-command-completion` JSON records the exact supervised controller argv
and normalized status 0 with mode 0600. Source, fixture, controller and adapter
hashes remain unchanged, and the source remains clean. The guardian's observed
exit and the supervised controller's receipt are recorded separately. The old
workflow's unavailable status remains unavailable.

The private archive under the common Git directory is
`alab-validation/f44bf4b62df285e85ad6c17ff4fc4690ea2b29bc/guardian-receipt-integration-eph2ph_p`.
Its 40 indexed payload files retain both Python logs, native context observations,
the typed receipt and source/tool inventory. Index SHA256:
`23bea5ed3907e5a3ac59f9919d626c4e79c8722d38066723b58913b6507b7319`.
This is local regression and completion evidence, with no application proof claim.

Decision: land the optional receipt after both code reviews and this protected
integration. Future long-running controller checks can opt into fresh private
receipts; preserve original outcomes and keep cache settlement separate. There
is no reason to repeat the full Python workflow or Rust library/lint sweeps for
this isolated optional guardian path after its focused and live controls pass.
