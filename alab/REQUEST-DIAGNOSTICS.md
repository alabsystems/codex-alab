<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Responses request diagnostics

When ALab behavior is active, the Responses endpoints emit local INFO
events under `codex_api::endpoint::alab_request_diagnostics`:

- `Responses request shape`: metadata for a prepared endpoint request.
- `Responses request failed`: the selected terminal error for that request.
- `Responses WebSocket handshake rejected`: an HTTP upgrade rejection before
  sending a model payload.

Responses HTTP requests captured with diagnostics also emit
`Responses HTTP operation completed` under the dedicated INFO target
`codex_api::endpoint::alab_request_diagnostics::http_operations`. These rows
report failed operations and successes after an earlier operation failed; an
ordinary first-operation success stays quiet. Their scope is described below.

The default HTTP transport also emits `HTTP rejection body capture
finished` under `codex_http_client::response::alab_error_body_capture` for the
rejection-body phase described below.

These describe source capabilities. An older installed package may not contain
them; an absent event does not establish that no request or failure occurred.
The TUI's default SQLite filter admits these events when its log database is
available. A standalone API caller must supply its own logging subscriber.

First identify the current history source and retain its recent turn records.
A legacy rollout can be stale while the session uses paginated SQLite history.
For a verified current, unbranched paginated root whose logical thread ID equals
its rollout ID, read `thread_history_1.sqlite` under the effective `sqlite_home`.
`log_dir` relocates logs, not this history database. Forks and reverts require
the existing history API and lineage resolution; do not assume their logical
thread ID identifies the current rollout. The source resolver and paging rules
are in [thread_rollout_resolver.rs](../codex-rs/thread-store/src/local/thread_rollout_resolver.rs)
and [segment_paging.rs](../codex-rs/thread-store/src/local/thread_history/segment_paging.rs).

List recent statuses together so an older failed turn is not mistaken for
current activity. Preserve `error_json` intact in private evidence before
correlating its turn ID with the request logs:

```sh
# Supply the verified current rollout ID and effective history database path.
# Use a new private output path; existing evidence must not be overwritten.
(
umask 077
set -C
python3 - "$thread_history_db" "$current_rollout_id" > "$private_turn_evidence" <<'PY'
import json
import sqlite3
import sys
from contextlib import closing
from pathlib import Path

uri = Path(sys.argv[1]).resolve(strict=True).as_uri() + "?mode=ro"
with closing(sqlite3.connect(uri, uri=True)) as db:
    db.row_factory = sqlite3.Row
    rows = db.execute("""
        SELECT thread_id, turn_id, rollout_ordinal, status,
               started_at, completed_at, duration_ms, error_json
        FROM thread_turns
        WHERE thread_id = ?
        ORDER BY rollout_ordinal DESC
        LIMIT 10
    """, (sys.argv[2],)).fetchall()
    json.dump([dict(row) for row in rows], sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
PY
)
```

The default logs database is `$CODEX_HOME/logs_2.sqlite`, with `CODEX_HOME` normally
`~/.codex`. Use the running session's effective configuration: `sqlite_home`,
`CODEX_SQLITE_HOME`, or an explicit `log_dir` can relocate it. A managed
`sqlite_home` requirement keeps SQLite logs there despite `log_dir`. This is
the dedicated logs database, separate from `state_5.sqlite`.

For a bounded metadata-only timeline, first set `request_logs_db` to the verified
database path and `codex_pid` to the verified process ID. Supply an explicit
process or thread scope:

```sh
python3 -I -B alab/request_timeline.py \
  --db "$request_logs_db" --pid "$codex_pid" --limit 100
```

Use Python 3.11 or newer. `--process-uuid` selects an exact canonical
`pid:N:UUID` identity, `--thread-id` selects a canonical thread UUID, and supplied
filters intersect. `--since` is an inclusive Unix-second lower bound applied to
integer stored seconds before the row limit; unusable seconds are excluded.
Without a time filter, unknown timestamps remain visible.
Use `--target` to select one of the exact diagnostic targets listed in
`--help` before the row limit applies. For example, add
`--target codex_core::responses_retry::alab_capacity_retry` to a thread-scoped
query to see retained rows from that module even if newer API events fill the
default window. Use `codex_core::responses_retry` for the parent module.
A PID can have multiple retained process instances; their UUIDs remain separate. These
filters do not identify the current rollout, resolve forks, or discover a
session's effective configuration.

