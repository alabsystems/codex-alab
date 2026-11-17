<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retained backend FORK

The private retained host now executes the original complete `fork_thread`
operation through the existing thread processor. It starts no child turn. The
original client path keeps its response, subscription, history and notification
behavior; the backend obtains the same typed result without a frontend ACK.
Arguments, schema and result are shared with the TUI.

Self-fork uses the immutable issuing turn supplied by Core's actual prepared MCP
call. The existing public metadata hook matches the exact configured server,
transport and tool, and uses a weak binding reference. An opaque call claim only
correlates Source data: the actual runtime binding still authorizes the call.
It supplies no Native role, currentness or effects receipt.

Acceptance and shutdown share one registry mutex. Exact replays join the same
worker/result; changed arguments and other bindings are rejected. Only pending
claims expire. Accepted and uncertain records remain through the real binding
lifecycle, bounded by fail-closed capacity. Removed entries are destroyed after
the registry lock is released.

The host owns one whole worker with a pre-spawn guard. Core entry and exact birth
are retained outside the future; capture happens before fallible post-birth
processing. Unknown poll/drop outcomes cannot be recovered by child termination.
A definite returned error may stop only its captured newborn through the existing
Source shutdown helper. This accounting is not Native effects closure.

Five actual HTTP/Core/model controls cover label-only rejection, a genuine
self-fork whose observer is lost before a later issuing-thread turn and C1/C2
replacement, another active paginated source, explicit legacy history, and a
real paginated store with no database. They use actual observed approval IDs and
Core-produced claims. The unsupported store retains the original concrete error
and creates no child; legacy success does not pretend to exercise an old remote
server's unsupported-method fallback. Production fallback policy remains shared
with the original TUI.

Validation: 34 actual app-server task controls, 12 TUI dynamic-tool tests and
31 existing retained-host controls pass on the recorded base plus exact working
patch. After removing two unused TUI bindings left by schema extraction, the
12 TUI tests pass again and production/test-library Tippy has no owned, spanning,
failed-target or missing-script findings. Independent read-only review finds no
concrete custody, admission, replay, cancellation or cleanup blocker.

The component uses source-selected Trust15698 in the protected explicit
unverified runtime lane. Checks retain their actual dirty-input identities;
committing the checked blobs does not turn them into a clean-commit build or an
application certificate. Ordinary startup, retained Native frontier activation,
certified adoption and the full live-update goal remain unfinished.
