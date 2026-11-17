<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve buffered SSE pacing on ordinary tail failure — 2026-11-16

Decision: enhanced-mode ordinary SSE stream termination retains the exact advice
already captured on the latest buffered generic Retryable failure. Preserve the
selected timeout/transport message. Copy the existing RetryAfter; do not parse
headers, sample clocks, restart advice or deliver the buffered error early.

Before this change, response.failed with an unknown code and valid advice stayed
buffered for completion. EOF delivered that Retryable and its deadline, whereas
idle timeout or ordinary transport failure delivered plain Stream and lost the
same deadline. Recovery could therefore retry sooner depending only on how the
body ended. Keep the selected terminal cause and preserve its pacing.

Owned capacity glue adds a narrow conversion: active selected Stream plus latest
pending Retryable with Some advice becomes Retryable with the selected Stream
message and that copied advice. Current active typed capacity/request/policy
failures already deliver immediately. No other selected or pending type changes.
In particular Transport(Policy) remains authoritative, and a newer unadvised
failure cannot inherit older advice. Completion and EOF keep their current paths;
inactive clients and missing/invalid advice keep ordinary Stream termination.
Zero/expired advice stays Some, without granting any new retry authority.

Two marked insertions in upstream's SSE consumer call the existing owned module
before the original telemetry and channel send. There are no upstream deletions
or formatting changes. The core/protocol representation remains Other/Stream;
the API error now has an advice-bearing Retryable slot. Local diagnostics thus
report retryable and retry_advised=true for this selected outcome, without
logging either deferred provider text or transport content. The guide records
this distinction. No new input, task replay, actor policy or history rewrite.

Initial tests-only source49e30bad33ca33c2017d16a2ad807a44c9e7b878 and first
implementation7d751cbf827a8fbc969e78b1daceeb86aa015b80 exposed a test-oracle
mistake: public assertions demanded exact virtual timer instants, but Tokio
rounded idle/network observation by two/one milliseconds. Those public failures
occurred before the advice checks, so they do not establish the intended RED;
first GREEN also failed and is not accepted. Preserve their original raw logs,
controllers and native directories. Correct the premature progress/reach claim.

The timing-only correction allows observation at or after the nominal 1-second
idle / 250-millisecond transport fault, within 10 milliseconds of timer ticks.
Exact original receipt deadlines remain unchanged. Corrected RED source
9ce84c2c5a2d86db2cadcbd1c64d0a4019cc49f6 runs the old production path:
492 of495 outer methods pass, three fail and none are ignored/measured/filtered.
Both public first7-second rows now fail at the late missing-advice assertion,
after selected fault text, elapsed bounds, ordinary protocol/noHTTP mapping,
channel close, body drop, one custom transport send, safe IDs/local attempt and
privacy checks. Zero rows and exact retained-deadline/diagnostic assertions are
GREEN-only. The owned900-second case fails its required Retryable/Some pattern;
its later explicit clock/deadline/channel-close checks were not reached.
Nested exact-child failures are not additional outer methods.

Merge both corrected regression histories into the product topic, then remove
only the temporary RED2 branch. Preserve peer80d42a7e513f1bc1ee108735a945b5e40fc9f789
and its three sync paths exactly before qualified verification. The new tests
are byte-identical between corrected RED and fixed composition; production still
matches the reviewed25-line helper and two hooks.

Qualified fixed source047fee9c7c1ee3b4f07c87615a8c60b1344f628b passes 87 public
contracts, 271 API units and 137 HTTP-client units: 495 passed, zero failed,
ignored, measured or filtered. All four new methods and the owned fallback
boundary method pass exactly once. Scoped all-target Tippy reports zero owned,
spanning, failed-target, missing-script or set-aside findings. Independent source
and native artifact reviews retain the exact phase attribution and current
receipt/snapshot evidence. Only this ADR changes after qualified execution.

Coverage: buffered unknown failure followed by idle or delayed network failure,
7-second and zero advice, EOF, completion, policy denial, absent/invalid advice,
a newer unadvised failure and inactive behavior. The policy control injects a
typed denied tail, not a real permission revocation. Fixtures use paused time
and an in-memory HTTP200/SSE transport; no physical TCP, actual Core retry,
Autopilot execution or new explicit thread/turn-span claim is made. The existing
marker case retains its separate span assertions. Native project frontend,
source-selected Trust15698 and native unit runner use the explicit unverified
runtime lane; no application certificate, package or installation is claimed.
The recorded application-build prerequisite and original opaque400 backend cause
remain unchanged. Private evidence lives in the shared Git directory under
alab-validation/sse-buffered-advice-20261116.