The command opens an existing current-schema database through SQLite's read-only
URI mode and emits one JSON document. Only UTF-8 database encoding is supported;
another encoding returns `unsupported_database_encoding` before metadata
selection. Path-resolution failures, including symlink loops, report
`database_unavailable` before SQLite is opened. It selects row IDs, timestamps, levels,
fixed diagnostic targets and validated process/thread identities. Rendered bodies,
span fields, paths and error messages are not selected. This limits SQL projection
and output, rather than isolating physical database pages. Row count and returned
metadata size are bounded; SQLite VM, elapsed-time and lock budgets limit query
work without promising a hard I/O deadline. Active WAL records remain visible;
this is neither an immutable-file snapshot nor a pathname-custody guarantee.

The exact target `codex_core::responses_retry` is included. Its WARN rows mix
ordinary sampling retries, remote-compaction V2 retries and unbounded connection
waits; this adds no coverage for other legacy compaction targets. Metadata alone
does not distinguish these warning types, expose retry counts, pair attempts or
turns, or measure pacing. Older emitters using targets outside the fixed allowlist
remain excluded.

The exact HTTP-operation target
`codex_api::endpoint::alab_request_diagnostics::http_operations` is also selected.
Matching remains an explicit allowlist, including neither arbitrary descendants
nor similarly named targets. The timeline still returns row metadata only; it
does not extract `local_attempt` or `operation_attempt` from formatted fields,
pair their rows, or reconstruct retry decisions.

Rows are returned in ascending row-ID order within the latest selected window.
The limit and `more_rows_available` describe SQL candidates before canonical
identity validation. A true flag means a lookahead candidate exists, which need
not become a returned row. `omitted_invalid_scope_rows` counts only omissions
inside the limited window, excluding that lookahead; the command does not refill
the window past these bounds.
`selected_row_gap_ns` compares stored wall-clock timestamps only within the same
validated process, thread, target and level. Missing identities are not joined;
unusable or backward timestamps supply no duration. These are selected-row gaps,
not retry intervals, remaining deadlines, outage durations or complete request
counts. The command does not parse formatted bodies to infer an error kind,
pair attempts or decide whether a task made progress. Buffering, retention,
older emitters and omitted invalid metadata can all leave incomplete evidence.

Read an existing database without creating one or changing its contents:

```sh
request_logs_db="${CODEX_HOME:-$HOME/.codex}/logs_2.sqlite"
# Replace the path above if the session's effective configuration relocates logs.
sqlite3 -readonly "$request_logs_db" <<'SQL'
.headers on
.mode line
PRAGMA table_info(logs);
SELECT id, ts, ts_nanos, level, target, process_uuid, thread_id, feedback_log_body
FROM logs
WHERE target IN (
  'codex_api::endpoint::alab_request_diagnostics',
  'codex_api::endpoint::alab_request_diagnostics::http_operations',
  'codex_http_client::response::alab_error_body_capture'
)
ORDER BY id DESC
LIMIT 40;
SQL
```

The current table stores formatted fields in `feedback_log_body`, including
inherited span context. The Rust `query_logs` API exposes that column as
`message`; SQL against the current schema must use `feedback_log_body`.

Pair prepared-request records using **`process_uuid` plus `local_attempt`**. The attempt counter
is process-local and restarts in another process. One attempt identifies one
prepared endpoint request; internal HTTP transport retries can share it.
It is not a conversation turn ID or a count of every network transmission.
An endpoint failure is not a final conversation verdict: core may retry or
switch transport and continue the same turn. Follow later attempts and the
higher-level recovery result.

