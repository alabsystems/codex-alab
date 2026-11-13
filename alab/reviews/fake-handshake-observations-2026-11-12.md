<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Observe fake-host handshake progress without changing admission

## Decision

Retain bounded metadata about the fake host's version handshake in the native
upgrade fixtures. A full native run on `01e48105fa` recorded a real client timeout
in `/tmp/ac.2ITTQH/target-refused`: the child reported `aterm did not answer in time`,
while the host's request list contained only `version`. That request list is
updated before the reply write, so it does not establish whether the host wrote
the reply or the client consumed it.

`FakeAterm::handshake_events()` now distinguishes accepted connections,
authentication, receipt of `version`, deliberate reply drops, and the start and
outcome of the version reply write. Each event has a diagnostic connection ID and
monotonic microseconds relative to that fake host's creation. The accept marker
is recorded before spawning its existing serving task; the diagnostic ID does
not alter the fake's protocol connection IDs. Both native upgrade fixture
parents save `private-handshake-events.json` before checking the child's exit
status or decoding its outcome, including ordinary child failures and the
existing parent timeout path. A scoped witness also attempts to retain the JSON
if setup or output collection exits early; a filesystem error during unwinding
does not replace the original failure.

## Limits

The trace retains its first 256 events and reports how many later events were
dropped. It contains no authentication values, request bodies, reply contents,
or error display text. Write records include the intended reply length and only
the error kind and OS error number. A successful write means the socket accepted
the bytes; it does not prove that the client read them. A failed write can follow
a partial write. Each snapshot includes its capture time on the same monotonic
clock, and may end with a connection still pending. Absence of a later stage is
evidence only as of capture, not proof that the stage could never run afterward.

Timestamps are local monotonic observations, not a cross-process clock. Interpret
stages for the same diagnostic connection; concurrent connections can interleave.
Recording adds bounded in-memory bookkeeping but no awaits, retries, gate changes,
or client deadline changes. This is test-host evidence, not a repair for or proof
of the cause of the intermittent native handshake timeout.

## Validation

Focused tests exercise a real gated version request before its reply, successful
reply completion, an intentionally dropped reply, and a write to a closed peer.
They also check the retention bound, capture time, and that authentication, reply,
and error display text are absent from serialized metadata.

- `targo --unverified test --locked -j 2 -p codex-alab-aterm --lib fake::handshake
  -- --test-threads=2`: 4 passed, 0 failed.
- The complete `codex-alab-aterm --lib` suite with the same arguments and no test
  filter: 164 passed, 0 failed, 2 ignored, in 4.69 seconds.
- `targo tippy --locked -j 2 -p codex-alab-aterm --lib --tests -- -D warnings`
  passed without warnings. Its log is `codex-handshake-events-tippy-20261112.log`
  beside the test logs.
- Both runs used the worktree's separate `target/handshake.noindex` cache and the
  PATH Trust toolchain: `trustc 0.12.0+g29ebb403d`, `targo 0.11.0+g20565c325`.
  These were explicitly unverified test runs, with no formal-proof claim.
- Logs: `$HOME/.aterm/.ubd/codex-handshake-events-unit-20261112.log` and
  `codex-handshake-events-aterm-20261112.log` in the same directory.

Only the added ALab code was formatted; `git diff --check` passed. Independent
read-only review found no remaining privacy, ownership, or protocol changes.

The composing session built the actual native TUI with these observations at
`c60e0f91de156262bc06aa664d5a56ae1aed789e`. Its three-thread `native_` selection
passed 24 ALab TUI tests (one ignored) and 71 native TUI tests (two ignored),
including both fixture parents. The log is
`$HOME/.aterm/.ubd/codex-pty-handshake-composed-native-20261112.log`.

All five retained cases produced readable bounded snapshots. Each of the three
active cases recorded 20 events, including four completed version writes; both
inactive controls recorded no events. None dropped events, and every event
preceded its snapshot capture time. The metadata inspection is retained as
`$HOME/.aterm/.ubd/codex-handshake-native-evidence-20261112.json`.
The full native TUI run before this diagnostic change retained one handshake
timeout; these scoped passing checks do not erase that failure or establish a
cause for it.

The extra composed startup replay was deferred because another session held a
timed machine reservation. No build or test failure is inferred from that
admission wait, and the reserved measurement was not bypassed. The 95 composed
native checks above ran to completion before the reservation began.

The deferred startup replay later passed. The
[follow-up record](native-handshake-followup-2026-11-12.md) retains a complete
native run with these observations, the measured pre-authentication intervals,
and the limits on interpreting them. The handshake timeout did not recur in
that run; a separate MCP registration timeout remains recorded.
