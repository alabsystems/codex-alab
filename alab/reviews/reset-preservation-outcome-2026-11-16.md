<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve resets without reporting a verification failure

A valid preflight at 99–100% usage with a weekly refresh within 48 hours printed
the reset inventory and its preservation reason, then returned `Recovery::Stop`.
The generic Stop fallback consequently printed an identity/ledger failure too.
This made a successful policy decision appear to be failed recovery.

Use a separate `Recovery::Preserved` outcome after the existing ledger and
custody checks. It grants no cached clearance, model retry or redemption. The
read must still match the bound owner, state ordinary permission, have no
explicit spend restriction, and contain an ordinary window at least 99% used.
Every recognized ordinary window must have finite, nonnegative usage; a valid
five-hour value cannot hide malformed weekly data. Authentication is checked
again after the awaited inventory read, and a pending operation cannot take
this outcome. The earlier quiet 95–98% outcome remains separate.

The HTTP regressions cover 99/100% with one-hour/exactly-48-hour refreshes:
inventory, both expiry dates and the preservation reason remain visible, with
no generic failure, clearance, POST or repeated unchanged notices. Negative
cases cover a malformed weekly value beside a valid five-hour value and an
account change while inventory is pending. A read-only review confirmed the
guards, exhaustive match and async test borrow scopes.

Scoped `trustfmt` and `git diff --check` pass. Rust execution remains deferred:
the native CPU authority reports a changed slot device identity and the
admission controller refuses execution. The prior run and corrected fixtures
are recorded in `reset-decision-notices-2026-11-16.md`; this change does not
claim a passing rerun or a new installed binary.

The agent restart did complete. The current conversation's saved turn context
confirms `gpt-6.1-sol` and `ultra`, including collaboration settings. Native
cache retirement/cleanup and package GC found nothing currently eligible to
reclaim; the fresh shared test cache remains protected by its age/use policy.
