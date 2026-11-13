<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Retain committed terminal custody through normal teardown

Normal TUI teardown drains reports, flushes kernel input, and resets terminal
protocols. A captured parser cannot protect keys typed into that kernel queue
while source capture and exec run. Terminal ownership needs a conditional handoff
that survives the actual exit guard.

The mode owner now uses Core's revocable native admission state. Every mode
entry, exit and adoption registers before waiting for the terminal boundary.
New operations revoke prepared transfer, even when the resulting mode would be
unchanged. Preparation retains an actual transfer token and a separate data
snapshot. `commit_if` re-reads native identity and modes under the terminal mutex,
then holds admission through the caller's final synchronous decision. A refused
decision retains rollback; a revoked attempt never invokes that decision.
Successful commit permanently refuses new mode operations. Snapshot decoding
alone never commits this source state or grants SDK/executable authority.

A marked hook in the real exit path calls ALab-owned terminal glue. Only an
actual committed mode owner preserves input and protocol modes; it pauses the
real event broker and still releases stderr ownership. The existing stderr
component synchronously restores a saved descriptor, rather than draining a
worker. Normal and panic exits otherwise retain their existing cleanup. No
legacy pending-upgrade flag or serialized claim enables the handoff.

The committed owner has explicit fatal-exec recovery that restores the original
terminal without reopening admission. Panic cleanup can likewise restore the
actual retained terminal. After recovery the normal exit path runs again, while
new mode entry remains refused.

The controlling-PTY TUI case still proves cancelled capture-waiter ownership and
partial-paste rollback. It then commits real input and mode guards together,
queues CR/LF/Ctrl-C bytes, invokes the actual `TerminalRestoreGuard`, and checks
that its kernel byte count is unchanged and raw mode remains enabled. It reads
the exact bytes, restores after an aborted exec, and exercises normal cleanup
without reopening source admission. The separate same-PID exec test now also
covers refused decisions, revocation, committed-operation refusal, and caught
panic recovery of the original configuration.

Source-selected Trust 15698 checks passed: the real same-PID mode case and all
52 focused TUI cases. The isolated completion-order helper is invoked by its
parent and ignored as a standalone entry. All-target Tippy for input, Core,
ALab TUI and TUI has no owned, hook-spanning, failing-target or missing-script
diagnostics; the repository classifier sets aside 24 upstream diagnostics.

This does not enable full automatic transfer. Production still uses its legacy
request and active-goal guard. Full transfer must bind these actual terminal and
input owners to Core/goal/SDK/host admission, require a target that restores the
whole capsule, and retain all state across exec or startup failure. The new TUI
case proves actual teardown preservation and abort recovery; the separate mode
case proves same-PID mode adoption. They do not prove a complete published
all-root live upgrade or source SDK authority in a replacement.

## Combined main qualification

Main advanced with `01f3fdb5ad`'s account-reconciliation observation change,
which was merged without conflicts. Actual combined source `bb7e9b5319` passed
the same-PID component case and all 77 selected TUI/account cases. These include
the real exit-guard kernel-input and abort-recovery assertions. The final
all-target lint classification is clean, with 24 upstream diagnostics set aside.
This is 78 distinct scoped passing cases, not the full release gate.

Main then advanced with `d64acb4320`'s account-view refresh ownership and
`59c750fca6`'s clock-format preference correction. Both were reviewed and merged;
no terminal/input logic overlapped. Final combined source `0340e161c4` passed the
native component case and all 81 expanded TUI/account cases, including refresh
ordering and clock-format cases. Total final scoped coverage is 82 passing cases,
with the same parent-invoked isolated helper ignored as a standalone entry. Its final all-target lint classification is
clean, with the same 24 upstream diagnostics set aside.
