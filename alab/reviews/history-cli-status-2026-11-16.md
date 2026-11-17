<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep failed history observations out of successful CLI exits — 2026-11-16

Decision: apply the existing shared CLI status policy to history pagination's
normal entry. A successful-shaped `SystemExit` after an observation starts is
an incomplete failure. The public main API retains its exact cancellation;
ordinary, genuinely nonzero and keyboard failures remain authoritative even
when emitting their failed JSON packet fails.

At source `49d17b516e`, a bounded subprocess executes the original entry AST
and actual main with inert listener, RPC and process seams. A controlled seed
operation raises `SystemExit(0)`; real main completes shutdown/join/close and
saves `passed: false`, then the entry prints failed JSON and exits zero. Its
binary is inert owned data, and attempted application processes or forks fail
the fixture. The initial eight methods expose 17 failing subcases. Retained
before/after witnesses show the same failed observation now exiting one.

Parsing finishes before the observation catch so help and argument errors keep
their genuine status without failed-observation packets. The CLI reuses
`weekly.CLI`, already loaded from `alab/cli_status.py`; it adds no import or
source dependency. The maintained runtime snapshot and release hook already
bind that helper. A new history-stage tamper control confirms the final runtime
snapshot refuses a helper changed during history observation.

All bytes preceding the original main function, including guardian custody,
signal restoration, publication, waitid/WNOWAIT and the early isolated branch,
remain unchanged. A separate copied-source startup control runs with `-I -B -S`
and no adjacent modules or policy helper. A raw-evidence collision refuses
before its explicitly forbidden fork, preserving prior bytes.

Managed Python 3.14 with `-I -B` passes nine new CLI/API methods and five runtime
dispatch methods (14 total). Controls cover API exception identity, successful
and wrapped exits, genuine failure status, failed diagnostics, hostile message
formatting, unchanged successful observations, helper custody and startup.
`git diff --check` passes. These are finite inert Python subprocesses and fake
listener/RPC boundaries: no native application, actual forked PTY child, provider
account, Rust build or qualification is claimed.

Independent review approved the CLI policy, source-input closure and unchanged
guardian startup. Root separately repeated all nine new methods successfully
before accepting this topic for composition.
