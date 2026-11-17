<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Real HTTP retry wire contract — 2026-11-17

Existing tests already cover compressed prepared-request reuse through a mocked
transport, and real first-wire rejection bodies, advice and diagnostics. They
did not combine an executed HTTP retry, final compressed wire framing and
endpoint correlation through the real Reqwest transport.

Decision: add GREEN-only integration coverage, with one strict isolated outer
method running plain then Zstd rows. Each fully drains a bounded POST before an
immediate complete 503 with `Retry-After: 1`, then a second POST before successful
HTTP200 SSE creation and completion. No production retry or request logic changes.

Both actual request bodies and normalized header multimaps must match across
the retry. Content-Length equals actual bytes; Transfer-Encoding is absent.
Plain bytes equal the original JSON. Independent bounded Zstd decoding must
recover that same JSON, including the supplied completed function-call/output
pair. This preserves history; it does not execute a tool or prove effect safety.

The first request-completion callback observes the actual typed advice deadline
and must retain at least 500ms of its one-second interval. A cheap synchronous
`apply_auth` observation records retry initiation before real Reqwest dispatch.
Its Tokio instant must reach the captured deadline, and physical monotonic
elapsed time must reach the conservative remaining interval sampled after its
physical anchor. Server read time is not the pacing oracle or exact packet
arrival. There are no paused clocks, explicit sleeps, body gates or artificial
response delays; the production retry loop performs the advised wait.

Each recovered logical call has two original request-telemetry observations and
one endpoint request-shape record, with no selected endpoint failure. Assertions
retain the original input counts, request bytes, compression and controlled
thread/turn context while excluding private body, tool, input and header values.
Event target, level and field names are checked before formatting. Headers,
wire bodies and decoded bodies are bounded; network phases and server join have
five-second budgets. Client assertion failures abort and await the owned server;
remaining assertions follow its joined completion.

The test uses direct Reqwest source policy and an exact `zstd = "=0.13.3"` test
dependency already present in the lock. Only the ALab test package's dependency
list changes; all 1,498 other package blocks remain byte-identical.

Runtime source `535eb86beb527fc41adb5b42fadfb30997b2c9d2` passes all 390 outer
methods: 107 public fixtures and 283 API library methods, with zero failed,
ignored, measured or filtered. Wrapper, summary and capture statuses are zero,
and scoped all-target Tippy reports zero findings. Runtime capture SHA256:
`80af0aae5d0b5855559207a2558b486fcfdd126d6691f7711ec53fa244216e35`.
Independent qualification is accepted with SHA256
`0e5d36da68554cb8004da41adbb99a8515c9850fd93c0494d69dd6393d1143b6`.

Successful child raw output is not retained in the parent result. Reach of both
declared rows follows the passing parent marker contract and source flow, not a
separate per-row transcript. Rows are not additional outer tests. This decision
record is the only change in this topic after that runtime source.

This adds no production defect fix or RED claim and does not explain the
original opaque HTTP400. It qualifies neither an application nor a provider,
host sleep, exact scheduling latency or global privacy. No package or running
binary is adopted; existing application-certification prerequisites and human
holds remain.
