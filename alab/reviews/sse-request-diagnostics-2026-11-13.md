<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Correlate HTTP stream failures after successful setup

The evening bad-request fix records each outgoing request's shape and failures
opening its HTTP response. A successful HTTP response can subsequently fail
while consuming SSE. Those failures need the same request attempt and safe
server identifiers; a successful opening status must not become the stream
error's status.

## Decision and reason

Extend the existing SSE telemetry observer with a default no-op terminal-error
callback. Invoke it where the existing consumer selects an error for its output
channel: transport failure, unexpected EOF, idle timeout, or an immediately
terminal semantic error. A deferred error remains unreported until EOF; a later
successful completion supersedes it exactly as before. A selected error can
race with consumer cancellation, so the observation does not claim the channel
accepted delivery.

An ALab-owned observer retains the original request attempt, sanitized response
correlation IDs, and the caller's tracing dispatch and span. The spawned SSE
task does not inherit a thread-local subscriber automatically. Restoring the
captured context for the diagnostic preserves request/thread/turn attribution
without changing the task's ordinary telemetry context.

Forward existing telemetry callbacks unchanged. Do not proxy the stream, add
another channel or task, reparse event bodies, retain request content, or change
backpressure, cancellation, error values, ordering or retry advice. Inactive
mode returns the original observer without an allocation. Opening-response
identifiers are fallback evidence; explicit failure identifiers take priority.

## Validation and delivery boundary

All 257 API library tests passed in the source-selected Trust 15698 explicit
unverified test lane. The new cases exercise all four terminal selections, a
deferred failure delivered at EOF, successful supersession, activation isolation,
original observer forwarding and sensitive-data exclusion. A separate loopback
regression using the real tracing Registry also passed: after dropping the
caller's subscriber and both parent span handles, the delayed failure retains
the original thread/turn ancestors and safe opening-response identifiers.
Focused Tippy for the API and Autopilot crates, with all targets and no
dependency linting, also completed successfully. The maintained `alab/lint.jq`
classifier reports no owned, hook-spanning, failed-target or missing-script
findings; three upstream diagnostics are set aside.

These checks exercised the Rust sources committed as
`638f5b782a30864638d84fea1989228fcb23a752`. The subsequent merge of remote
main `a4723b9066` carries unrelated helper-turn recovery and documentation;
every SSE production and test source remains byte-identical to the tested
commit. The API fixture does not start those core helper-turn services.

The selected Targo's procedural-macro graph-admission refusal
is already recorded in
`bad-request-2026-11-13-evening.md`. This observer change adds no dependencies.
Do not repeat that unchanged failed application certification as a substitute
for the missing evidence-grade macro admission, relax the proof requirement, or publish
an unverified application. Runtime and focused lint checks for this source change are complete; no
application package was built or installed. The
[prerequisite audit](targo-macro-admission-2026-11-14.md) distinguishes this
selected-Targo refusal from native resource admission and solver execution.
