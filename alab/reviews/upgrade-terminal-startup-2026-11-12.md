<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Restore terminal startup before queries

Input and original termios ownership alone do not make replacement startup safe.
Normal TUI initialization sends cursor/color/device queries and pushes keyboard
modes. Repeating it after exec can place query replies inside a pending paste and
leave an extra keyboard-stack frame. The terminal destructor also resets the
cursor independently of the main exit restoration path.

The actual TUI capture factory now attaches its current startup facts to the same
prepared terminal/input owner used by source commitment. These facts include the
cursor, viewport and saved inline viewport, visible inline history count, cached
colors, keyboard support and release reporting, SSH terminal policy, tmux image
base, focus and screen ownership, mouse policy, and the actual alternate-screen
protocol/cleanup state. Pointer cleanup includes the uncertain state left by a
failed write. Reading the palette cache never starts a query.

A fresh destination installs this strict versioned record while it restores the
matching native input and original-mode owners. Malformed startup or input data,
a rejected native mode, duplicate installation, and installation after actual
startup refuse. A private initialization owner carries the selected record from
the initialization boundary to the actual new Tui; callers cannot manufacture
that owner from data. Snapshot bytes establish no SDK or executable authority.
The transport must independently establish custody.

The production `init` hook consumes an installed record before normal setup can
emit output or read input. It establishes the new Crossterm cache through the
existing original-mode wrapper, adopts the actual alternate-screen cleanup
tracker, uses the recorded cursor and colors, obtains the current native terminal
size, and constructs the new terminal without queries or keyboard pushes. The
new Tui adopts the screen/focus/mouse/saved-viewport facts before its event stream
starts. Fullscreen geometry follows the current native dimensions; inline
geometry and cursor coordinates are bounded by them. A committed source terminal
owner also suppresses the terminal destructor's independent cursor reset. Panic
and ordinary exit still use normal cleanup.

The private controlling-PTY case initializes the real source TUI, enters its
owned alternate screen, accepts a key followed by a partial paste, and prepares
through the actual TUI capture factory. It commits both native owners, runs the
real exit restoration path, drops the source Tui, and execs the test executable
with the same PID. The parent supplies the remaining paste bytes and resizes the
PTY. The destination restores before actual `init` and `Tui::new`, checks the
adopted facts and native dimensions, reapplies owned-screen configuration, and
receives the exact completed paste followed by its next key. Output delimiters
prove source drop and replacement initialization emit no terminal commands.
Final normal exit restores canonical mode and clears the alternate-screen owner.
The parent requires the success marker and exit zero and reaps failed children.
Foreign startup version, malformed parser, missing explicit optional facts,
duplicate installation and late source installation are also exercised.

This connects terminal restoration to actual startup, but the complete runtime
receiver still needs authenticated whole-capsule transport, launch/routing
validation, fresh native SDK and all-root/goal activation, and early fatal-startup
recovery custody. Production automatic requests still use their older path and
active-goal hold. No full-target compatibility capability, new publication,
canonical release-gate pass or published automatic adoption is claimed here.

Qualification before composition: all 87 focused TUI cases passed, including the
actual resize/exec case above. Two isolated helpers remain ignored as standalone
entries and are invoked by their parent tests. The final all-target lint run for
input, Core, ALab TUI, TUI and autopilot has no owned, hook-spanning, failing-target
or missing-script diagnostics; 24 upstream diagnostics are set aside. The only
change after the executed resize case removes a redundant clone in that test;
final all-target lint compiles the corrected test too. These are scoped checks,
not a complete release gate.

## Combined main qualification

Concurrent main `c4f4f3166f` repaired native TUI fixtures. The merge preserves its
clock, path and exact-snapshot assertions. On combined `03ab965300`, all eight
native goal-upgrade cases, four ALab TUI fixture cases and 168 selected TUI cases
passed (180 total); the resize/exec proof is included. Two isolated helpers were
invoked by their parents. Final all-target lint on `20dfd0f109` is clean for owned
code and hooks, with 24 upstream diagnostics set aside. Its only difference from
the executed combined tree documents the intended panic contract on a test-only
fixed-timestamp helper; it changes no runtime behavior. The concurrent review's
wider-suite failures remain recorded and are not waived by these scoped checks.
