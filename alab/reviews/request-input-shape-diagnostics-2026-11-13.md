<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Distinguish input representations in request diagnostics

Generic JSON requests with missing or nonarray input previously logged six zero
item counts. Those counts came from a default accumulator, rather than an
observed array. This made a string, null, malformed object or missing field look
like an empty input array while investigating a backend rejection.

## Decision and reason

Record a fixed `input_kind`: `missing`, `null`, `array`, `string`, `object`,
`boolean` or `number`. Emit item counts only for actual arrays. An empty array
has six measured zeros; missing and nonarray input omit all six counters. Typed
HTTP and WebSocket input remains an array with measured counts, including zero
for an empty array.

Each array element contributes to the total and exactly one fixed category.
Unknown or malformed elements count as other items. Do not infer a message
count from a string or repeat payload serialization to measure another
representation. The static kind reveals no input content or arbitrary type
name. Request generation, transport, cancellation and recovery policy are
unchanged, and inactive diagnostics still return before inspecting input.

## Validation and delivery boundary

All 258 API library tests passed in the source-selected Trust 15698 explicit
unverified test lane. The actual HTTP endpoint fixture covers every generic
input kind, empty arrays, typed empty input, and a mixed array containing
recognized, unknown and malformed elements. It checks omission versus measured
zero, category totals, request-attempt/server-ID correlation, activation and
content exclusion. The landed WebSocket and isolated tracing regressions pass
in the same run.

Focused Tippy for all API targets passed after naming the fixture's case type;
`alab/lint.jq` reports no owned, hook-spanning, failed-target, missing-script or
upstream findings. All 258 tests also passed again with that final fixture.

Both scoped checks were repeated after merging remote main `81ec56a7b9`, on
`c832b664b2ddb57d4fcd1b7204512dc8ab94a059`: all 258 tests and focused Tippy
passed with the same empty lint verdict. The incoming session-admission changes
affect a shared library; the API crate and request-diagnostic production and
test sources remain byte-identical to `dc1a9f193c`.

Application packaging remains deferred at the procedural-macro certification
boundary recorded in `bad-request-2026-11-13-evening.md`. No application package
was built or installed.
