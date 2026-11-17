<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Retained backend SEND

The retained private task host now consumes the original complete
`send_message_to_thread` operation. Previously its backend consumer supported
creation, reads and management while SEND still depended on the frontend.
The TUI and backend share the same argument schema, prompt limits, delegation
format and target-scoped Daybreak selection.

SEND uses the existing loaded/cold resume body and complete turn-start policy.
Loaded account, provider, override/rebuild and V2 ownership checks remain in that
body. The direct result captures the exact selected runtime before fallible
metadata work and captures a cold runtime immediately on its actual creation.
Only cold construction issues another opaque host binding; loaded targets retain
their actual namespace and owner. The resume target is boxed at the enum boundary
to keep the surrounding request target small.

The host accepts and tracks one whole SEND worker under its admission mutex.
Its work record retains runtime and turn custody outside the polled future.
HTTP cancellation only ends observation. Poll/drop loss and unresolved Core or
turn outcomes remain uncertain; runtime termination does not manufacture a
missing outcome. Definite helper errors retain captured runtime custody until
the existing Source termination observation. This is Source lifecycle accounting,
not a Native effects-closure receipt.

Three actual HTTP/Core/model controls cover loaded self-SEND, cold persistent
resume across observer cancellation and C1/C2 replacement at a required MCP
startup barrier, and configured namespace refusal. The shared fixture now performs
the real WebSocket post-initialize registration callback. The self case saves
metadata through the original name mutation. Cold teardown closes actual C1 RPC
admission before C2 initializes, and joins the original serialized close callback
after releasing startup; both real barriers are released before cleanup.

Protected Source-selected Trust 15698 checks passed: 29 task controls, 12 TUI
dynamic-tool controls, 14 Daybreak controls, one history fallback, 21 resume
controls and 31 existing retained-host controls (108 total). The incoming-main
directory-cleanup and version-ledger workflows passed 58 Python tests. Production
and test-library lint passed the repository classifier with zero owned, spanning,
failed-target or missing-script findings; 11 upstream diagnostics were set aside.

These checks used committed base `4dcc5d0ebb2cf24fa5237c23c5bf80fe304e47b8`
plus the exact 16-file working input `b9b12271e23dd38aca90186584690b2e0a1c9eb9d43160c8ee08244f24fe9012`.
The external `retained-task-send-checked` input/log records and
`source-retained-task-send-validation.json` preserve that scope. The later commit
adds those code blobs and this review; it is not relabeled as a clean-commit build.

Ordinary CLI/TUI activation, Native admission/frontier qualification and actual
new-version adoption remain unfinished. A separate native Trust 27009 adoption
of the base passed broad lint but refused certified build-script execution before
measured proof. Its generated lock remains uncommitted. These unverified component
checks do not supply an application certificate or complete the live-update goal.