`thread_id` is also a database column when the event or its admitted ancestors
supply it. Production turn spans carry `turn.id` in the formatted body. The
prepared-request failure diagnostic retains its original subscriber and span, including
their ancestors, after the caller's scope exits. A caller without that context
cannot supply missing thread or turn identity through these events.

For Responses HTTP streaming requests, the operation event correlates each
completed operation result with the immutable prepared-request `local_attempt`.
An operation may terminate during authentication, source admission or transport.
The original request observer receives its unchanged
arguments in its ambient callback subscriber and span first. Only the local INFO
emission restores the captured request subscriber and span. Concurrent calls on
one client retain separate diagnostics; the client's shared observer is unchanged.
Calls prepared without a diagnostic are not backfilled by later activation.

| HTTP operation field | Interpretation |
| --- | --- |
| `local_attempt` | Prepared endpoint identity; pair with `process_uuid`, as above. |
| `transport` | Fixed `responses_http`. |
| `operation_attempt` | Original zero-based retry-loop operation index. Authentication or source admission may fail before any wire send. |
| `elapsed_ms` | Supplied duration of this completed operation, truncated to milliseconds and bounded to a `u64`; not the selected-failure elapsed time or a retry interval. |
| `http_status` | Typed status supplied by the callback, when available. A later success can report an opening status such as 200. |
| `failure_kind` | Fixed transport-error label, omitted on success. |
| `retry_advised` | Present only for an HTTP error's typed advice slot: true for retained `Some`, including zero/expired advice, false for `None`. Omitted for success and non-HTTP errors. |
| `request_id`, `cf_ray` | Independently accepted bounded tokens from this HTTP error's named headers only; success retains no previous error IDs. |

Failure labels are `http`, `network_policy`, `retry_limit`, `timeout`,
`connection`, `network`, `request_build` and `response_too_large`. The new local
operation logger neither formats error text nor selects bodies, URLs or arbitrary
headers. It reads only named correlation tokens from HTTP error headers and does
not parse retry headers, read deadlines or sample wake clocks. The original
caller observer still receives its original error and retains its own behavior.
Advice presence grants no retry authority and reports no chosen wait.

An operation success means the HTTP operation returned and its response stream
opened. It is neither successful SSE completion nor a final turn verdict.
An emitted failure does not establish that another retry was scheduled; the
retry budget or policy can select it as final. The index is not a wire-send or
conversation-turn count. Serialization/preparation failures before the operation
and cancellation of an incomplete operation add no fabricated completion row;
a completed earlier failure may already have a row. An inner callback panic
retains its original behavior and can prevent the subsequent local emission.
These records cover Responses HTTP streaming setup, not other endpoint methods
or WebSocket exchanges. They are not a complete first-success opening ledger.

Complete HTTP rejections already have the separate body-capture INFO phase.
Operation records add request/index correlation and non-HTTP or pre-wire failure
evidence; they do not replace that phase or the selected endpoint-failure event.
Logging, queue loss and retention can still leave incomplete evidence.

The separate HTTP capture event has `phase="rejection_body"`. It observes an
unconfigured (`response_body_limit_bytes=None`) non-2xx response in
`ReqwestTransport::stream` or buffered `ReqwestTransport::execute` while ALab
behavior is active. It uses available parent subscriber/span context and emits
no `local_attempt`; correlate through process, available thread/turn identity
and accepted response IDs. Buffered endpoints such as Images can emit this
phase without any Responses prepared-request shape or failure record.
Dropping its still-pending owning future emits no completed phase.

| HTTP capture field | Interpretation |
| --- | --- |
| `phase` | Fixed `rejection_body`. |
| `http_status` | Status received in the original response headers. |
| `body_capture` | `complete`, `too_large`, `timed_out`, `read_error` or `policy_denied`. |
| `body_wait_ms` | Elapsed time from capture initialization after headers to its outcome; includes any original source-admission wait. |
| `rejection_body_bytes` | Complete retained entity bytes after Reqwest transfer/content decoding and before text conversion; omitted when unavailable, zero for measured empty. |
| `rejection_body_bytes_read` | Entity bytes returned by successful body reads, including a chunk rejected by the retention limit. Zero means chunk collection began without data; omission means no count was available. |
| `retry_advised` | Boolean for the already captured typed advice slot, including zero/expired advice; not remaining delay or retry authority. |
| `request_id`, `cf_ray` | Sanitized correlation tokens from the original response headers. |

