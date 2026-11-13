# Joined terminal input

This ALab-owned Unix input driver supplies the reader lifecycle and parser data
needed for a lossless live update. The ALab binary's TUI streams, startup replay,
protected-screen reads and runtime cursor queries share this reader through the
owned broker. Other library consumers keep crossterm. The update controller still
has to join the input fence with Core/goal/SDK capture and transport; this alone
is not a complete live-update capability.

`Driver` owns one independently opened, nonblocking terminal description, a
reader thread, and a wake socket. Freeze signals that thread, joins it, and
returns the original descriptor with its parser. SIGWINCH also wakes the socket;
there is no periodic idle poll. Bytes arriving after freeze remain on the PTY.
No new input read can occur after freeze returns. Dropping a driver also joins
its thread; callers that need to retain state must freeze rather than drop it.

`Parser` retains decoded events together with the bytes and raw-mode policy
that produced each event. Its checkpoint preserves their order, resize records,
partial UTF-8/control/paste sequences, an existing security-screen quarantine,
and the exact monotonic baseline for a pending Escape timeout. Restore reparses
complete records, rejects partial/extra queued data and malformed checkpoints,
and retains that baseline instead of starting a fresh timeout. A checkpoint is
bounded before serialization; ordinary input does not disappear when it is too
large to checkpoint. Quarantine is an explicit security-screen operation, never
an update fence.

The source of `parse.rs` and the sequence framing/quarantine rules is
`openai-oss-forks/crossterm` commit
`ed1cdab335221515706178d68495bba2aed1924f`, which is the workspace's pinned
crossterm revision. The MIT notice is in `LICENSE.crossterm`. ALab owns the
adaptation because that revision hides its parser state and discards the input
thread's JoinHandle. Changes retain crossterm's public event types, make raw-mode
policy explicit, and keep internal query responses as owned data. The imported
parser cases remain tests of this adaptation. Reassess the adaptation when the
workspace changes its crossterm revision; do not silently switch parsers.

Neither a parser checkpoint nor a terminal file descriptor grants native SDK,
account, model, or continuation admission. The production transport independently
establishes same-process/boot custody and the destination obtains fresh native
admission before releasing work. The standalone exec fixture passes only test
input in its environment; production uses authenticated descriptor transport.

`Broker` retains its original input description and parser across pauses. Its
subscriptions share one driver; the last subscription joins that driver and
retains its accepted data. Query filtering and peeking leave unrelated keys in
order. Wakeups fan out to registered consumers, including when a filtered query
leaves or a prepared fence rolls back. Reader construction transfers the parser
only after a successful thread/initialization handshake. Even a failed read
recovers accepted parser data into the broker.

A source subscription can prepare a fence for its exact broker. Preparation
joins the actual reader, blocks new starts and creates a bounded checkpoint.
Commit holds that input gate through the caller's final synchronous decision;
refusal stays revocable, and successful commit never silently reopens. Higher
TUI filter/composer state still belongs to the TUI and must be checked under its
owner lock before this operation. The full capsule/startup controller is pending.
