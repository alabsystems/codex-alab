<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep incomplete reset smoke CLIs nonzero — 2026-11-16

Decision: apply the existing `cli_status.py` policy to the weekly-reset and
quota-wait command-line entries, as request recovery already does. Their main
APIs retain the exact cancellation exception and failed partial report. Their
CLIs classify successful-shaped observation exits as incomplete failures,
including POSIX-wrapped integer exits; genuine nonzero and keyboard
interruptions keep their behavior.

At source `0491766d84`, bounded inert subprocess controls execute each source
file's original entry AST with only its case function replaced. Both entries
exit zero after `SystemExit(0)` despite retaining `passed: false` and
`interrupted: true`. The binary is owned inert data and never executed. Nine
new test methods expose 26 failing subcases before the repair. The same saved
before/after witnesses now show exit one for both failed observations.

Argument parsing occurs before the observation catch, preserving help and
argument errors. Weekly's isolated exec-wrapper still executes before adjacent
imports; quota's wrapper retains its startup route, EOF stdin and actual child
exit. The tests exercise wrapper exits zero and seven with inert Python only.
Diagnostic and output failures cannot restore CLI success, while nonzero or
keyboard interruptions during diagnostics retain their new exception identity.

The policy is loaded from the existing adjacent ALab helper, without inheriting
module-search configuration. Weekly's helper dependency is included in the
maintained runtime input snapshot; a helper changed during observation refuses
that run. The existing release hook already binds `cli_status.py`. An isolated
history-loader import confirms its weekly dependency resolves without running
an observation or application.

Managed Python 3.14 with `-I -B` passes 82 affected methods: nine new CLI/API,
16 quota-wait, five runtime-dispatch and 52 weekly-reset fixture methods. The
nine new methods also pass on macOS Python 3.9 with `-B`. `git diff --check`
passes. No provider account, native Codex binary, Rust build, application
qualification or package publication is claimed.
