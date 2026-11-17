<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Buffered SSE consumer cancellation — 2026-11-17

The SSE consumer already gives its closed public receiver priority over another
stream poll. Existing diagnostics fixtures covered selected errors and server
fixture cancellation, but did not verify physical response retirement when the
public consumer drops after an error has been buffered.

Decision: add expected-pass public integration coverage without changing the
consumer or retry policy. One isolated outer method runs two declared rows in
order: positive EOF selection, then consumer cancellation. Both receive the same
unknown `response.failed` followed by an exact public `Created` witness. The
witness establishes ordered processing while EOF remains withheld; the positive
row then proves the original buffered error would be selected and logged.

The new fixture mode shares the existing complete POST parser. It polls a real
peer read to Pending and signals that observation before the test drops the
whole public `ResponseStream`. Only a zero-byte read or exact ConnectionReset
can satisfy physical retirement. It never releases its EOF gate or shuts down
the server first. Peer-read and fixture-join budgets are five seconds, shorter
than the ten-second provider idle timeout and fifteen-second held-EOF budget.
The client stays alive through observation and assertions. Existing default
fixture behavior and prior context-test bytes are preserved.

The positive row retains the original Retryable message, absent advice, mapped
retryability, safe response identifiers and channel closure. Both rows leave the
caller's Registry and parent spans before releasing events, check captured
thread/turn context and exclude private payload, input and unrelated headers.
The cancellation row requires only the request-shape diagnostic after the
bounded peer-close observation and joined fixture cleanup.

Runtime source `587c2634f5e3653d096b8db2a4037e030d506367` passes all 389 outer
library methods: 106 public API fixtures and 283 API library methods, with zero
failed, ignored, measured or filtered. Native wrapper, summary and capture exit
zero; scoped all-target Tippy reports zero findings. Independent qualification
`initial-green/native-qualification-review.json` is accepted with SHA256
`14ef53194703d8d5ec424c8f39456c284d0ce713650c92888c1d1f553eff2d16`.
Its runtime capture SHA256 is
`faa10794fa5d9c00085516f57ec6481536e23aafabbec37ab2d6437288ea8702`.
This decision record is the only change in this topic after that runtime source.

The parent retains the passing wrapper result, rather than the successful
child's raw output. Two-row reach follows the strict child's completion-marker
contract and source flow, without a separately retained per-row transcript.
Those rows are not additional outer test methods.

The joined task is the loopback server fixture. The public API does not expose
a consumer-task join handle. Evidence establishes bounded physical transport
retirement and no selected failure observed through that boundary, without a
forever-no-work or global-privacy claim. This is GREEN-only coverage, not a RED
result or a production defect fix. It does not explain the original opaque
HTTP 400 or qualify the application. No package or running binary is adopted;
the application-certification prerequisite and existing human holds remain.
