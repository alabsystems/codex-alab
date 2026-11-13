<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Restore native goals before forest activation

A native goal can resume from its database as soon as a thread is loaded. That
can start a continuation or establish a fresh accounting epoch while other roots
are still being restored. The forest's input barrier alone is not sufficient.

The actual forest bootstrap now creates one native startup owner and installs its
fence in each root's extension initialization before creating that root. Child
restoration propagates the same fence before creating each private descendant.
The native goal extension binds it before publishing the goal runtime. While it
is pending, resume does not start an accounting epoch, idle continuation defers,
and ordinary goal mutations refuse. A changed enabled policy invalidates the
pending startup. This fence is not serialized and supplies no SDK permission.

`ForestBootstrapRequest::new_with_goals` constructs its input forest and goal
records from one validated data capsule. After all trees are installed and all
native admissions are ready, the actual bootstrap enumerates its loaded Core
handles and requires the exact corresponding goal runtimes. Each native runtime
checks its bound fence, original thread ID, idle state and exact persisted goal
row under its mutation semaphore before adopting the source clock. It never
replaces a database row from snapshot data or enables a stopped runtime. Absent
and stopped goals clear their fresh accounting state. An active enabled goal
requires its captured clock. The older constructor can restore only goals that
do not need an active clock.

The goal startup owner remains held through the same synchronous decision that
releases the Core forest barrier. A refused final admission, changed goal row or
changed configuration leaves goals held; activation failure stops all restored
roots. The existing owned activation task and cached outcome continue to survive
loss of a caller waiting for its reply.

The real app-server bootstrap fixture now also covers an active goal with a
native monotonic timestamp and known counters. A delayed final root admission
must leave the actual database at 93 tokens and 11 seconds and make no destination
model request. After release, the first restored turn pauses the goal through its
native tool and retains 93 tokens while accounting the carried interval. A second
case changes the expected row and requires activation to refuse with zero model
requests. The fixture seeds its native database and clock data after source Core
shutdown; it is destination qualification, not a full authenticated source exec.
The separate source SDK/quiescence cases remain part of verification.

This integrates goal restoration into the actual forest API. The production CLI
still refuses ordinary resume for complete capsules until the TUI startup path
can use that API, fresh root policies and real native SDK admission. Complete
failure custody outside the replacement process, production source request
wiring, target capability publication and the published automatic transition
remain unfinished. No new package or full release gate is claimed here.

Scoped validation: the combined tree `2c52fd042e355d80aba0cc385b05ffa85504c73e`
passed eight source goal-capture and five native app-server forest tests, with
zero failures or ignored cases. The native goal extension's eight library tests
also passed. All-target Tippy completed for input, ALab Core, ALab TUI, TUI,
Autopilot, CLI, goal extension, app-server and Core on the combined tree. The
existing classifier reported no owned, spanning, failing or build-script
findings; 27 upstream diagnostics were set aside. These are scoped unverified
Trust execution results, not a formal proof or full canonical release gate.
