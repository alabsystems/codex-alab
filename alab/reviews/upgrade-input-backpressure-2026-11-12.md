<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Backpressure the joined terminal reader

The background reader could drain the terminal indefinitely while the TUI was
busy, accumulating completed events in memory and eventually exceeding the
checkpoint envelope. The actual reader now stops polling input once 4096 events
are queued. Its final read can add up to one 1024-byte batch. Bytes not yet read
remain in the kernel; no accepted key or paste event is discarded to make room.

Consuming an event from a full queue wakes the original reader through a separate
nonblocking socket. That channel cannot produce a resize or fill the SIGWINCH
pipe. While held, the input poll descriptor is negative, so terminal HUP/ERR does
not cause a busy loop. The stop channel remains live: freeze joins the reader
without requiring the application to drain first. Consecutive resize events
coalesce to their latest dimensions, while sizes separated by keys retain their
order. This is a completed-event count bound, not a bound on an unfinished paste
or the byte size of one event.

The native stream regression sends 64 KiB through the kernel, waits for the real
reader's backpressure, freezes it, checkpoints and restores its accepted events,
then starts the reader on the same input description. It fills that queue again
before consuming, exercising the consumer wake path, and requires every original
byte in order with no synthetic resize. The existing PTY/parser tests retain
coverage of partial paste, UTF-8, Escape deadlines, protected-screen quarantine,
descriptor ownership and read failures.

This removes one prerequisite defect in the full update path. It does not wire
the TUI's complete forest receiver, supply native package restoration admission,
provide outside-process failure custody or qualify a published live upgrade.

Scoped checks passed: all 50 input library tests and the actual TUI controlling
PTY startup/exec test, with zero failed or ignored cases. All-target Tippy for
input, ALab Core, ALab TUI, TUI, Autopilot, CLI, goal extension and app-server
completed. The existing classifier found no owned, spanning, failing or
build-script findings and set aside 25 upstream diagnostics. These are
unverified Trust execution results; no formal proof or complete release gate is
claimed.
