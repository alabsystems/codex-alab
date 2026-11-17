<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Protect renewal's plain and TLS listener startup

Decision: reuse the existing shared startup owner for both renewal listeners.
Give that owner explicit binding and preparation hooks so it receives the bound
server before TLS setup. Keep renewal's handlers, TLS handshake attribution,
certificate commands and application/controller logic unchanged.

At `cc1dbc8930`, inert public-`serve` controls reproduced unclosed listeners after
TLS context/wrap failures and thread construction/start failures. A real worker
could also start before `Thread.start` raised, leaving an active loop outside
the caller's ownership. Six baseline methods exposed 26 failing subtests, while
bind-refusal and successful plain/TLS handoff controls passed. No real socket,
TLS context, OpenSSL command, application or PTY was used.

`start_listener(handler, bind=..., prepare=...)` assigns the constructor's returned
server directly to its guarded ownership slot. Only then does it run the
preparer, before creating any worker. The plain renewal branch uses the default
binding. The TLS branch passes its existing Listener subclass and a preparer
that attaches the wrapped socket to that owned server. A preparation failure
closes the current server socket, with no shutdown or join of an unstarted loop.
Successful TLS wrapping transfers the raw socket to its wrapper; cleanup does
not separately close the detached raw object.

The same gate now handles partial native worker start and cancellation before
or after commitment/release in renewal. Startup exceptions retain their exact
identity. Serving/shutdown retirement keeps the existing single cleanup bound
and diagnostics. In particular, refusal to start the shutdown helper after
commitment can leave a live serving worker: closure plus explicit unconfirmed
retirement is a failed startup, never a successful handoff or retirement claim.
No synchronous potentially unbounded shutdown fallback is added.

Seven new methods cover bind/context/wrap/thread refusals, a preparer that
transfers a wrapper and then raises, positive plain/TLS binding and daemon
handoff, unchanged handshake-refusal attribution, real-start and committed
cancellation, and shutdown-helper refusal. The fake bound server and TLS objects
model current-socket ownership. Real workers are owned before start; finite
fixture release/join is separate from assertions about factory retirement.
Gate fault injection targets the shared factory's own Event calls, leaving
Thread's internal startup synchronization intact.

All 42 selected methods pass without skips under managed Python 3.14 and system
Python 3.9 with `-I -B`: the seven new methods, eleven shared startup controls,
five capacity controls, six renewal acquisition controls and thirteen renewal
tail controls. `git diff --check` passes. The new controls start no child
processes, bind no sockets and execute no OpenSSL, app, PTY, compiler or provider.
The runtime input list already binds capacity support; the renewal matrix binds
both capacity support and renewal. These hooks add no runtime input edge.

The owner can close a constructor's returned server and any socket attached by
preparation. It cannot recover resources a failing constructor or preparer
conceals without returning or attaching them. Successful handoff leaves ongoing
cleanup with the caller. These Python fixture controls make no native renewal,
TLS interoperability, application qualification or certified-build claim.

Independent review passed the same 42 methods on both Python versions and found
no ownership or integration blocker. Root reviewed the production change and
the stated limits and approved these four topic paths for composition.