The read count can survive timeout, read failure or policy denial while the
complete-only length is omitted. It measures bytes Reqwest returned after
transfer/content decoding and before charset/BOM conversion, rather than wire
traffic, retained allocation or a complete body. It can exceed 64 KiB when a
returned chunk triggers the retention limit. It is omitted if collection never
began (including a declared-size rejection, pre-read denial or an already-ready
deadline) or the exact count cannot be represented. Dropping the owning future
still emits no completed phase. The original source-admission and policy rules
continue to govern the read; this observation grants no recovery authority.

Default capture retains at most 64 KiB of entity bytes and requires a complete
HTTP framing end. Its fixed five-second absolute deadline starts after response
headers and does not reset for chunks. Streaming errors preserve existing
charset, BOM and replacement decoding. Buffered errors keep strict UTF-8
conversion: a UTF-8 BOM remains in the text, and invalid UTF-8 leaves `body=None`
even when `body_capture=complete` records a measured entity length. The limit
is not a cap on the expanded UTF-8 string, individual network allocations or
total process memory.

Size, read and body-timeout failures leave the body unavailable (`None`) while
preserving the known HTTP status, original headers and header-time retry advice.
An incomplete JSON prefix cannot establish an opaque 400, and an unavailable
body supplies no provider terminal code. The original permit still governs
source preparation, admission and revocation: pending admission is not replaced
by an ordinary HTTP body timeout, and policy denial wins and remains a policy
error. The phase's `http_status` therefore does not promise a returned HTTP error
when `body_capture=policy_denied`.

Inactive behavior, explicit `Some` response limits and successful responses
keep their existing paths. This rejection bound does not cap successful SSE or
buffered image/search bodies, or change request/header timing. The emitter
excludes payloads, URLs, arbitrary headers and raw errors; inherited
embedding-caller span fields are not covered by that emitter guarantee.

An HTTP upgrade rejection has no prepared model payload. Its standalone event
has `phase="handshake"`, no `local_attempt`, and no request bytes, model, input
or tool counts. `upgrade_request_id` and `upgrade_cf_ray` identify the rejected
upgrade; they do not identify a later model response. Use its process, thread,
turn and accepted upgrade IDs for correlation.

| Handshake field | Interpretation |
| --- | --- |
| `http_status` | The rejected upgrade's original HTTP status. |
| `body_capture` | Fixed outcome of bounded HTTP entity capture; omitted when no typed capture observation is available. |
| `body_wait_ms` | Time from complete rejection headers to the capture outcome. |
| `rejection_body_bytes` | Decoded HTTP entity length, only when captured completely; zero means measured empty. |
| `rejection_body_bytes_accepted` | Entity bytes accepted by the WebSocket rejection decoder, including incomplete bodies; zero means a decoder was created but accepted no entity bytes. Omitted when no decoder was created or no typed capture observation is available. |
| `retry_advised` | Whether provider advice was recognized. Its deadline is captured when headers arrive. |
| `upgrade_request_id`, `upgrade_cf_ray` | Sanitized IDs from the rejected upgrade's response headers. |

The accepted count is bounded by 64 KiB and excludes raw transport reads,
transfer framing and entity data rejected by a decoder limit. Unlike the HTTP
capture's `rejection_body_bytes_read`, it does not include an oversized returned
chunk. Unsupported or malformed headers that prevent decoder creation leave it
absent. Partial timeout, truncation, malformed framing or read failure can retain
accepted progress while the complete-only length remains absent. This observation
does not create a handshake event on policy-denial passthrough or cancellation.

