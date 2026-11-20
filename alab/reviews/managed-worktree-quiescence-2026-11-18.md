<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retain original managed-worktree creation until publication

Decision: register the existing managed-worktree task with its actual Session
work ledger. Its detached outer task previously escaped a ledger capture even
though blocking Git creation and the original `Created` event were still pending.
The same omission also excluded that task from normal Session shutdown joins.

Commit `58f0c286264eb2691ddb8fb860e5032af22e5615` changes one marked upstream
scheduling call to the existing all-platform `alab_spawn_original_work`. Every
other byte of the original body remains unchanged: the same outer future owns
the same manager, source and request data, awaits the same blocking Git operation,
maps its actual result, and publishes the same completion event. Observer
cancellation does not abort, recreate or resubmit creation. This is quiescence
in the original process, not migration of a Rust future across images.

One maintained native invocation on 58f0c286 passes four exact TUI methods:

- The new actual-entry test starts a genuine embedded session and uses its real
  returned primary state. After joining the same ledger's baseline, it enters
  actual managed-worktree creation on a current-thread runtime and immediately
  observes the queued task. Canceling only that observer preserves the original
  task. Its later join must include one real checkout and the original queued
  `Created` event, with unchanged source HEAD and committed content.
- Existing transition coverage checks ownership and preservation of fork history.
- Existing source-builder coverage checks cleared destination state and retained
  original overrides.
- One background-terminal error method covers five existing blocker/acceptance
  cells; it counts as one method.

The fixture owns its repository, home and explicit desktop worktree allocation
root. Per-command Git setup removes inherited selectors and restores isolated
global/system configuration, hooks, attributes and fsmonitor settings. It changes
no global environment/configuration and introduces no Git hook or artificial
blocking barrier. Canonical checkout root/cwd containment is checked before
content reads. Cleanup retains the source and actual event receiver until the
original effect has completed; unresolved cleanup retains the owned directory
and reports failure rather than removing input under a possibly running task.

Source review caught the original fixture's missing Git-config isolation before
execution. Separately, the first launch attempt failed shell redirection into a
sealed plan directory before invoking any command, guardian, Native prerequisite
or test. The corrected plan preserves that immutable preparation record and
places runtime output in a separate private writable directory. Root rechecked
its mode, ownership, absence and an exclusive create/remove probe before launch.
No permission boundary, protected profile, compiler flag or deadline was changed.

Actual process 35847/start 73b8ef/completion 51f2ca exits zero. Test, collector,
coverage, source-observation and guardian statuses are all zero, with four passes,
no failures or ignored methods, and three complete authenticated library harnesses.
Only the TUI harness has matching methods; empty-sibling order is not inferred.
Source-selected Trust 15698 executes the maintained unverified test lane through
the existing project admission and protected profile. Before/after books cover
12,513 source inputs and 11 tools and are identical, SHA-256
`c9edcb67571bb5f204ac71135bb0b98c888dfa7618c85ac92fe04d595f4b63f4`.
Raw log SHA-256:
`1b23b0ae26f4c81affa69b05a52b00a7956b0882e9d6ca47b7a46971e781be3d`.
The private saved packet contains 51 payloads and 12,751,487 bytes, index
`0d2c22c64f3439921f177dc006e82947bf75253c1c165bf5f5fdcead7c3e34a1`.
It retains both preparation plans with their distinct execution status, twelve
selected Git source blobs, source/tool books and actual results. Opaque native
metadata is only hashed; no compiled helper, artifact or cache is copied.

Only two ALab test files were formatted. The upstream hook is +1/-1, risk 10
and churn 5 against integrated upstream ce3031018e. The test measures canceled
observation before the queued task's first poll and its subsequent real effect;
it does not inject mid-Git cancellation, apply the completion to a replacement
App, establish whole-scene installation, grant Native authority or certify an
application. The enclosing scene cut still needs custody of the actual App FIFO
and every other owner. The [merged gate record](merged-wide-lint-2026-11-18.md)
retains its failed result independently of these four focused passes.
