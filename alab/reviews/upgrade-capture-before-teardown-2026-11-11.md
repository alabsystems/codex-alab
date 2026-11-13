# Capture before TUI teardown

The upgrade event previously had only a synchronous ready/leave decision. A full
source transfer needs to join Core producers and SDK readers and flush final goal
accounting while the source runtime is still alive. Teardown cannot precede that
asynchronous capture.

`UpgradeRequest::with_context` requires an actual typed Rust owner supplied by the
TUI. Cancellation, an absent or mismatched owner, a refused ready check, or a
missing running runtime refuses before invoking the source callback. Dispatch
starts an owned capture task synchronously before returning its completion;
cancelling a waiter cannot cancel the operation. Completion is cached, including
errors. This mechanism supplies no SDK or execution admission.

The production event hook awaits capture before shutdown. Its input context comes
from the actual running event broker, under its state mutex, after checking that
the higher Kitty reply filter holds no keys, queued events, or timer. The input
guard joins the existing native reader and retains parser state. Legacy leave
requests retain their existing conditional decision.

The controlling-PTY test now passes that actual input guard through capture,
loses a waiter, proves subsequent bytes remain in the kernel while capture owns
the guard, then releases capture and verifies the complete paste and following
key. Four Core tests cover dispatch before wait, lost waiter ownership, refused
contexts and unavailable runtime, and failed capture never reporting readiness.

This does not enable the full automatic transfer. The updater still uses its
existing request and active-goal guard. A full source coordinator must retain its
committed owners and recover from retryable failures, require a target that really
restores the complete capsule, and wire fresh native destination admission and
all-root activation before that guard can be removed. No new release or automatic
published-version transition is claimed here.

Qualification uses the source-selected Trust 15698 lane through the qualified
native `atpkg-publish`, with the committed lockfile. The complete ALab Core,
input, and TUI library runs passed 268, 47, and 352 cases respectively. Four
existing TUI cases explicitly require separate CLI/signed-package fixtures and
were not executed by this run. This is scoped qualification, not the full release
gate or proof of a published automatic handoff.

All-target Tippy for ALab input/Core/TUI and the hooked upstream TUI completed
successfully. The repository's `alab/lint.jq` classification found no owned,
hook-spanning, failing-target, or missing-build-script diagnostics; 24 upstream
diagnostics were set aside. None of these four packages declares a build script.

The focused upstream TUI run passed all 50 selected cases, including the real
controlling-PTY capture/lost-waiter/rollback case and the native App event's
new-draft/new-turn refusal and unconditional explicit-exit regression. Total
scoped coverage is 717 passing cases and four unexecuted fixture-dependent cases.

Final integration preserves main's reasoning-persistence tests (`8598668193`) and
completion-order fixture repair (`c4b4a8c4c4`). Its explicit test-support backend
remains separate from production package admission. The combined source passed
352 ALab TUI tests (four fixture-dependent cases unexecuted), and 51 focused
upstream TUI tests. The latter executes both completion-order children through
the parent; its one ignored entry is that parent's explicitly invoked isolated
helper. Both the real PTY capture case and completion-order regression passed.
The final all-target lint verdict is clean, with the same 24 upstream diagnostics
set aside. The fixture's new `expect()` was replaced with an explicit directory
path to satisfy the owned-code lint rule.

Normal `TerminalRestoreGuard` teardown still drains exit reports, calls
`tcflush(TCIFLUSH)`, and restores terminal modes. A full handoff must retain input
and terminal-mode custody through teardown and restore the original terminal
state on eventual exit. The PTY case in this change proves reader custody and
rollback; it does not exercise that normal exit guard or prove input survives
same-PID exec. This remains required before enabling full transfer.

The feature-enabled scoped fixture-guard unit test also passed separately,
covering exact-session binding, overlapping registration refusal, failed
preparation counting, and cleanup. Final distinct scoped coverage is 719 passing
cases across the unchanged Core/input runs and final TUI/fixture runs.