The body reader preserves complete entities across transport reads, including
chunked transfer framing. Incomplete, ambiguous, oversized, malformed or timed-out
entities remain unavailable; a partial JSON prefix is never treated as complete.
Capture limits rejection entities to 64 KiB, wire body bytes to 128 KiB,
chunk metadata to 16 KiB and individual chunk/trailer lines to 4 KiB. The
five-second body deadline starts when complete rejection headers arrive and
does not reset for later fragments. Header buffering retains the original
64 KiB, 512-read and read-density bounds; caller timeouts govern header timing.
Shorter outer connection timeouts and source revocation still win. This event does not cover earlier
admission, authentication or TLS failures, successful upgrades, or cancellation
before an HTTP rejection reaches the mapper. Explicit handshake probes can
produce the same event; it does not establish a model request. It records no
body text or URL.

A proxy's HTTP CONNECT failure precedes the target WebSocket Upgrade. It is a
proxy transport error and does not produce an upgrade-rejection event. An
absent handshake event therefore does not validate the proxy request bytes.
IPv6 targets use bracketed HTTP CONNECT authorities and binary IPv6 addresses
in SOCKS5/5h; the original target URI and TLS identity remain authoritative.
An HTTPS proxy at an IPv6 address keeps brackets in its TCP authority but uses
a bare IP identity for certificate verification. Proxy TLS failures precede
both CONNECT and the target Upgrade, so they produce no upgrade-rejection event.
Malformed environment WebSocket proxy errors retain their configuration-error
category while redacting the original URL or userinfo before public API errors
and connection telemetry convert them to text.

| Field | Interpretation |
| --- | --- |
| `transport` | `responses_http` or `responses_websocket`. |
| `model` | Bounded token-shaped model label, when accepted. |
| `request_bytes` | Serialized JSON bytes before HTTP compression; for WebSocket, serialized request text bytes before framing or negotiated compression. |
| `compression` | HTTP selection: `none` or `zstd`. |
| `mode` | `incremental` when a previous response reference is present, otherwise `full`. |
| `connection_reused`, `warmup` | WebSocket flags; warmup means `generate=false`. |
| `input_kind` | Observed JSON representation, such as `array`, `string`, `null`, or `missing`. |
| `input_items` | Number of items in the submitted array. |
| `messages`, `reasoning_items` | Counts of those item kinds. |
| `tool_calls`, `tool_outputs`, `other_items` | Counts grouped by item kind. |
| `tools`, `tool_bytes` | Tool metadata measurements described below. |
| `elapsed_ms` | Time from shape observation to the selected failure. |
| `failure_kind`, `http_status` | Error classification and its HTTP status, when available. |
| `opaque_bad_request` | Exact opaque HTTP 400 envelope recognized at this diagnostic boundary. |
| `retry_advised` | Retained typed advice: `true` for a captured value, `false` for an empty advice-bearing slot; omitted for types without that slot. |
| `request_id`, `cf_ray` | Accepted response correlation tokens, when supplied. |

Selected-failure `retry_advised` is a normal optional boolean. Only HTTP transport
errors, generic retryable errors, rate-limit-exceeded errors, server overload and
policy-capacity errors carry this typed slot. A retained value is `true` even if
zero or already expired. `false` describes the typed slot, not proof that no raw
header existed; a type without that slot omits the field even if its wire body
contained retry metadata. No header/body is reparsed to infer this fact.

Enhanced mode tolerates repeated `Retry-After` fields by selecting the latest
valid, timer-safe deadline from one receipt time. Invalid values do not erase a
valid wait. JSON header names that differ only in case retain their individual
retry values for this selection; other header collision rules are unchanged.
This is ALab's recovery policy for ambiguous metadata. Values remain whole:
comma-separated text is not split because an HTTP date itself contains a comma.
Exact duplicate JSON keys already collapsed by the existing JSON decoder cannot
be recovered here. Vanilla and unactivated clients retain their original selection.

