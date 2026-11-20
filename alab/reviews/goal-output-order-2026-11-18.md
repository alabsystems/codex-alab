# Goal storage recovery evidence, 2026-11-18

The goal storage contract has seven scenarios. The component tests below have named
runtime producers. The historical pool and accounting executions are recorded in the
[November 13 review](goal-storage-recovery-2026-11-13.md). This review adds a public
app-server and SQLite check for the seventh scenario.

| Catalog scenario | Runtime evidence | Coverage boundary |
| --- | --- | --- |
| 1. SQLite FULL and pool reuse | The production pool tests force SQLite’s page limit, borrow again and commit. A healthy dropped transaction keeps the original connection. [Pool tests](../../codex-rs/alab-core/glue/sqlite_transaction_recovery.rs) | The page-limit fault exercises SQLite’s implicit rollback without exhausting the host filesystem. |
| 2. Cancellation, failed stop and lost acknowledgement | A blocked SQLite writer outlives its cancelled observer. A zero-usage stop holds admission until a fresh persisted action. The shared accounting guard exercises ambiguous commits and rechecks a queued writer. [Native tests](../../codex-rs/alab-autopilot/src/goal_persistence_tests.rs) · [Guard tests](../../codex-rs/alab-core/src/goal_persistence.rs) · [Typed error tests](../../codex-rs/alab-autopilot/glue/goal_read_recovery.rs) | The three native accounting entry points still need a committed-then-lost-ack fault injected at each producer. |
| 3. Retained turns, descendants and time | Native tests forward provider tokens through a failed turn and subsequent manual input, including terminal edits, Plan time and raw replacement. The shared boundary test preserves its original descendant usage. [Native accounting tests](../../codex-rs/alab-autopilot/src/goal_retained_usage_tests.rs) · [Shared boundary](../../codex-rs/alab-autopilot/glue/goal_accounting_persistence.rs) | The fixture forwards the real contributors with its opt-in goal setting. |
| 4. Same-ID reactivation and recovery | A delayed stop behind a reactivation loses its mutation epoch. Another test requires a fresh persisted action before a failed external stop releases admission. [Mutation tests](../../codex-rs/alab-autopilot/src/goal_persistence_tests.rs) | The earlier review records these focused native executions separately from release qualification. |
| 5. Writing, held and ready status | Native outgoing controls deliver an inline snapshot while the goal read is gated. The widget controls check saved Active display, input and writer activity on status changes. [Transport](../../codex-rs/alab-autopilot/glue/goal_status_transport.rs) · [Widget](../../codex-rs/alab-tui/glue/goal_runtime_status_widget_tests.rs) | The reviewed components cover different parts of the path. This run did not carry the same gated read through listener reattachment into a live TUI. |
| 6. Stale delivery, overflow and connection changes | The transport exercises dropped selections, overflow, reset and rearm. The TUI checks duplicate and stale packets, thread and backend switches, and lost transport. [Transport](../../codex-rs/alab-autopilot/glue/goal_status_transport.rs) · [Widget](../../codex-rs/alab-tui/glue/goal_runtime_status_widget_tests.rs) | These are transport and widget controls. |
| 7. Committed output order | Public RPC checks observe paired responses and notifications on loaded and unloaded threads, durable clears and delivery to a subscriber after the requesting connection closes. Native queue tests observe per-thread FIFO, independent progress and recovery from both per-thread and global reservation exhaustion. [Wire order](../smoke/goal_output_order.py) · [Disconnect](../smoke/goal_observer_disconnect.py) · [Queue tests](../../codex-rs/alab-autopilot/glue/unloaded_goal_rollout.rs) | The 129-conversation test below also exercises the global limit through public RPCs during a blocked write. The listener-retirement boundary around enqueue and saturation by already-committed deliveries still require direct observation. |

## Public goal RPC control

Run the independent order check with a CLI and a fresh output directory.

```sh
python3.14 -B alab/smoke/goal_output_order.py --codex /path/to/bin/codex --output /path/to/new-output
```

