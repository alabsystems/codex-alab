# History backends and asynchronous trace storage

Review the next two commits through
`b8dde1eb4048520a8b6e982a27bc99ef5b2dfe7f`, extending the unchanged prior 41
decisions to 43. There are 53 later commits in the observed upstream snapshot.
This is source review, not an integrated build or runtime qualification.

## History/notes backend traits (`aa5f492031`): adopt

The request registration macro now binds namespace, tool name, request type,
catalog key and parallel-call policy. Existing note mutation tools remain
nonparallel. HTTP routes come from this registration rather than catalog
text, and HTTP tools retain their direct-model exposure. Catalog descriptions
and parameters remain replaceable.

The HTTP backend deliberately deserializes opaque JSON values so future
catalog arguments survive forwarding. It still requires an object and
overwrites its context with the thread's trusted session and agent identity.
That identity is now captured at thread start in `HttpContext`, then shared
with the backend. Feature, token-budget, OpenAI-provider and authentication
gates remain in extension setup and update.

Output handling still removes image attachments before logging/hooks and
keeps encrypted output in its model-facing representation. Server-side output
budget headers and encrypted-argument route selection are unchanged. Read all
production and test changes; require the unchanged bundled-contract snapshot,
forged-context replacement, unknown future argument forwarding, catalog override,
encrypted-output and malformed-image tests on integration. ALab private exec
configuration preserves the extension option, but no ALab direct caller of the
replaced backend/tool structs was found.

## Trace worker (`b8dde1eb40`): keep recording opt-in

One worker owns each bundle's files. FIFO jobs preserve event order and write
payloads before their references. The synchronous channel permits eight
pending jobs; full-channel submission blocks the caller instead of dropping
evidence. This bounds job count, not bytes, and does not make every trace call
nonblocking. Borrowed observations are still serialized before submission.

Inference starts share encoded bytes and an immutable `Arc<Prompt>` with the
worker. WebSocket request repair and remote compaction retry use copy-on-write
so a queued snapshot keeps its original input. The request-only compaction
trigger is removed from checkpoint history after success and before rebuilding
a rejected-audio retry. Disabled compaction tracing does not evaluate the
snapshot closure.

Explicit writes wait for their result. Live readers use the new `flush`
barrier. Dropping the last writer closes admission, drains accepted work and
joins the worker; the closing event is attempted only after earlier jobs and
is suppressed by the retained first write/serialization failure. This is
recording closure, not proof of task success. Keep the diagnostic feature
opt-in and preserve its existing error policy.

Read all 26 changed paths, including tool/agent receipt ordering and adjusted
live-read tests. Require the gated-serializer test, start/completion ordering,
shutdown drain, deferred and borrowed serialization failures, compaction
worker/disabled tests, WebSocket warmup snapshots, rejected-audio retries,
backpressured cancellation and tool-dispatch receipt tests. The changed
`&Arc<Prompt>` stream signature has no ALab direct caller found; API client
`stream` calls in ALab auth tests are a different interface.

Run ALab's opaque-error HTTP/WebSocket recovery and private exec-handoff tests
against the eventual integrated candidate. The certified application gate
remains required and currently blocked; none of these source observations
waives its proof requirements.

## Validation

Check exact 43-commit metadata coverage, prior 41 entries unchanged, unique
active decisions and rejection of the next unreviewed target. Committed
admission fixtures check only this graph and its supersession declarations.