An unknown SSE failure can stay buffered while the stream waits for completion.
In enhanced mode, a later ordinary idle/transport stream failure retains any
validated advice captured on the latest buffered retryable failure, including
rate-limit or capacity errors buffered in vanilla before enhanced mode is enabled.
Only pacing is retained; the pending rate/capacity category is not re-emitted.
Its selected tail message stays unchanged. When a diagnostic was captured at request
opening, the failure is recorded as `retryable` with `retry_advised=true`, including
expired advice. A request opened in vanilla has no diagnostic to backfill after
activation. Completion, EOF, policy denial,
a newer unadvised failure and inactive behavior keep their existing ownership.
No deadline is reparsed, restarted or extended at this terminal boundary.

This flag grants no retry authority and reports no remaining or chosen delay.
It does not read a deadline or sample the shared wake observer. Core wait and
recovery records remain authoritative for timing; a current recovery wall can
retain a provider deadline, but need not preserve every earlier selected request.
Use the existing process/attempt/thread/turn and server IDs for correlation.

For typed transport failures, `failure_kind` is `http`, `network_policy`,
`timeout`, `connection`, `network`, `request_build`, `retry_limit` or
`response_too_large`. These are fixed categories, not parsed error messages or
proof of backend contact. A request/auth build failure or policy refusal can
occur before any transport send. An error already represented as `Stream`
retains `stream`; its original subtype is unavailable at this boundary. Normal
Responses exhaustion retains its final error, and the endpoint does not opt into
the generic whole-response limit. Its separate default rejection-body budget
retains HTTP identity rather than producing `ResponseTooLarge`, so the last two
defensive variants do not establish those conditions in an ordinary request.

The normalized `opaque backend HTTP 400: ` message prefix is reserved for the
bridge's verified HTTP 400 producer. Raw retryable, stream, untyped rate-limit and
network/build messages beginning with that prefix gain `provider stream: ` at
the API-to-core boundary; their complete original text and existing retry advice
remain available. They stay ordinary stream errors and acquire no HTTP status.

Reservation follows ALab binary identity, including vanilla mode, because an
error can remain queued while enhanced mode changes. Ordinary strings in every
mode and unactivated upstream behavior stay unchanged. Genuine opaque HTTP 400
normalization still follows the enhanced-mode recovery policy. This boundary
protects current Responses recovery consumers; trusted internal code remains
responsible for constructing its own errors correctly.

Semantic API variants also retain fixed categories. Existing `api`, `stream`,
`retryable`, `invalid_request`, `invalid_prompt`, `context_window` and `incomplete`
labels remain unchanged. Additional type families use these labels:

| Semantic type family | `failure_kind` |
| --- | --- |
| Quota and budget | `quota_exceeded`, `usage_not_included`, `session_budget_exceeded` |
| Capacity and rate limits | `rate_limit_exceeded`, `rate_limit`, `server_overloaded`, `policy_capacity_unavailable`, `flex_unavailable` |
| Validation and access | `invalid_audio_input`, `model_not_found`, `access_program_not_enabled` |
| Policy | `inference_hook_denied`, `content_policy_violation`, `cyber_policy`, `bio_policy`, `misalignment_policy_violation` |
| Deferred filter outcome | `content_filter` |
| WebSocket size refusal | `websocket_message_too_large` |

These describe the selected API type at this observer, not a parsed provider code
or a retry decision. HTTP errors and explicit-status WebSocket wrappers remain
`http` even when the later protocol bridge maps their bodies to a specific error.
Only the existing HTTP/API status cases supply `http_status`; an opening 200 or
an optional semantic status field does not become a new error status here.
Deferred content-filter failure selected at EOF is `content_filter`; completion
supersedes it without a failure record, and a selected idle/I/O error remains
`stream`. The untyped `rate_limit` and `api` variants have no observed constructor
on the Responses stream paths. Encoding failures before diagnostic creation and
connection setup outside this observer do not acquire a fabricated request record.

When ALab is active, a standalone `error` with an exact named-object
`invalid_request` or `invalid_prompt` code shares the validated `response.failed`
parser. A selected refusal is delivered immediately, keeps its diagnostic
category, and receives no retry advice. `invalid_prompt` preserves present blank
messages; only missing/null messages use `Invalid request.`. Malformed fields,
positional arrays, type-only or near-code objects keep the ignored standalone
path. Other policy/quota/rate codes gain no new admission from this rule.

