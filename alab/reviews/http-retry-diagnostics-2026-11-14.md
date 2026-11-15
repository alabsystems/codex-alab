<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Correlate logical requests across internal HTTP retries

The diagnostic guide defines `local_attempt` as one prepared endpoint request.
Internal HTTP retries can send that request more than once. Existing diagnostic
fixtures disable those retries, so they do not prove the relationship between
the request-shape event, per-send telemetry and the selected final outcome.

## Decision and reason

Exercise the public Responses client with its actual retry pipeline, a scripted
transport and a real tracing Registry. Observe the encoded bodies delivered to
the transport and the existing request telemetry independently of diagnostic
events. Cover an initial 503 followed by an opaque 400, an initial 503 followed
by successful SSE completion, and a later endpoint call.

Require unchanged payloads across sends, one shape per logical request, final
failure correlation from the selected response, no failure for a retry that
eventually succeeds, and a new local attempt for the later call. Preserve the
original error and exclude request content, arbitrary headers and provider
messages from the diagnostic events. This adds a cross-layer contract without
copying retry policy or changing production behavior.

The completed tool pair is seeded request history. This fixture proves encoded
history preservation across transport sends; native core regressions retain
responsibility for actual tool execution and avoiding repeated effects.

## Validation and delivery boundary

All six API contract tests pass with source-selected Trust 15698 in the explicit
unverified test lane, including the retry contract, original SSE/WS context
regressions and three real TCP fixture controls. The final run compiled in
1m25s and executed in 0.11s. Focused all-target Tippy for `codex-alab-api-tests`
passed in 2m10s; `alab/lint.jq` reports no owned, hook-spanning, failed-target,
missing-script or upstream findings. Committed feature ownership validates.
Independent review found no concrete issue.

The native test and lint admissions waited 8m52s and 4m19s respectively. These
are observed queue and compilation times, not a controlled performance claim.
The Cargo manifest, dependency lockfile, compiler pin and production sources
are unchanged. Exact checked source, logs, hashes and review evidence are
retained under `.git/alab-validation/0faaad72cf1b04c5d554ced418f9e7aabcd722b4/`.

This work builds test targets only. Certified application delivery remains
deferred at the selected Targo procedural-macro graph admission boundary;
these tests make no installation or application qualification claim.
