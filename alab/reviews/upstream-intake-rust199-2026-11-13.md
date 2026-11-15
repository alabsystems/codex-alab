# Rust 1.99.1 intake extension

The fresh upstream observation is `f941a5fb927b429e2575cf32da8b27472255683a`,
96 commits beyond integrated `ce3031018ef130e653ed4d9c5bb971652de97161`.
The existing `1f175fa3c4af-intake.json` already reviews the first 21. Extend
that batch through the next commit, `caf0d30005bc8487c9505d0d09fcd1d2e600660d`,
against ALab base `cc2400b644f27e451de2d2dbf0fe31e56286c6da`.
Copy those 21 decisions without changing their contents, and supersede the
old batch in the same commit. Only the 22nd decision is new. The remaining
74 upstream commits are outside this review; no upstream merge is performed.

## Decision: adopt

The commit moves the stock Rust pin from 1.95.0 to 1.99.1 consistently across
rust-toolchain.toml, Bazel toolchain configuration, Windows setup, and CI and
release jobs. All 204 changed scalar leaves in MODULE.bazel.lock belong to
old/new Rust-version artifact facts; there are no changed facts outside
those two versions and no changed Cargo.lock entries. Artifact digests were
read as committed metadata, not independently downloaded or certified.

The Rust changes are compiler/lint adaptations, reviewed across all 45
paths:

- Atomic `fetch_update` calls become `try_update` with the same ordering,
  closures and checked/saturating bounds. No counter semantics are relaxed.
- Fixed-width decoding and rendering use `as_chunks`. UTF-16, opaque Windows
  paths, digest parsing and audio playback retain their preceding length
  checks; complete-group iteration and finite-sample checks remain.
- Realtime buffered strings use `mem::take` instead of draining to a new
  String, clearing the same fields. The halfwidth-mark snapshot changes its
  debug spelling while retaining the same Unicode characters and styles.
- Match/destructuring, optional-value propagation, closure borrowing, and
  test assertions are simplified. The manual exec-policy error conversion
  constructs the same variant. MCP startup send errors are boxed without
  changing the event or its delivery result.
- The two `double_must_use` allowances are scoped to generated tonic service
  modules. They do not disable application-wide diagnostics.

Seven touched files contain ALab hooks (agent identity, app-server dispatch,
realtime auth transport, exec recovery, TUI lifecycle tests, list selection
and pager rendering). The changed hunks do not remove those hooks or replace
their responsibilities. Keep them when resolving the earlier pending TUI and
execution changes; this review does not assert that the whole 22-commit range
merges without conflicts.

## Qualification remains separate

Adopt upstream's source/toolchain declaration without substituting stock
Rust for the source-selected Trust compiler. Run native project selection,
the certified application build and the ordinary ALab gate on the integrated
candidate. Reconcile any compiler-lock change through the native controller;
do not hand-edit trust.lock or treat a newer stock pin as proof authority.
The existing procedural-macro certification refusal is not resolved by this
commit.

The affected checks include full ALab lint/tests, native exec input and
handoff scenarios, account credential/digest parsing, skill-resource reads,
Windows opaque-path and UTF-16 cases, TUI pager/palette/halfwidth snapshots,
and the unchanged native permission-policy refusal assertions. These are
required future candidate checks, not executions claimed by this review.

Validation here covers the 22-commit intake snapshot, exact preservation of
the prior 21 decisions, and absence of duplicate active review coverage.