Valid WebSocket HTTP wrappers retain status precedence. A malformed wrapper can
still supply an independently valid request refusal, but that semantic diagnosis
does not assert HTTP status, authentication state or account-recovery authority.

When ALab is active, an unambiguous non-success WebSocket HTTP status also
survives a malformed optional error body. Strict decoding runs first, including
existing special reconnect-code handling. Recovery requires unique root fields,
a valid single status alias or two equal valid aliases, and unique fields in a
named error body. Only optional nested headers reenter the parse copy; code and
message cannot grant a special reconnect route. The original payload remains
unchanged for the ordinary HTTP bridge, which keeps its own retry policy.
During this fallback, scalar or positional bodies supply no inferred nested
headers. Conflicting,
invalid, repeated or statusless wrappers gain no new HTTP identity; the existing
bounded malformed-capacity path retains its advice handling.

For that bounded outcome, duplicate raw retry fields and header/error containers
are retained while collecting candidates. Valid nested advice takes precedence
over envelope advice, including an expired zero. The selected tier supplies one
header set to the shared retry parser, which selects a deadline and captures its
clock origin once. It grants neither HTTP identity nor capacity-loop authority.

Established WebSocket error frames can supply IDs in outer `headers` or nested
`error.headers`. A valid outer `x-request-id`, then `x-oai-request-id`, takes
precedence over nested equivalents; `cf-ray` falls back independently. IDs must
be 1 to 128 ASCII letters, digits, hyphens or underscores. The mapper retains
only validated nested correlation values; nested authorization and other recovery
headers do not enter the returned HTTP metadata. Status, original body and the
already captured provider deadline retain their existing meaning.

Selected terminal WebSocket provider failures can also use safe IDs from the
received `error` text frame, or from
`response.error.headers` in a `response.failed` frame. Top-level `headers` takes
the same precedence over
nested headers described above. This fallback retains only the two validated
tokens for that request. Each received text frame replaces the candidate,
including frames with absent or invalid IDs; a new request starts empty.
The eligible family is `InvalidRequest`, `InvalidPrompt`, `InvalidAudioInput`,
`ContextWindowExceeded`, `QuotaExceeded`, `UsageNotIncluded`,
`SessionBudgetExceeded`, `AccessProgramNotEnabled`, `ModelNotFound`,
`InferenceHookDenied`, `ContentPolicyViolation`, `CyberPolicy`, `BioPolicy`,
`MisalignmentPolicyViolation` and `FlexUnavailable`. Model and content-policy
errors must have no HTTP status. Completion and every other error type stay
outside this fallback, including incomplete responses and errors with retry-advice
slots even when advice is absent. Wrapped HTTP errors continue to use the metadata
retained on their transport error.

Outer correlation is decoded independently of optional nested error/response
metadata. Unusable nested shapes omit nested IDs while preserving valid outer
IDs. A positional error array supplies no named nested headers, even when the
separate `response.failed` parser accepts its semantic fields. This logging rule
does not admit standalone arrays or validate provider errors. Borrowed raw views
remain within metadata extraction; only the two validated tokens enter state.

The observer does not retain frame bodies or arbitrary headers, change error
values or retry decisions, or parse provider deadlines. It forwards caller
telemetry under the caller's existing context. Correlation identifies the
selected received frame; it does not resolve ownership of a late provider frame.

Counts describe the prepared endpoint JSON, after preparation and
bounding. They do not measure changes a custom auth or transport implementation
makes afterward. An incremental WebSocket request can contain only a delta; its counts
do not measure the full conversation. Counts do not validate call/result pairing
or establish that history is malformed. These are JSON item counts and byte
lengths, not token usage. A shape record can precede a socket acquisition
failure and does not prove the backend received the request. Handshake errors
before payload preparation have no shape record.

**Omission means unmeasured or unavailable; zero means measured empty.** Array
input supplies counts, including zero for an empty array. Other representations
omit item counts. Typed HTTP and WebSocket requests measure retained encoded
tool bytes but omit a present tool list's unmeasured count. An absent typed tool
list reports both metrics as zero. Generic JSON requests count a tool array but
omit its unmeasured encoded size; other present tool representations omit both.
An absent generic tool field reports both as zero.

