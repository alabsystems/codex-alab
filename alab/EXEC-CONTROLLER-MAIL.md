<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Unread controller mail across account replacement

Unloaded child sessions retain queue-only messages in the shared agent
controller. Ordinary tree shutdown drops these queues. Native account
replacement must carry messages that were already accepted into the new
controller, even when no child runtime is loaded to own them.

The root input snapshot is now version 3. It flattens root input, root mailbox
messages, and controller mailbox messages into one private input codec, with
separate routing records. This preserves shared publication/consumption
identity even when multiple queues refer to the same delivery. Each controller
record retains its recipient and its receive-event ID.

Root suspension seals the controller queue under its acceptance mutex. New
sends are then refused. An eviction already in progress may still transfer
previously accepted messages after sealing; its transfer joins the same retained
storage. The snapshot reads that storage after the existing whole-tree shutdown
barrier has completed. Ordinary shutdown has no such retained storage.

Bootstrap decodes the snapshot without delivering its messages. Original-turn
admission installs the old controller messages ahead of newer messages for each
recipient. Installation emits no receive or consumption event. The ordinary
controller take/consume path remains responsible for those events.

Focused checks cover per-recipient order and receive IDs, sends racing with
sealing, late eviction transfers, shared publication identity across root and
controller queues, malformed queue boundaries, and installation requiring live
original-turn admission. All seven focused tests passed. The full `alab/check.sh` gate passed on
`23d8563e0b2d99491501b38eecf48bc8ca9f777b`: all 25 ALab groups passed
(1,542 tests, four ignored), owned lint passed (118 upstream diagnostics set
aside), and all required hooked library suites passed. These included Core
(2,966 passed, one ignored), exec (70), login (237), and TUI (6,185, five ignored).
The retained log is `exec-controller-mail-full-gate.log` in `.aterm/.ubd`.
These checks qualify the queue codec and admission path. The combined private
package also passed the real-process checks recorded in
[the start-barrier qualification](EXEC-TREE-STARTS.md#qualification-2026-11-04).

This is one part of preserving the agent tree. It does not resume active child
turns, preserve live tool processes, or restore the remainder of per-child
session state. The active-child regression in
[EXEC-CHILD-CONTINUITY.md](EXEC-CHILD-CONTINUITY.md) remains required work.
