<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Selected WebSocket audio refusal correlation — 2026-11-16

## Problem and decision

A selected `response.failed` audio-validation error could contain usable request
IDs yet omit them from the request-failure log. The existing WebSocket observer
captured safe outer or nested correlation, but the selected-result fallback in
ALab-owned `api_request_diagnostics.rs` admitted only `InvalidRequest` and
`InvalidPrompt`. `InvalidAudioInput` therefore lost correlation despite already
having its own safe `invalid_audio_input` failure category.

Include the selected audio variant in that existing fallback and update the
incident guide. This is one match alternative in owned glue. Error parsing,
returned values, recording scope, retry policy, transport priority and clocks
retain their existing behavior. No new dependency or upstream-owned source edit
is required.

## Reproduction and verification

On tests-only source `f0a4cbe259db25e4ec721755f120e0ee47ca7ab6`, the two new real
public WebSocket methods failed at the missing `request_id` assertion. Outer
results were zero passed, two failed and 97 filtered; ignored and measured were
zero, with actual unit/controller status 101. The two isolated child failures
remain separate from the outer count.

Both children first established the original audio error message and
`per_recording` value, mapped message and terminal/no-advice/no-fallback policy,
channel closure and joined fixture, request/turn context, failure category and
privacy. Static markers immediately before the ID assertions were observed.
Both then saw `None` instead of the available nested request ID. Other matrix
rows and the following `cf-ray` assertion were unreached in this failing phase.

On fixed source `d641a6adc5b5e03854d8bbbf5024cd4ded857148`, both complete libraries
passed: `codex-alab-api-tests` 99 and `codex-api` 281, totaling 380. Failed,
ignored, measured and filtered counts were zero. Scoped all-target Tippy passed
with zero owned, spanning, failing, missing-script and set-aside findings. Unit,
lint and outer controller statuses were zero.

The public test file is byte-identical across reproduction and fixed execution.
Its two methods enumerate 15 object-frame rows each: 14 audio refusals and one
completion control. They cover `invalid_audio` with recording scope true and
`audio_too_large` with scope false, outer precedence/aliases, malformed headers,
unsafe IDs, stale-frame replacement and completion. Positional-error coverage
remains in its existing request/prompt tests. The existing direct selection and
HTTP-priority control and all-transport typed audio/category/policy test also
passed. The new public assertions inspect the original audio boolean before the
bridge; they do not separately inspect the mapped boolean at runtime.

## Evidence and limits

The prescribed native project frontend selected committed Trust 15698. Execution
used the maintained `unit_tests --record-results` path in the explicit unverified
library-test lane, one record round and one test thread. Reproduction selected
exactly two full test identities in one public library; fixed verification used
both complete libraries. Lint used canonical `target/tippy` and both normal/test
library profiles, with 657 canonical artifact paths observed.

Live capture retained native compiler digests, prepared/current file identities,
launch paths, working directories, execution receipts and diagnostic directory
identities: one library plus the native runner for reproduction, two libraries
plus the runner after the fix. Source and target stayed frozen through capture.
Historical executable paths are not rehashed after rebuilds. These are harness
and custody observations, not a complete wire-frame inventory. No additional
manifest-relative runtime fixture files were found in the selected source scan.

This verifies the two selected API libraries with local fixtures, not a full
Core actor, TUI, packaged-binary smoke or every feature context. No application
binary was certified, packaged, installed or adopted. The unavailable
application-certification prerequisite and existing holds remain unchanged.
The original opaque HTTP 400 backend cause remains unknown; this reproduction
establishes the separate cause of missing audio diagnostic IDs. Detailed native
evidence and independent reviews remain in the private
`ws-audio-error-ids-20261116` validation stage.
