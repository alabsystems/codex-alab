<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Cancel the original child before its PTY group exists — 2026-11-16

Decision: a missing process group during forced birth cleanup must not leave
the actual unreaped child running. Signal only the original child returned by
the custodian's own fork, retain its identity through `WNOWAIT`, sweep any group
it formed, and then perform the owning reap. A permission refusal remains a
refusal; it does not authorize a direct-PID fallback. No PID from a result file,
process scan or external observation grants signaling authority.

The newly included history fixtures exposed a real birth-return race at source
`efd8a16a2d`. The forkpty parent can return before the child establishes its
session and group. Cancellation closed the master, then `killpg` returned
`ProcessLookupError`; the following owning wait blocked while the child later
entered `signal.pause()`. The fixture's own repeated timeout cleanup could
also abandon its guardian.

Two bounded controls use a real `os.fork()` child and `openpty()` endpoint,
holding the child before `setsid()` to reproduce that fork/PTY return boundary
deterministically. One releases the child after the first group lookup; the
other waits for its actual late session before letting the failed lookup
return. Every control child arms a three-second safety alarm before it can
park. Both old-source controls ended only through that alarm, with exit
`-SIGALRM`; both still performed their owning reaps, so the negative controls
left no live child behind. These are actual fork/PTY controls, distinct from
the existing real forkpty-return test.

Forced cleanup now falls back to SIGKILL of the original unreaped child only
after a missing-group answer. A blocking `waitid(WEXITED | WNOWAIT)` observes
that child's exit while keeping its PID reserved. The second group sweep
therefore handles a session established between the first lookup and direct
cancellation, before `waitpid` releases custody. Local guards require the
original positive child PID and reject signaling after owning reap. The
fixtures additionally reject external child/group targets and post-reap
signals, and assert direct cancellation plus the retained-identity sweep.

The fixture actor cleanup closes its lifeline, waits within a bound, and can
kill/reap only its own retained `Popen` actor if the detector failed. Its pipe
handles close in all paths. The real forkpty and other PTY fixture children
also arm finite alarms. An already established controlling PTY may end its
child with SIGHUP when closed; the birth test accepts that real cancellation,
while both deterministic no-controlling-session controls require SIGKILL.

The three new bounded controls passed before rerunning the maintained history
module. All 12 history methods pass on managed Python 3.14. That interpreter
provides `os.waitid`; macOS system Python 3.9 does not. The existing custodian
already requires `WNOWAIT` in its ordinary path, and the maintained full-workflow
instructions now name that capability alongside Python 3.11+. No reap fallback,
runtime skip or qualification waiver was added. `git diff --check` passes.
Previously orphaned external work remains deferred under the native ownership
boundary; no external PID was signaled. No Rust, native application, build,
package, installation or new product qualification is claimed.
