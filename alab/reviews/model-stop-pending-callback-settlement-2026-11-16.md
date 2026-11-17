<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Settle terminal goal callbacks without losing mutation custody

During composition of exact goal-mutation custody with current main, the real
Code Mode/native overlap fixture intermittently returned `goal status is held
until ambiguous usage can be reconciled`. A token callback can advance the
accounting-data generation after SQLite dispatch but before the successful
mutation owner publishes its transition. Treating that generation mismatch as
an unknown mutation outcome is incorrect: the durable status write succeeded,
the write/failure witnesses are unchanged, and only same-goal callback data is
new.

Decision: a dispatched successful mutation may publish its transition from the
original accounting witness when the only newer state is healthy ownerless data.
The resulting effect is non-destructive: it may acknowledge the unchanged goal
association, but it cannot clear pending usage. Ordinary admission remains Held
until an additive write persists that data; `permits_write` stays true. The
post-commit effect predicate remains strict, so failed, ambiguous, retired,
clock-bearing, different-goal, or actively owned state cannot use this path.
A successful exact mutation may also supersede its captured recoverable failure
state when the failure generation is unchanged, including
`settled_mutation_unknown`; a post-dispatch callback must not turn that recovery
back into ambiguity.

Terminal turn accounting previously used `ActiveOnly`. That refuses retained
late tokens once the exact mutation has already made the row Blocked, Paused, or
Complete. Decision: only when Core reports the exact healthy ownerless-pending
data state, acquire the native goal permit, verify the retained and persisted
goal IDs, select the existing status-specific accounting mode, and delegate to
the existing additive owner/durability path. A replacement or missing goal is
quarantined. Every other state keeps the old ActiveOnly behavior; in particular,
known failed or ambiguous additive work is never widened or replayed.
Both normal turn stop and turn abort use this finalizer. Keeping abort on the
old ActiveOnly path would strand exactly the late callback this change recovers.

The four-thread stress run exposed two distinct test-only deadlocks. Blocking
direct B inside `PreToolUse` shell commands can consume hook-command capacity.
Moving that wait to `on_tool_start` was also incorrect: the callback runs after
Core has admitted B, and in stress round five B's admission prevented A from
reaching its handler even though nested A had already been observed. The goal
row remained Active with only the first 10-token callback, and A never created
its real `PostToolUse` marker. Decision: gate the mock provider response itself.
It emits B only after A's real PostToolUse marker exists and the scenario has
finished its intermediate assertions and explicitly released B. This consumes
no hook subprocess and admits no B side effects early. The actual PostToolUse
commands, SQL writes, Code Mode host, call-ID collision, lifecycle outcomes,
and callback-order assertions remain real.

The failure variants accept an accounted delta of either 10 or 30. Both are
valid scheduler orders: the first callback can drain before terminal dispatch,
after which a later provider response belongs to the already-settled stop and
is intentionally uncharged; or both callbacks can arrive after dispatch and be
retained for same-goal settlement. Requiring 30 would contradict that boundary.

Evidence after the fixture-gate correction:

- Core passes all 410 tests, including the new settled-unknown/post-dispatch
  regression, in 37.34s.
- All 96 model-blocked recovery tests pass in 40.93s.
- The ten dispatch/identity native cases pass once and then 100/100 across ten
  four-thread stress rounds. The formerly hanging combined direct-failure and
  nested-abort case passes in 2.59s by itself.
- A deterministic lower-level regression begins an already-dispatched exact
  mutation, adds a 23-token callback, and proves the terminal finalizer persists
  exactly 23 tokens and returns Ready for Blocked, Paused, and Complete rows.
- A hostile read-only review found the settled-unknown recovery edge; its fix is
  covered by the new Core regression. Further hostile review found the abort
  hook omission and approved the status-aware fix after both terminal paths used
  it. Separate fixture forensics rejected invocation-order `on_tool_start`
  gating and approved the provider-boundary gate.

The merged-tree gate ran against source-only commit
`3dca72107f53792b3da7bd4625c8f2cd5db18c3d`. Its executable tree is identical
to reachable topic commit `c5a6cf2cfbe198fdf93d3511c25f5401c4e7bcaa`,
which differs only by appending this evidence record:

- The qualified `atpkg-publish` controller was built from aterm commit
  `c0a183cc1c15d9de9f4b1a76b8d53d82f992fc9c`; its SHA-256 is
  `1a9021c41cb7d370c6e4179edd2c72528c56fa2a5e0829855b58ed1c0d6bc4f1`.
  Its controller preflight succeeded and selected Trust 15698, tree
  `9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`,
  from the same immutable store path for payload and execution.
- The first merged-tree run exposed `expect_used` diagnostics in two ALab
  tests and Clippy's method-reference diagnostic in the supervisor. The
  correction changes only test panic spelling and the equivalent method-call
  form. Focused all-target Tippy then passed, as did the two affected goal
  recovery cases and the affected policy continuation case (3/3).
- The protected rerun completed its authoritative lint stage with nothing in
  ALab's code and 44 upstream diagnostics set aside. It then failed closed with
  exit 2 at the certified CLI build, before the gate's later runtime and unit
  stages. Trust refused the `codex-cli` unit because its in-process proc-macro
  dependencies share compiler arguments, memory, stderr, and the artifact
  filesystem, outside the current no-proc-macro evidence TCB boundary. The
  compiler consequently emitted neither authenticated `TRUST_JSON` rows nor a
  coverage summary; the report was INCONCLUSIVE with zero obligations
  published. No allow-gap flag, unverified substitution, or other proof
  weakening was used.

Thus the source, focused lint, and runtime evidence above are green, while
`alab/check.sh` remains deliberately non-green at the unchanged Trust
proc-macro certification boundary. A future gate can resume when Trust can
authenticate that boundary; this change does not bypass it.

After composing the topic with authoritative `origin/main`
`33eb7462bd464c4fd27587d09337cdf5113202e1`, protected gate source
`413af81c9691f58e431304f7fa137add4e014b1e` repeated the same result. Tippy
found nothing in ALab's code and set aside 44 upstream diagnostics; the
certified CLI build then exited 2 at the identical in-process proc-macro and
missing authenticated `TRUST_JSON`/coverage-summary boundary, before later
gate stages. The subsequent remote advance to
`673c454e5d61f6f574a598103b08dde79cb183ff` changes only publication-status
Python, tests, and `publish/pre-promote`; it does not touch `codex-rs/`,
`alab/check.sh`, or this topic. The recreated final merge therefore has the
same Rust executable and gate-script trees that run checked.
