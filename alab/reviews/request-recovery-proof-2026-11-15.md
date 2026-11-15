<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Completed-work evidence in the recovery smoke — 2026-11-15

Decision: require the recovery smoke's known successful plan step, rather than
accepting two identically corrupted histories. The smoke uses a fresh exec and
empty private homes. Its model emits exactly one `update_plan` call, with a
fixed call ID and raw JSON arguments; the native tool's successful wire output
is the plain string `Plan updated`.

The previous checker counted records only under the expected ID, checked the
tool name, and compared failed-request input with retry input. It accepted
missing, invalid or changed arguments and missing, failed or changed output
when both requests had the same defect. Extra tool records under another ID
also escaped its filtered counts. These are false positives in the executable
smoke's evidence, not observations of those defects in the application.

At RED source `73bd4b364ab7d164c3c57c2c891b225bb31535f1`, the module ran nine
tests: eight controls passed, and fourteen subtests failed only because the
expected rejection was absent. Seven argument/output corruptions each passed
incorrectly in recovery and terminal-refusal modes. Unit exit was one.
The initial collector expected ten top-level tests and stopped after saving the
actual log; corrected observation retained the nine-test result without a rerun.
Three archive files were indexed; record SHA-256:
`6195816cf87d9e650b4c0f71b88cb6e6d29e153ad8cd34361befa49d6f911ab6`.

At the second RED source `6658e9f26c5a0be9cb03694a12b9ef0396d0cf9c`, the
module ran eleven tests: ten controls passed, and six subtests failed only at
the expected rejection assertion. An extra call, output or pair under another
ID was accepted in both modes. The first repair's argument/output controls and
the valid reasoning-item control passed. Unit exit was one. Three archive files
were indexed; record SHA-256:
`7ccc29983378a4ac32a804d62d2715a86c1c7457e39ff84dc2825c644d45432f`.

The final checker requires exactly the emitted function call and output, their
expected IDs, exact raw argument string and successful output. Native history
retains the emitted argument string, so semantic JSON equivalence would weaken
this fixture's preservation contract. The fixture test also decodes its actual
first SSE response and matches its call to the valid trace. Unrelated reasoning
items remain allowed. Original-input retention, call/output ordering, retry
input equality, request counts, Retry-After timing, terminal outcomes and binary
identity checks remain in force. No application retry or request policy changed.

At `eb053426dfbcdb2678e5fb3c57edf9f8366f4b1d`, all eleven recovery smoke tests
passed in 0.007 seconds, including the twenty corruption subcases and the
reasoning-item control. The two shared capacity timing controls passed in 0.009
seconds. Both commands used managed Python 3.14 with `-I -B`, exited zero, and
had no failures, errors or skips. Five archive files were indexed; record SHA-256:
`d39081bf00c3ac298d9821fc373c3c9150ab268da35e77ad77481d134d3c02cd`.
Archives reside under `.git/alab-validation/<exact-source>/`, in
`request-recovery-proof-red`, `request-recovery-tool-cardinality-red` and
`request-recovery-proof`, respectively.

A separate documentation commit, `2fe60d50ec`, corrects
[request diagnostic retention guidance](../REQUEST-DIAGNOSTICS.md).
Whole-database maintenance is scheduled at startup and every thirty minutes;
its ten-day age window shrinks under the 64 MiB occupied-page budget, independently
of per-partition limits. The source check used `state/src/runtime.rs` and
`state/src/runtime/logs_maintenance.rs`. This prose repair has no new runtime
claim, and the SQL smoke does not exercise database maintenance.

These are pure Python fixture and oracle results. No real Codex binary, live
provider, native host or application certificate was exercised. The original
backend rejection cause remains unknown. A positive packaged-binary recovery
run and installation remain deferred at the recorded certified-build prerequisite;
the smoke hardening changes no compiler, shipping gate, package or running agent.
