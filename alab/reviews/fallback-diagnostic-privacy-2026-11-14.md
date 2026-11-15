<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep raw provider text out of the owned fallback warning

The fallback adapter logs `diagnostic = %warning.message` at WARN. The real
retry handler constructs that warning from the complete error, so an accepted
WebSocket error message can enter default local logs through this field. For
example, `previous_response_not_found` retains the provider's message as a
retryable stream error; retry exhaustion selects HTTPS fallback and reaches
this log. The content-free API diagnostics do not prevent this separate path.

## Decision and reason

Remove the raw diagnostic field from this ALab-owned warning. Retain the static
fallback message and inherited context. Preserve the original diagnostic in
the user-facing retry event's `additional_details`, along with status, provider
deadlines, retry-count reset and transport selection. This removes an unnecessary
default persistence path without reducing the evidence available to the caller
or changing recovery policy.

Extend the existing real-handler contract with a structured tracing capture.
Require one actual fallback warning as a positive recorder control and reject
its raw diagnostic field and private sentinel. Keep the existing assertions
that the model client selects HTTP, retries reset, the same turn owns the event,
and the original diagnostic remains observable to the caller. Limit the privacy
assertion to this owned log event; this is not a claim about every upstream log
or caller-provided tracing field.

## Validation

Committed RED source `3679c1c0156bbbc704e35ffe2a84f4ec95aa1898` fails the
expected raw-field assertion in the isolated real-handler child, after the
original recovery/details checks and one-warning recorder control pass. Its
parent and child output are retained with source-keyed validation evidence.

Fixed source `9448700a47db021dbda45c749dba3499cd8ee495` passes all three focused
fallback contracts (the isolated child entry is ignored in the parent suite).
The real handler switches to HTTP, resets retries, retains the same-turn caller
diagnostic and passes the live warning privacy assertions. Provider pacing and
inactive/unrelated behavior controls also pass. Source-selected Trust 15698
executes these in the explicit unverified test lane; runtime is 0.20 seconds.

Core all-target Tippy completes with no ALab findings; eight upstream warnings
are set aside by the repository classifier. Committed ownership validates all
86 hook topics across 13 features. An independent source review accepts the
actual production change and structured capture. The production delta removes
one log field; upstream code, dependencies and compiler pin are unchanged.
Exact-source logs, hashes and classification are retained in the common Git
directory's `alab-validation` evidence.

Certified application delivery remains separately deferred at the recorded
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
