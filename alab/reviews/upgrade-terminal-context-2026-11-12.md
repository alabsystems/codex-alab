<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Supply paired native terminal ownership to the capture request

The actual TUI capture boundary previously supplied only its joined input fence.
It now supplies one non-serializable `upgrade_terminal::Prepared` owner containing
that exact fence and a revocable native mode guard. Construction compares the
retained input file's native device/inode identity with the mode owner's terminal;
a checkpoint or a pathname cannot establish this pairing.

The pair holds mode and input admission through one final caller decision.
Refusal retains rollback, while success records and checks both committed owners.
A repeated commit never invokes the callback. Snapshot accessors expose separate
input and mode data, without supplying SDK or executable admission. Fatal abort
restoration requires this pair's actual committed state and does not reopen the
old runtime. The obsolete TUI method returning only the input fence was removed.

The real event-broker factory checks the higher reply filter and joins its input
reader before pairing modes. `accept_upgrade_and_capture` now passes this actual
paired Rust owner to `UpgradeRequest::decide_with_context`; decoded data cannot
construct that request context.

The controlling-PTY case now runs the production paired factory. It cancels a
request and refuses a wrong context type under a live runtime, asserting neither
source callback runs and the next preparation reacquires both owners. It then
checks refusal, successful combined commit, repeated-commit refusal, actual exit
guard preservation of queued CR/LF/Ctrl-C bytes, and abort cleanup. The same-PID
mode case also attempts pairing with an actual foreign socket input descriptor;
construction refuses, releases both preparations, and allows normal retry.

Source-selected Trust 15698 checks passed the real same-PID component case and
all 81 focused TUI/account cases. The one ignored completion-order helper is
explicitly invoked by its parent. All-target Tippy for input, Core, ALab TUI and
TUI has no owned, hook-spanning, failing-target or missing-script diagnostics;
24 upstream diagnostics are set aside by the repository classifier.

The production updater still uses its existing legacy request and active-goal
readiness guard. The paired TUI context is ready for the full source coordinator,
which must require a compatible restoring target and join Core, all goals, SDK
and host admission before capture and teardown. Whole-capsule destination restore,
all-root activation, failure custody and an actual published automatic transition
remain required. These scoped cases do not prove that end state or a full gate.

## Combined main qualification

Main advanced with Cargo-copy provenance attribution, selected-Python read-only
runtime inputs, native fixture repairs, UI-readiness ordering cases, and failed
account-refresh cache retention (`a17b1b8b68`). Those changes were preserved by a
clean merge. Actual combined source `e505f1078b` passed 156 selected native
attribution/inventory/passive cases, the real same-PID terminal component case,
and 86 focused TUI/account/readiness cases. Both standalone ignored TUI helpers
are explicitly invoked by their respective parents. The five-package all-target
lint verdict is clean, with 24 upstream diagnostics set aside; none of these
packages declares a build script.

The merged containment suite passed nine cases. Two portable-Python cases were
explicitly skipped because this run selects the system Command Line Tools Python,
rather than a runtime below the protected account home. Native package discovery
has no registered `python`/`python3` tool on this machine. No alternative runtime
or permission boundary was substituted. Total executed scoped coverage is 252
passing cases; it is not a complete release gate or published upgrade proof.
