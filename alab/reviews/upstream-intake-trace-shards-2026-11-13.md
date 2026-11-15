# Trace shards and exact request evidence

Review `e73744b3f46dcc6786ab08405c2715c898511281`, extending the reviewed
upstream prefix from 34 to 35 commits. Preserve all earlier decision objects.
This records source review only. No upstream code is merged or qualified by
this decision, and 61 commits remain after this prefix in the observed
`f941a5fb92` snapshot.

## Decision: keep diagnostic recording opt-in

Adopt the storage and request-evidence improvements when integrating, while
retaining the existing opt-in diagnostic recorder. Prepared prompts include
full context and provenance, while outgoing requests retain provider-facing
bytes. This is diagnostic artifact storage, not the normal session JSONL
format or ALab's private exec handoff encoding. Searches found no direct ALab
consumer of the changed trace types, payload paths or inference-start API.

The exact-byte capture is useful for future opaque backend errors, including
the Bad Request under investigation. It cannot reconstruct the original
rejected request retroactively. HTTP capture observes `EncodedJsonBody` bytes
before dispatch; WebSocket capture observes the same `Utf8Bytes` handle passed
to the sender. Prepared prompts use a separate payload role and retain readable
reasoning in the diagnostic serialization. A continuation following an
unrecorded warmup keeps both the actual incremental message and full formatted
context for semantic replay. No change to request content is intended.

## Storage and failure boundaries

Schema 3 replaces per-payload JSON paths with numbered Zstandard shards.
The reducer explicitly rejects a different manifest/event version and directs
the caller to the recording executable. Keep that executable with retained
diagnostic artifacts; do not invent a silent version-2 migration.

The writer flushes each whole payload before publishing its reference. Related
request, prepared-prompt and warmup payloads are written with their start event
under one lock. The final shard is sealed before the closing event. The first
write failure poisons further recording, including closure, rather than
claiming a complete artifact. Inference terminal events are emitted only after
a successful start row. Recorder closure remains distinct from task success.

Records have checked ID lengths, explicit body lengths and duplicate-ID
rejection. Rotation counts headers and IDs, with an oversized record kept
whole in its own shard. The reader caches two ordinary shards, or one oversized
shard, reloads an open shard when it needs newly published data, and requires
the footer for a closed bundle and earlier shards. Validation covers every
referenced payload, including opaque data not otherwise read by the reducer.
Malformed UTF-8/JSON, conflicting references, missing records and corrupt
checksums are errors. Partial live records are not published records.

The 32 MiB target bounds rotation, not total decoded size: oversized bodies
are allowed and `read_to_end` materializes the shard. Keep resource stress
checks in integration qualification; do not describe this as a hard memory
bound. `PayloadCapture` distinguishes complete, truncated and omitted bodies;
exact replay rejects missing descendants, and reduced closure distinguishes
unclosed from closed-with-partial-payloads and complete.

## Required integration checks

Read all changed production paths and regression diffs. Require shard rotation
and exact-fit boundaries, oversized bodies, live prefix and sealing, cache
eviction, duplicate references, malformed bytes and checksum failures.
Require HTTP byte equality, WebSocket warmup and next-turn deltas, prepared
provenance, response interception, backpressured cancellation, shutdown and
child-recorder lifetime. CLI trace tests now reduce with the actual recording
binary. Guardian denial and large MCP-result tests use `PayloadReader`.

Rerun ALab's HTTP/WebSocket opaque-error recovery and exec handoff tests on the
integrated candidate: the upstream observer hooks touch the same transport
paths. The certified application gate must include the new `zstd` dependency
edge; the upstream lock changes add dependency edges without changing package
versions. Do not bypass the existing procedural-macro certification refusal.

Validate exact 35-commit metadata coverage, unchanged prior 34 entries, unique
active decisions, and rejection of the next unreviewed commit. Those checks
are separate from the native runtime requirements above.
