<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve the original terminal mode across exec

A full input transfer must leave the terminal raw through exec. A replacement's
fresh Crossterm cache would otherwise save that raw mode as its original and
restore raw mode on eventual exit. The original terminal configuration therefore
needs an owner outside Crossterm's process-local cache.

ALab's TUI raw-mode calls now use an ALab-owned mode tracker. Before the first
raw-mode entry it opens the same native terminal selected by the existing input
factory, validates its identity, and retains its original termios. Disabling raw
mode first clears Crossterm's cache, then restores that native original with
`TCSANOW`, which does not flush input. Successful disable releases the owner;
a later editor or foreground cycle captures its new original. Failed restoration
retains ownership for a retry. Non-ALab library consumers keep Crossterm's behavior.
The only upstream changes are marked imports/calls at the existing TUI, OSS
selection, startup exit, and daemon-recovery mode boundaries.

A strict data snapshot contains version, OS, terminal device/inode identities,
original and running flags, control characters, speeds, and line discipline.
Adoption opens and checks a fresh native terminal; it requires matching current
raw configuration and validates the original before installing ownership. It
changes no terminal modes and consumes no input. It refuses after any earlier
wrapper interaction, even when a terminal's raw bytes happen to match again.
Neither the snapshot nor this owner supplies SDK or executable admission.

The first native test exposed an important distinction: Darwin sets `PENDIN`
when entering canonical mode. It is live pending-input state, not saved mode
configuration. The installed SDK marks it as state, and Apple's kernel sets it
on `ICANON` entry and consumes it when processing pending input
([XNU tty.c](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c)).
Snapshots exclude that bit from configuration. Decoding refuses a stale serialized
pending bit and preserves the current terminal's pending state. The test compares
all remaining configuration fields exactly; it does not waive another flag mismatch.

The actual controlling-PTY test enables raw mode, records the original and PID,
and execs the test executable without changing PID. The source retains a ready
key and an unfinished paste in its joined input checkpoint; the parent queues
the remaining paste bytes, including carriage return, newline, Ctrl-C, and a
following key. The destination starts with an empty Crossterm cache, validates
and adopts both input and mode snapshots, then receives the original ready key,
the exact completed paste, and the following key in order. It restores the
source's original canonical configuration,
checks repeated enable/disable, and verifies that a later editor cycle restores
its updated configuration. Foreign version/OS/terminal, stale modes, invalid
control-character layout, duplicate adoption, and late adoption refuse without
changing the terminal. The parent requires a success marker and exit zero, with
a bounded watchdog and owned child cleanup.

Source-selected Trust 15698 checks passed: one real same-PID PTY exec case and
52 focused TUI cases, including the existing input reader/cancelled-waiter/rollback
PTY case now using these production mode wrappers. The one ignored TUI entry is
the completion-order parent's explicitly invoked isolated helper. All-target
Tippy for input, Core, ALab TUI and upstream TUI has no owned, hook-spanning,
failing-target or missing-script diagnostics; 24 upstream diagnostics are set
aside by the repository's classifier.

This is not a complete live-upgrade path. Normal terminal teardown still drains
reports, flushes input, and resets terminal protocols. A full transfer needs an
actual terminal admission/commit guard joined with input, Core, goal and SDK
capture, a compatible target and whole capsule, and destination restoration and
failure recovery. No bypass of normal exit or active-goal readiness is enabled by
this change. No published automatic version transition is claimed.

## Combined main qualification

Main advanced with `1ab704c65a`'s pending-update indicator correction. It was
merged without conflicts. The actual combined source `b04b07b9ff` passed the
same-PID PTY exec case, all 32 native handoff cases, and all 52 focused TUI cases.
The completion-order parent still invokes its isolated helper explicitly. The
five-package all-target lint run, now including ALab autopilot, is clean under
`alab/lint.jq`, with the same 24 upstream diagnostics set aside. These 85 scoped
passing cases do not establish a full release gate or published automatic update.

## Destination input boundary

`input::restore_after_exec` installs the parser only in a fresh input broker,
under its owner lock, after the matching native terminal mode owner accepts its
snapshot. A malformed parser never reaches terminal adoption; a rejected mode
leaves input fresh for a corrected attempt. Replay, protected-screen quarantine,
reader startup, preparation and a previous restoration permanently retire this
freshness boundary, even after subscriptions or fences are dropped. Restoring
data neither starts a reader nor establishes executable or SDK authority.

The controlling-PTY exec case exercises the combined destination call, including
foreign mode data followed by a valid retry, duplicate refusal and the exact
paste continuation. Separate broker cases cover malformed data, failed validation
and every prior input-owner operation. This entry point is not yet installed in
production startup: the full authenticated capsule must also restore startup
probe/protocol and routing state before any query or continuation. No additional
target compatibility capability is advertised by this component change.

Qualification: all 49 input and 353 ALab TUI library tests passed. Four existing
ALab TUI cases require separately supplied CLI/native-host fixtures and remain
ignored; their results are not claimed. All-target lint for input, Core, ALab
TUI, TUI and autopilot has no owned, hook-spanning, failing-target or missing-script
diagnostics, with 24 upstream diagnostics set aside. This is component
qualification, not the canonical release gate or a published automatic update.
