<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Protect bound listeners through worker startup

Decision: share listener startup ownership in the existing capacity support.
Weekly previously left a bound listener unclosed if thread construction or
start failed. Capacity closed that listener, but a real worker could already
have started when `Thread.start` raised. Closing alone did not retire it.

`start_listener` owns the returned listener inside its acquisition guard and
holds its daemon worker behind a decision gate. A start failure releases an
abort, closes the listener and attempts bounded worker retirement without
calling shutdown on a server whose loop was never committed. Successful startup
returns the same server/daemon-worker pair; both public handlers and their
existing server loop remain unchanged. Cleanup after a successful handoff
remains the caller's responsibility.

Cancellation after commitment, including after gate release but before return,
dispatches shutdown through a separately owned daemon helper. Both handles are
owned before their respective starts, including a start that raises after
native thread creation. Shutdown and serving-worker retirement share one
five-second cleanup deadline. Shutdown exceptions are captured and attached to
the original startup error rather than lost in a thread exception hook.
Independent release, shutdown dispatch, retirement observations and close still
run if cleanup clock setup fails; remaining budget then becomes zero. No
unbounded shutdown call runs in the factory's caller.

Cleanup preserves the exact startup exception even when close, shutdown, clock
or optional diagnostics fail. Failure to observe either worker's retirement
adds explicit unconfirmed-retirement evidence; listener closure alone is never
a retirement claim. This bound does not promise retirement when shutdown fails,
the budget expires or startup cannot be observed. The factory cannot take
ownership of an object a failing constructor never returns. An immediate
post-bind line-trace control verifies the actual returned listener is already
inside the guard before any thread construction.

Ten new methods exercise both public serve functions using fake listeners,
finite inert server methods and actual owned threads where start publication
matters. They cover constructor/start/bind refusals, original cancellation,
close failure, positive daemon handoff, partial native start, cancellation
before/after release, shutdown-helper partial start, failed shutdown before/after
its stop side effect, deadline exhaustion and failed cleanup clocks. Each real
thread fixture owns its handles before start and has finite fallback retirement;
assertions distinguish factory retirement from that fallback. The existing five
capacity tests retain handler timing and exercise the gated handoff rather than
requiring the previous exact Thread target.

All 15 focused methods pass with `-I -B` on managed Python 3.14 and system Python
3.9. The fixtures bind no sockets, launch no child processes, and invoke no
application, native host, compiler or provider account. Existing runtime and
renewal-matrix source closures already include `capacity_exec.py`; the shared
factory adds no runtime input. These controls establish Python startup ownership
behavior, not native application qualification or a certified build.

Private composition `efe3a3d5f1a6c1587b4b22efd904fc05b2d56489` preserves the
concurrent CLI publication changes and passes all 343 selected Python methods
in 27.157 seconds, with no failures, errors or skips. The exact source remains
clean and unchanged. This includes all 158 maintained smoke methods and the
history, listener, CLI, runtime, cleanup and recovery controls. Discovery alone
finds 1,891 unique IDs, zero discovery errors and all ten new listener identities;
it executes no test body or native prerequisite.

Local native CPU admission still reports an unreadable slot-file identity.
The later signed Codex update succeeds but reports build `2026111601` already
current at index 155; the earlier store-lock contention has cleared. No newer
unit was installed, and no agent restart was needed. Core integration, the
application gate and a newly certified package remain unqualified. This
selected Python composition supplies none of those missing results.

Hostile review also exercised an ordinary refusal before the shutdown helper's
native start. After committed serving begins, either a thread-start refusal or
a second cancellation can leave no shutdown dispatched. Listener closure and
the exact original cancellation remain, with explicit unconfirmed-retirement
evidence; the serving worker can still be alive. Retain this stated limit:
calling potentially blocking shutdown in the caller would break the cleanup
bound. This is a failed startup, never a successful handoff or retirement claim.

One additional bounded regression covers both refusals through the public
capacity handler. It distinguishes the live serving worker and never-started
shutdown handle from the fixture's later release/join, which retires every
owned worker. It changes no production behavior or runtime input.
All 16 focused methods pass without skips under managed Python 3.14 and system
Python 3.9 (`-I -B`). No application, socket, PTY, native build or qualification
was run.
