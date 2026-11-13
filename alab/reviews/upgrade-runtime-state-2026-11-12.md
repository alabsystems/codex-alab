<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Typed runtime state at the real CLI boundary

The final source capture now encodes one namespaced frame containing the entire
Core/goal forest plus the exact captured input, original terminal modes and TUI
startup facts. Its public encoder refuses a Core-only capture without its actual
TUI startup record. Nested goal and input JSON are carried as borrowed raw values,
so their data does not expand into arrays of byte numbers or undergo unnecessary
parse/re-encode cycles. The overall transport limit remains enforced.

The generic framing and early terminal recovery belong to `codex-alab-tui`, which
the CLI already uses. Core and native goal validation remain in Autopilot. This
avoids adding a CLI dependency on the app-server policy implementation. No new
lockfile dependency entry or upstream version pin is needed.

The actual first main hook still adopts the kernel-delivered, same-PID exec
custody descriptors before any runtime threads. Immediately after ALab
activation, the new hook selects only this frame's namespace. Other opaque state
remains available to its consumer. It installs the native original-mode/input/TUI
startup owners, retains the whole original frame, and returns a recovery owner
held on main's stack through CLI completion. Failure in running-build setup or
fresh SDK construction restores the actual original terminal. Data alone never
establishes an SDK source, package admission or continuation permission.

Goal/configuration JSON stays opaque at this early boundary. The real CLI parser
requires explicit local UUID routing for a runtime transfer and checks that it
matches the captured primary. A transfer also requires independently requested
fresh native constructor admission, whose actual entry runs before dispatch.
Clap errors on this path return through main instead of calling `process::exit`,
which would bypass recovery. Ordinary launches retain their existing clap exit
behavior.

A valid complete capsule must enter all-root restoration. The current CLI rejects
fallthrough into ordinary disk resume, because that would discard hidden or
private roots. The all-root app-server/goal consumer is still pending; this change
does not advertise complete target restore support or enable the production
active-goal update path. In particular, process-local retention does not yet
provide recovery custody outside a failed replacement process. That and complete
forest/goal activation must be implemented before advertising the capability.

The Core-only three-root source fixture still proves the public encoder rejects
missing TUI facts. A separate, explicitly synthetic view is used only to qualify
the data framing around its real captured goal, clock, forest, input and mode
records. Round trips preserve those records; changed versions, primary mismatch,
missing terminal/input fields and unknown claims refuse. This codec exercise is
not the complete real-TUI/native-SDK destination proof.

The real-CLI failure fixture uses a private controlling PTY and kernel descriptor
custody. Its source deliberately leaves a partial paste in its input checkpoint
and uses an invalid opaque goal body. Constructor denial and argument failure
must exit without trying ordinary resume and restore every original terminal
configuration field, excluding Darwin's live `PENDIN` state, plus control
characters and input/output speeds. The original lease must release after exit.
The fixture's image/lease receipt is private test data, not signed package
admission or an SDK grant. It requires an explicitly built CLI and must be invoked
rather than counted from an ignored test entry.

The real-CLI fixture passed with both failure phases and a negative control that
executes a test image without recovery and remains raw. On Darwin, exit of a
controlling session revokes the slave descriptor; the fixture observes the
retained master endpoint, which a native probe confirmed preserves the raw
settings without our cleanup. This prevents kernel cleanup from creating a false
positive. Both real CLI cases were explicitly invoked against the newly built
normal executable, including the existing opaque-state custody case; none was
ignored in that run.

Before composition, eight goal-upgrade cases, four custody cases and one real CLI
routing case passed. The four custody entries include the isolated child helper;
its actual transitions are exercised by the parent. Final all-target lint for
input, Core, ALab TUI, TUI, Autopilot and CLI has no owned, hook-spanning,
failing-target or missing-script diagnostics, with 25 upstream diagnostics set
aside. The CLI build script ran successfully. These checks do not constitute the
canonical release gate or a published automatic adoption.

## Protocol cleanup before TUI initialization

Review found that original termios recovery alone leaves terminal protocol state
behind when startup fails before TUI initialization. The CLI now attaches the
actual TUI cleanup callback to its recovery owner. A pending startup record first
adopts the native alternate-screen and keyboard trackers; the callback then uses
normal exit quarantine, keyboard/mouse/paste/focus resets, screen release and
cursor restoration. It runs only while actual original-mode ownership remains,
avoiding a second cleanup after normal TUI exit. Successful committed source
custody still preserves the terminal through exec.

Startup facts now include the existing DA1-support flag, so a replacement retains
normal exit's report barrier. The controlling-PTY startup case supplies the reply
to the destination's actual exit query after proving initialization emitted no
queries. The failure fixture activates both keyboard stacks, alternate screen,
mouse, paste/focus reporting and a hidden cursor. Both real CLI failure phases
must emit normal protocol cleanup as well as restore termios. Its no-recovery
negative control emits neither screen release nor canonical restoration.

Concurrent main's isolated diagnostics helper was loaded twice under different
module names. The combined all-target lint detected that duplication. One
ALab-marked root test module now supplies both existing isolated fixtures; their
validated-child logging and assertions retain the same behavior.

Final protocol qualification on the composed working tree passed eight native
goal cases, four custody entries, one CLI routing case and 50 focused TUI cases,
including the actual DA1 exit-reply/resize/exec test and the concurrent native
setup fixtures. The two explicit real-CLI tests also passed against the newly
built normal CLI, with protocol restoration and the no-recovery control. Final
six-package all-target lint is clean for owned code, hook spans, targets and the
CLI build script; 25 upstream diagnostics are set aside. Ignored standalone
helpers and explicitly supplied CLI fixtures are distinguished in the logs.

## Final composition

The final merge also preserves main's centralized private fixture directories and
bounded, notification-based launch observations (`db518933b0`). The composed
`6573378d5e` passed the selected Core/goal, custody, routing, fixture-helper and
50 TUI cases, including the actual startup/DA1/exec case. Its six-package
all-target lint classification is clean, including the CLI script, with 25
upstream diagnostics set aside. The normal CLI failure cases were executed
against identical product behavior before this test-only composition. No full
release gate or production adoption is claimed by this qualification.