Correlation accepts nonempty ASCII letters, digits, underscore and hyphen, up
to 128 bytes. For repeated HTTP fields, each emitter selects the first safe whole
value per header name; an invalid earlier value cannot hide a later usable ID.
All `x-request-id` values are considered before `x-oai-request-id`; if no primary
is safe, the alternate can supply its first safe value. The selector never trims
values, splits them at commas or truncates them into another identifier. Invalid
or absent IDs are omitted. JSON headers retain their existing case-collision
normalization.
SSE failures can retain IDs from the successful opening response, but that
opening HTTP 200 is not reported as the later stream error's HTTP status.
IDs help backend correlation; they do not diagnose why a request was rejected.

There is no request-failure event when a deferred SSE error is superseded by
completion, or when WebSocket cancellation cleanup preserves a received
completion. These metadata events omit request bodies, tool schemas, arbitrary
response headers and raw provider error messages. Their inherited built-in
request context is retained; this is not a privacy guarantee about all logs or
arbitrary fields an embedding caller adds to its own spans.

Core's retry handler adds `Opaque backend rejection` under
`codex_core::responses_retry::alab_capacity_retry` for every normalized opaque
400 it sees, including the one that spends the retry budget. It follows that
attempt's request-failure row in the same process and turn span.

| Rejection field | Interpretation |
| --- | --- |
| `transport` | `https` or `websocket`: the transport the session used when the handler ran, which is where the rejection arrived unless another request changed it meanwhile. A `websocket` row is a rejected handshake. |
| `replay` | `unchanged`: the same request again. `without_routing_state`: the turn's routing token, if it held one, was dropped first. `handshake`: core retries the handshake or, with the budget spent or on a socket the session had just returned to, moves the turn to HTTPS. `none`: the budget is spent and the turn ends. |
| `turn_state` | `kept`, `dropped`, `absent` (the turn held no token) or `not_sent` (a handshake carries none). |
| `https_rejections` | Opaque rejections of this request over HTTPS, this one included; `0` on a handshake row. |
| `retries` | Retried stream errors of any kind this request had spent on its current transport. |

The first HTTPS rejection of a request is replayed unchanged and a later one
without the token ([review](reviews/bad-request-https-replay-2026-11-16.md)).
`turn_state=absent` on a rejection means the token did not cause it. A
rejection the unchanged replay clears was transient. One that persists until
the `without_routing_state` replay is consistent with the token, and is not
proof: that replay also comes one backoff later.

The model client adds two rows about the transport itself, under
`codex_core::client::alab_transport`. `Responses requests moved to HTTPS`
carries `websocket_return`: `scheduled` with `after_secs`, `server_refused`
(no failed request preceded the fallback: HTTP 426, kept for the session) or
`exhausted` (four returns in a row did not last). `Responses requests return to
the WebSocket after HTTPS fallback` carries `on_https_secs` and `turn_state`:
`dropped` when the turn held a routing token, which the returning request does
not send, or `absent`. Request-shape rows between the two have
`transport=responses_http`. A `moved` row seconds after a `return` row is a
returned socket that did not carry its first request, which was then replayed
over HTTPS.

Logs are bounded evidence: the queue can drop entries under pressure, writes can
fail, and per-partition retention removes old rows beyond a 1,000-row or
approximately 10 MiB budget. Independent whole-database cleanup is scheduled at
startup and every 30 minutes. It removes rows older than 10 days, then halves
that age window while occupied database pages exceed 64 MiB, down to a one-second
minimum window. Even a small, quiet partition can therefore lose old evidence.

Inserts normally batch on a 10-second interval or 512 entries. A missing partner
can reflect buffering, retention, an interrupted process, an older package, or
successful completion. Preserve the
available process, attempt, thread, turn and response IDs before interpreting
an opaque `Bad Request`; those records alone do not justify rewriting history.
