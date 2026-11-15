<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Repeated opaque backend rejection: evening incident

The aterm tab `s-eb8457efc9e7f8275433` stopped its turn at
2026-11-14 01:30:30 UTC (November 13, 17:30:30 Pacific). The affected thread is
`01a1ce34-6fad-7e12-9524-de5dae0abb9f`; turn
`01a1d7a4-5792-7391-b7c4-7e1a3bded6bc`. Its saved error and SQLite log contain
only `{"detail":"Bad Request"}`, protocol category `other`. The installed
package predates the narrow opaque-400 recovery already on remote main.

## Evidence and limits

After the last compaction at 22:43:05 UTC, retained history through the failure
contains 359 tool calls and 359 matching results. There are no missing or
duplicate call/result IDs, duplicate item IDs, or results preceding calls.
All retained function arguments parse as JSON. The last successful sample
reported 406,562 input tokens against a 475,000-token context window.

At 03:58:25 UTC the same thread/process resumed after a new user message,
without compaction or history replacement. Its first model completion succeeded
with 409,406 input tokens. This weakens persistent malformed-history and
context-limit explanations. It does not prove the rejected wire request valid.
No request trace or correlated transport diagnostic was retained, so a backend
transient and a stale connection/continuation cannot be distinguished.

The proven client faults are treating an uninformative backend envelope as a
permanent request error in the installed binary, and losing the request evidence
needed to diagnose that envelope. The backend rejection's cause is unknown.
Do not describe speculative history corruption as established, or remove history
to work around this error.

## Decision

Retain main's strict opaque-envelope classifier, bounded HTTP retries and paced
Autopilot continuation. Switch a normalized opaque rejection to full HTTPS
replay immediately rather than spending retries on the same WebSocket path.
The existing switch discards connection and continuation state, preserves
completed tool results and respects provider retry advice. Already-HTTP attempts
retain their retry cap. Specific input, policy and access refusals remain terminal.

Add local INFO request-shape diagnostics alongside existing opt-in full-payload
tracing. Record transport, full/incremental mode, encoded byte length and item
counts, followed by a correlated failure classification and safe server IDs.
Never record conversation text, encrypted reasoning, tool arguments/schemas,
authentication or arbitrary headers. Request metadata must come from the actual
bounded/serialized request, without copying or parsing its payload again.

The shape and failure events share a process-local attempt number and inherit
the existing thread/turn tracing context. HTTP sizes are before compression;
the compression setting is recorded separately. Typed tool JSON is counted
in bytes without parsing schemas. HTTP errors while opening the response and
WebSocket stream failures have explicit failure events. A later HTTP SSE error
retains its preceding shape event and existing tracing context. A rejected
WebSocket handshake has no outgoing Responses payload to summarize. Existing
`CODEX_ROLLOUT_TRACE_ROOT` tracing remains available when exact request content
is needed; these default diagnostics do not enable full-content capture.

This reduces repeated rejected transport state; it does not establish or repair
an unobserved request-generation defect on the backend.

## Validation

At `24b01cc6a552f3066e3b87ccf9b34ff7237a9abb`, after merging the current
remote main, all 33 recovery tests and all 255 API library tests passed with
source-selected Trust 15698 in the explicit unverified test lane. They include
real loopback handshake/HTTP failures, provider deadline preservation, retry
exhaustion after one completed plan tool, specific-refusal controls, and actual
HTTP endpoint diagnostic/activation/privacy checks. The 27 feature-inventory
unit tests also passed before that merge.

The first full gate found two diagnostic lint issues; both were corrected.
The second gate completed the full Tippy phase with no ALab diagnostics
(41 upstream diagnostics set aside). After memory admission, its certified
application build exited 2: selected Targo graph admission refused the existing
in-process procedural-macro dependency graph (including Serde and Clap derives),
before solver execution or proof collection. Verification consequently emitted
no authenticated coverage or active-unit proof inventory. This is the same Targo
macro-admission boundary recorded for the earlier opaque-400 fix; it is not a successful proof
or an application compilation result. The retained exact-source observation
is `.git/alab-qualification/24b01cc6a552f3066e3b87ccf9b34ff7237a9abb/run-o8fbtz74/`;
test logs and checksums are under the same source's `.git/alab-validation/`
directory. The observation confirms the same clean source and tree before and
after execution, with `qualified_source=null`. The worktree and reports remain
retained for inspection.

These are source and runtime-test results. There is no successful full-gate,
certified package, installation or running-process upgrade claim. Managed
package `2026111301` remains selected at index 136. Preserve the source-selected
toolchain and certification requirement; defer packaging until evidence-grade
admission supports the application's actual macro dependency graph. The
[prerequisite audit](targo-macro-admission-2026-11-14.md) identifies the refusing
component and the limits of this historical observation. An unverified
application build or compiler override cannot substitute for that prerequisite.
