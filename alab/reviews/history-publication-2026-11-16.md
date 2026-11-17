<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Restore guardian handlers and include final cancellation — 2026-11-16

Decision: installed signal handlers belong to the guardian's entire invocation,
including startup and publication failures. Restore every installed handler
before leaving, preserve an active original failure, and include all witnessed
cancellation through the final publication check in the saved verdict and exit.
The actual owning-reap and group observations remain separate facts.

At source `0491766d84`, a real raw-file collision raised before the guardian's
inner `try/finally`. It left SIGINT, SIGTERM and SIGHUP handlers installed, and
would leave a nondefault SIGCHLD handler overwritten too. A separate no-fork
control delivered actual self-SIGTERM at master close, after the final drain.
The guardian returned zero and saved `forced: false` with
`cancellation_signal: 15`. All child and group operations in these controls were
inert fake boundaries; no external process or old orphan was signaled.

An enclosing scope now protects handler installation, raw open, the retained
custody sequence and publication. It captures each previous handler with
`getsignal` before mutating it, including an installation interrupted after its
native return, and attempts all restorations. Restoration diagnostics cannot
replace an active exception: both class-name and message formatting are guarded,
the kind must be an exact string with a literal fallback, and exception notes are
best-effort. The same label helper prevents a body exception's failing formatter
from escaping as `SystemExit(0)`. Without an active exception, a restoration
failure forces a failed saved verdict while preserving child exit and reap/group
facts.

Cancellation checks cover descriptor close, result writing, completion emission
and restoration. A late observed signal forces a nonzero guardian result and
corrects the saved custody file. There is one completion packet; a signal during
or after its emission can leave that earlier packet healthy, while the final
file and exit record failure. The consumer already requires both the actual
guardian return code to be zero and the final custody file to show an unforced
owning retirement, so the earlier packet cannot qualify the run.

If corrective publication fails, that failure cannot itself request a successful
CLI exit and combine with an older healthy file. With no active primary, the
guardian raises a definite `RuntimeError` with the exact publication failure as
its cause. Ordinary and genuinely nonzero active failures retain their identity.
There is one narrow exception: an active success-shaped `SystemExit` must become
a definite failure if its corrective record cannot be saved; that exact original
object remains the wrapper's cause. Classification matches `alab/cli_status.py`:
no code, zero, and bounded native POSIX integer wrapping are success-shaped;
genuinely nonzero and out-of-native-range exits retain their primary identity.

Ten safe public-guardian regression methods exercise normal restoration, raw
collision without a fork, partial and interrupted-after-native installation,
hostile class names/messages, refused exception notes, four cancellation
publication boundaries, and original completion failures combined with restore
and final-save failures. The consumer-conjunction controls include `SystemExit`
with no code, zero and POSIX-wrapped integers, both as secondary and active
primary failures. All ten pass on managed Python 3.14; the old source produces
52 failing assertions and three errors. These tests use real PTY descriptors
and self-signals with fully mocked child/group operations; they create no child
process. No broad or fork fixture suite was rerun. `git diff --check` passes.

The standalone early `--guardian` branch still uses only standard-library code.
The existing waitid/WNOWAIT capability and finite fixture bounds are unchanged.
No post-reap signal, external PID interpretation, permission workaround, Rust
build, native application run or product qualification is introduced.
