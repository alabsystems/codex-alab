<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Retain semantic API types in failure diagnostics

Eighteen of the 26 outer `ApiError` variants collapse to `failure_kind=other`
although the selected error still has its quota, capacity, validation or policy
type. Static type matching can retain this evidence without inspecting private
provider text or changing recovery.

## Decision and reason

Match the outer API enum exhaustively, as the transport enum already is. Add
fixed snake-case labels for the eighteen formerly collapsed variants; preserve
all existing labels and every status/header/opaque tuple. Borrow the existing
error and return only static labels. Do not construct, format or clone an error,
parse a body, add fields, or alter advice, retries, stream selection or context.
Future upstream enum additions require an explicit category decision.

The category describes the type at the existing observer. HTTP refusals and
explicit-status WebSocket wrappers remain `http`; later bridge classification is
separate. Flattened stream errors remain `stream`. Optional semantic status fields
keep their previous omission, and opening HTTP200 is not an error status. Generic
`Api` and untyped `RateLimit` have no observed Responses stream constructor;
exhaustive coverage does not establish public wire reachability. No new record
is invented for errors before request preparation or outside this observer.

## Reproduction and validation

RED source `535b6d865139da7200b0bbfe0101d9f9733a6c31` passes the existing-label and successful
completion controls. Terminal, advised and deferred-EOF groups fail only at their
final category comparison (`other` versus quota, rate-limit and content-filter
labels). All cases in each terminal/advised group run their original typed-value,
protocol mapping, retry, deadline, context, ID and privacy assertions first.

The new child test module reuses the existing owned gated transport, Registry
capture, tail probe and bounded exact-child launcher. Sixteen newly distinguished semantic types run
through the real public Responses endpoint and SSE consumer; selected content
filter at EOF and later successful completion exercise separate outcomes. A test
fixture EOF tail is the only transport extension. The audio privacy check now
excludes private sentinels and payload fields rather than rejecting a provider
code substring that can legitimately overlap a fixed enum label.

Independent review confirms all 26 variants are explicit and the eighteen new
arms add no parsing, formatting, allocations or policy changes. Fixed-source
runtime checks at `12af4481f88d8de51427862f7ceb60f84d0b320c` pass 34 public API
tests and 260 API units (294 total). Both crates pass all-target scoped Tippy
with zero ALab diagnostics. Exact-source logs, diff and hashes remain in common
Git validation. Runtime RED/final use the same two API packages; Tippy uses the
gate's existing separate cache helper. All lint artifact paths were verified
under `target/tippy`. This required check is not a controlled performance benchmark.

The prior full Python workflow sweep passed 1,003 tests with eight skips; no
Python workflow implementation or tests, native adapter or dependency/pin changes here.
Application delivery remains deferred at the independently confirmed
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md). These are
source/runtime checks, not application certification or an installed update.