The independent check queues two sets on each thread, reads their replies and notifications,
then clears the saved goals. It uses two loopback model turns to materialize the threads.
It requests immediate idle unloading and waits for both the native closure notification and
the loaded-thread list. A paused goal starts no additional model requests. The JSON receipt
records the binary hash, thread IDs, issued mutation IDs, SQLite results and observed stream.
The raw stdio capture includes shutdown through EOF.

Add `--overlap-threads` to issue requests for both sessions before reading their output.
The source-selected `21e660cf42` CLI passed independent ordering. In three parallel runs
of the cross-thread variant, each with a separate database, two passed and one returned
JSON-RPC error `-32603` on its first loaded goal set. The reported FULL checkpoint had a
busy result and frame counts `-1/-1`. Both first objective rows remained visible on a
fresh SQLite read. The signed 0.11 CLI passed one cross-thread run. These observations
precede the checkpoint change.

The production connection opens a local goals database in WAL mode with synchronous NORMAL.
A visible row confirms logical commit. The durability fence requires an acknowledged complete
checkpoint before clearing the chronology marker. SQLite returns BUSY immediately when another
checkpoint owns the checkpoint lock. The failed cross-thread RPC is consistent with that case.
Its receipt does not identify the owner. The ALab retry handles only the unavailable-frame-count
BUSY result, using the existing connection busy timeout. The mutation is issued once.
[Goal barrier](../../codex-rs/alab-core/glue/goal_store_durability.rs) ·
[SQLite checkpoint semantics](https://sqlite.org/c3ref/wal_checkpoint_v2.html) ·
[SQLite synchronous modes](https://sqlite.org/pragma.html#pragma_synchronous)

The source-selected Trust 15698 test build at `f815886684` passed the exact `codex-state`
contention test. Three physical SQLite connections observed the competing checkpoint, elapsed
busy timeout, successful release and the original usage row. A separate SQLite 3.53.1 control
observed `(0,-1,-1)` on a DELETE-mode main database while an attached WAL database returned
`(0,2,2)`. The production barrier explicitly addresses main and requires nonnegative counts.

The frozen CLI test helper from `f815886684` had SHA-256
`b4744122daa5af40248b7377c38a18f2459d5e31ed15da8596d3f40f4ec2bbfc`.
It passed independent ordering and four concurrent cross-thread runs on fresh databases.
Each run observed six mutations, twelve paired wire outputs and both clears in SQLite.
Three oracle tests rejected dropped, duplicate, misrouted and out-of-order messages.
This build came from ALab main `752ddc6b3e`. The reviewed goal queue, request processor,
SQLite pool configuration and native goal API matched the earlier `21e660cf42` candidate.

The same frozen helper passed the bare shared-login picker from a signed 0.11 seed. It
preserved the original conversation and completed tool record, and the resumed
turn used the destination login. This is an independent CLI compatibility observation.

At `4b7cc0ef1e` both protected state tests passed. The retry deadline bounds admission
to another FULL checkpoint. The implementation checks the monotonic clock after backoff
and preserves SQLite’s busy handler for a checkpoint already admitted. A separate SQLite
3.53.1 control with a 160 ms admission budget admitted a FULL attempt at 121.18 ms.
The attempt returned an incomplete `(1,4,3)` at 339.48 ms with an old reader still open.
After the reader released, a new FULL checkpoint returned `(0,4,4)`. Those timings measure
the standalone database control. The five public CLI runs above measured `f815886684`.

## Live Cruise stall on signed build 2026111701

On November 19, the Clean qualification conversation kept its goal active but
stopped starting turns in Cruise. The first accounting warning at 15:38:30
Pacific reported a committed usage update whose WAL checkpoint returned busy
with frame counts `-1/-1`. Subsequent tool-finish and turn-end warnings reported
the ambiguous-write hold. The TUI later showed an idle prompt while aterm still
reported `working`. A manual question received a reply and reproduced the hold.

The installed source, `70d6391259`, predates the checkpoint-only retry already
on main in `f815886684`. Keep the existing retry and regression coverage. Signed
index 166 still selects the older build. An exact-source packaging attempt
stopped at compiler selection because the selected payload had unowned path
indirection. Live recovery remains unverified. Restart would interrupt a
validation job still attached to the original process, so leave it running.

## Requesting connection closes after a goal commits

On November 19 the CLI from `96042e0c18` passed three independent Unix socket
runs. Each used two app-server clients and a fresh local goals database. One
client materialized the thread and stayed subscribed. The other set its goal
to paused. An old WAL reader still saw zero goals while a fresh reader saw the
new objective. The requesting client then closed its socket before receiving
a response. The subscribed client received no goal update while the old read
transaction remained open.

After the reader released, the subscriber received the matching update.
Its public goal read matched the notification, then its clear response preceded
the clear notification. Both SQLite and the public API reported an empty goal.
The wire captures contain no response to the abandoned request on either client.
The client recorded WebSocket close code 1006 on all three runs. This records
transport loss without a peer close acknowledgement. The server exited zero on
orderly fixture shutdown. This measures connection loss after logical commit
with a live thread listener. Its internal cancellation time and output-enqueue
time remain unobserved.

The frozen binary had SHA-256
`055dbdd11e8c0058d3a39d3e227dd4c31d2d8749a92b99e084183d8e762944b0`.
The first of the three receipts had SHA-256
`fe567104a20f39fd9ca76464e22ab62a0ec81245478f817f60cfc5a2f584321c`.
On the same binary the independent stdio order control passed all six
mutations on loaded and unloaded threads. The four wire-oracle tests passed,
including rejection of a leaked reply, missing or duplicate update, wrong
thread and reversed clear delivery.

Run the disconnect control on macOS or Linux with Python 3.14 and the Python
`websockets` package. The recorded run used `websockets` 16.1.1.

```sh
python3.14 -B alab/smoke/goal_observer_disconnect.py --codex /path/to/bin/codex --output /path/to/new-output
```

The full seven-scenario contract remains without a qualification recipe. A partial producer
cannot issue a complete coverage receipt.

## Global goal output reservations

The protected app-server library run at `d990d66e5f6cdf40796bb748f1c753a01c9803b8`
passed three controls through the source-selected Trust 15698 test lane. Two existing
controls covered FIFO delivery, independent progress and the eight-reservation per-thread
limit. The new control held one enqueued delivery open and reserved the remaining 127
slots across separate thread IDs. A fresh thread was refused. Releasing one slot admitted
its output, and another claimant was refused while that slot was reserved. The admitted
output delivered before the blocked thread was released. Completing the held output and
dropping the remaining reservations returned all 128 slots.

These observations exercise the production app-server queue used by the goal RPC processor.
The 127 pending reservations were acquired directly in the library test. The native result
log had SHA-256 `21ff491ee42a767076134da796a53b8625df601b2a0341afb594ae83d3b60a9c`.

## Concurrent goal writes at the ordinary file descriptor limit

On macOS, development CLI `8c12251a4e` received 129 concurrent goal updates
while SQLite's writer was held. One request reached the intended 128-request
backlog limit. At a 256-descriptor limit, only 66 of the 128 admitted updates
succeeded. The server exhausted its file descriptors while opening storage
for the other conversations. The same executable completed all 128 admitted
updates with the test runner's higher descriptor limit.

Commit `ae15894e5b` bounds simultaneous storage operations to five, matching
the state database connection pool. Each mutation reserves its output before
waiting for storage. On CLI `a14f29524b`, three runs with fresh databases and
a 256-descriptor limit each acknowledged and persisted all 128 admitted
updates. Their responses preceded their matching notifications. After the
backlog drained, the refused conversation accepted a retry and a clear.
Its row disappeared and the other 128 rows remained. The tested CLI had
SHA-256 `e15946bbd2c028832d311f6eee3bd9ee028868324c1e25f2700ad283043d9216`.

The control uses one loaded conversation and 128 persisted, unloaded forks.
Only the CLI child inherits the 256-descriptor limit.

```sh
python3 -B alab/smoke/goal_output_order.py --codex /path/to/bin/codex --output /path/to/new-output --global-pressure
```
