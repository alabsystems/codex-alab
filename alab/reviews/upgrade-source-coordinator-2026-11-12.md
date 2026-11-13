<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Commit and capture the complete source inventory together

The source coordinator now owns the actual prepared Core forest and every goal
guard. Its final `commit` takes the TUI's paired terminal/input owner and nests
that boundary around Core, goals, the actual SDK reader catalog, and one host
policy decision. A false decision releases preparation without stopping source
work. Inventory or native admission revocation prevents commitment.

After success an owned capture joins all source Core producers and native SDK
reader acknowledgements, then flushes final goal rows and native clocks. A lost
waiter cannot cancel that operation. The cached result retains the actual
committed terminal owner alongside final forest/goal data, input bytes and mode
configuration. Failure restoration therefore uses the original owner rather
than treating serialized state as permission. These records supply no target,
SDK, headless, or continuation admission.

The existing native three-member accounting case now exercises this coordinator.
Its isolated child installs a private PTY as stdin and uses the production input
and mode owners, keeping the parent's stdout/stderr pipes. It accepts a key and
retains an unfinished bracketed paste. The inventory includes an active-goal
primary, another root, and a private ephemeral child. A refused host decision
releases all preparation and permits retry. Success commits all actual owners.
A real SQLite writer then blocks final goal accounting after Core termination;
the waiter is cancelled, the writer released, and the same capture completes.
It verifies every member, the exact 93-token active goal row, accrued native time
and clock, retained partial-input bytes and original mode data, strict goal-forest
codec rejection cases, cached-result identity, and actual terminal abort restore.

Source-selected Trust 15698 passed all eight goal-upgrade cases, including that
actual combined source/SDK/terminal case. The all-target lint classification for
input, Core, ALab TUI, TUI and autopilot has no owned, hook-spanning, failing-target
or missing-script diagnostics; 24 upstream diagnostics are set aside.

Production still uses its older request path and active-goal guard. This source
coordinator is not enabled against an incompatible destination. Full integration
still requires the restoring-target compatibility boundary, complete TUI startup
and routing/protocol facts, whole-capsule transport and fresh native destination
admission, all-root activation and failure custody. No complete release gate or
published automatic update is claimed by these source-side cases.

## Combined main qualification

Main advanced with account-history clock preferences and a record of wider TUI
fixture limits (`1d62af54ff`). Both were preserved by a clean merge. Actual
combined source `b523e6ca97` passed all eight goal-upgrade cases and all 86 focused
TUI/account/readiness cases. The two ignored isolated helpers are invoked by
their parents. Final all-target lint classification is clean, with 24 upstream
diagnostics set aside. These 94 scoped passing cases do not establish a full
gate or published all-root automatic transition.
