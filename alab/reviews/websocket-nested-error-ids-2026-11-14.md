<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve nested WebSocket error correlation

The accepted WebSocket error envelope already reads provider retry advice from
`error.headers`, before outer `headers`. The HTTP error mapper retains only
outer headers, so selected failure diagnostics lose nested request and ray IDs
even while preserving the original error body, status and deadline. This is an
independent evidence defect; it does not establish the original opaque-400
backend cause.

## Decision and reason

Add one marked call at the HTTP header construction point into the existing
ALab-owned error-header module. Reuse the existing JSON-to-header normalization
for value types, case normalization and case-collision ordering. Reuse the
selected-diagnostic correlation validator: nonempty, at most 128 ASCII letters,
digits, hyphens or underscores. Do not parse the body again or infer identifiers
from messages.

A valid outer primary or alternate request ID protects every existing outer
request-ID entry. Otherwise, choose nested primary then alternate and store the
selected value as canonical `x-request-id`. Resolve `cf-ray` independently.
Only validated correlation values propagate; arbitrary nested authorization,
error and retry headers remain excluded. An invalid outer correlation value can
be replaced only when no valid outer counterpart exists. Preserve absent versus
empty outer metadata when there is no insertion, and retain exact outer-only
conversion while ALab is inactive.

Leave status, original wire body, typed provider deadline, special-code
precedence, authorization and recovery classifiers unchanged. The normalization
temporary uses the same converter already used for nested retry advice; this
is bounded correlation propagation, not an allocation-free parser. The IDs are
provider metadata, not evidence of a particular backend or root cause.

## Reproduction and validation

RED source `86978f16d74864f77b0ed5dd730ef83497b70467` passes four public controls and fails two
real WebSocket tests only at their final missing diagnostic request-ID assertion:
nested primary and mixed-case nested alternate. All earlier raw-body/status,
seven-second nested advice over eleven-second outer advice, terminal invalid
prompt mapping, original thread/turn/attempt context, privacy and non-correlation
header assertions pass. The isolated children retain bounded wait, drain and
kill/reap handles.

The owned helper matrix also covers independent request/ray fallbacks, valid
outer alias protection with an invalid primary, case collisions, numeric/bool
normalization, invalid types/characters and the 128/129-byte boundary, arbitrary
nested-header exclusion, absent/empty metadata and inactive behavior.

Fixed source `658c0399689ff5f405160f842feac8f6571686dc` passes all 24 public API
contracts and 260 API units. Both crates pass all-target scoped Tippy with zero
ALab diagnostics. Two independent reviews found no required correction. The
common Git validation archive retains exact-source logs, hashes and the diff.
The full configured Python workflow sweep runs separately against preceding
clean main. Application
delivery remains blocked at the recorded source-selected
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md); these unit
checks do not qualify or install an application binary.
