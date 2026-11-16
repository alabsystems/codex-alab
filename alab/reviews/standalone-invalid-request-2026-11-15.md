<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve standalone streamed request refusals — 2026-11-15

Decision: use the existing ALab-owned standalone-event hook for the exact
`invalid_request` code, sharing the validated response.failed parser. The branch
for a request refusal returns only a validated InvalidRequest. Do not classify from a type
or message, promote malformed data, or change opaque HTTP 400 recovery.

The prior repair handled response.failed. The standalone type:error envelope
was still ignored for this specific code: an SSE idle timeout or later completion
could replace its meaning, and a statusless WebSocket frame could become a generic
close error. That is an adjacent demonstrated client defect, not an explanation
of the original server's opaque rejection.

Only the owned responses_capacity helper changes in production. Exact active
request-refusal codes enter the shared parser after existing terminal-provider
handling. InvalidRequest returns before nested or envelope Retry-After fallback.
Malformed decoded fields return None; unknown, type-only and inactive events keep
their old ignored-event path. Existing capacity variants and nested-then-envelope
advice precedence remain intact. No upstream file, new hook, interface, dependency
or Autopilot source change is needed.

WebSocket wrapped-HTTP and special-code handling runs before the shared event
parser. Valid 400 and 503 wrappers therefore retain their transport route, raw
body and provider advice. The statusless case in the WebSocket regression
exercises the new branch. Existing full API tests retain the other socket/capacity/policy controls.

The tests-only source eaf0ccdc45bb438ae7f39f4e574137d413855e5f reproduces two
failures, with 320 other methods passing and none ignored: standalone SSE yields
idle-timeout Stream, and real statusless WebSocket yields a server-close Stream.
The raw RED child report retains completed 400 and 503 case markers after their
status/body/advice/channel/fixture assertions. Negative and inactive standalone
controls pass. Nested child failure summaries are not extra outer test methods.

At 8c9926782dfbfffe7fe46573d225b7164cce9e40, all 62 public contracts and
260 API units pass: 322 total, zero failed/ignored/measured/filtered. All four
new exact test identities pass once. Scoped all-target Tippy has zero owned,
spanning, failed-target or missing-script findings and zero diagnostics set aside.
Both packages remain selected in RED and GREEN, preserving the Hyper feature graph.

Remote main advances to 4399414a10d258f39ea33e8b480099447fb02c10 with the 0.10
version metadata and restored certified publication gate. Compose it without
changing its decisions. Because version metadata can affect compilation, repeat
both API libraries and scoped Tippy at a43c4982a66dc14a177d05b19f812db0d276410f:
322 pass again, zero ignored or filtered, and all lint classifications remain empty.
The version/publication changes are peers' work and are preserved as landed.

Publication then encounters a forward main advance and stops without writing.
Preserve that attempt and compose remote 7a63031cc848389c8dbf8f89eb2cae5be99dd3cb,
which changes the native results collector's directory cleanup. The runtime tests
for both API packages and scoped Tippy pass again at
f797daa4003a743d47b975e8e59143c415d52a79: 322 passed, none ignored or filtered,
and every lint classification empty. This exercises the changed collector;
no previous run is relabeled as a check of the later composition. Publication
continues through the same expected-main Agh route, without changing source guards.

SSE tests use the public Responses endpoint with an observed custom byte-stream
transport. Pending, contradictory completion and EOF cases verify exact messages,
blank/null/absent fallbacks, body drop, channel closure, no tail consumption for
specific refusals, and no mapped retry advice despite nested seven-second and
outer eleven-second advice. The safe diagnostic is correlated once and excludes
payloads and arbitrary headers. Malformed fields, near/unknown codes and inactive
mode retain the original pending/EOF/completion behavior.

The new real loopback WebSocket test checks one public stream request and observes
its response.create payload for each of 400, 503 and statusless cases. It verifies
routing, exact message/body, bounded advice observation and cleanup. All three
exchanges must finish before the child completion marker and strict successful
OS/harness/EOF report. Successful child per-case prints are captured rather than
relayed to the outer log; the GREEN case coverage follows that source loop and
completion gate, not separately retained per-case markers. The fixture observes
the first real request; it is not a complete wire-frame counter. This test adds
no direct WebSocket diagnostic capture assertion; existing diagnostic contracts
and the new SSE assertions have their own scope.

Activation stays in exact-name child processes, with inactive parents and controls.
Source-selected Trust 15698 runs through the existing qualified project frontend,
protected profile and native unit runner in the explicit unverified test lane.
Tippy uses the maintained separate cache and ownership classifier. Independent
read-only reviews check the source, RED/GREEN oracles, native artifacts and scope.

Private evidence is retained under shared Git
alab-validation/standalone-invalid-request-20261115: exact sources/controllers,
raw logs, native attribution, lint records and publication/cleanup observations.
The earlier Autopilot full/family and managed 0.9 runtime records retain their own
source identities; they are not relabeled as new runs. No application package is
built or installed by this topic, and no application certificate is claimed.
The previously recorded compiler-certification limitation remains separate; the
restored publication gate is not bypassed. The original backend reason remains
unknown.
