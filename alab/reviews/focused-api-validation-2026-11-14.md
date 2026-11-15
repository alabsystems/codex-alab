<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep focused API validation caches consistent

## Decision and reason

Use the same `codex-api` plus `codex-alab-api-tests` package closure for RED and
final runtime tests, with a test-name filter during RED. Scope Tippy alone to
`alab_check_lint_target` of the ordinary runtime target, matching `alab/check.sh`.
Keep the qualified project frontend, source-selected compiler, explicit unit
lane, native resource admission and existing lint ownership classification.

The focused audio investigation alternated API-test-only RED with the combined
package closure, then ran runtime and Tippy in one target. The gate already
avoids the latter pattern: its source documents the different compiler
fingerprints and separates Tippy's target so shared host build-script units do
not overwrite each other's freshness state. The focused recipe should follow
that established layout. No new wrapper or compiler configuration is needed.

## Evidence and limits

Retained build fingerprints show a real feature difference: combined selection
uses Hyper `full/server` and Hyper-util `server`; API-test-only selection omits
them. The API crate's `wiremock` dev dependency supplies that edge. Compiler,
profile, configuration and flags match across those build fingerprints. The
feature difference propagates through Hyper TLS and Reqwest dependencies.

The already-required final run at
`89132ddea6725984b0ba05a36ea633bb1f0d08e6` retained targeted frontend diagnostics
with `CARGO_LOG=targo::core::compiler::fingerprint=info`. The runtime build
finished in 31.22 seconds, and both suites passed 289 tests (29 public contracts and 260 API units).
Its trace reports
15 newer-dependency-output
cases and one stale dependency fingerprint. The subsequent Tippy trace reports
218 `RustcChanged` reasons, including Unicode-ident, proc-macro2 and quote, plus
245 filesystem/dependency freshness reasons. Unicode-ident's recorded compiler
fingerprint changes while its features, profile and configuration remain equal.
This is Cargo's observed invalidation boundary, not evidence of a compiler
upgrade or an established explanation of the underlying identity change.

The private exact-source archive under common Git
`alab-validation/89132ddea6725984b0ba05a36ea633bb1f0d08e6/sse-audio-rejection/`
contains the completed trace and measured note. The runtime scope change explains
a dependency subset; it does not establish the cause of every earlier rebuild
or a six-minute saving. Build and check artifacts also have intentionally
different modes. A controlled cache-speed comparison was not run, and a new cold
lint cache was not built solely to produce one.

Only the focused validation instructions and this review change. The cache helper
and gate code remain unchanged. Validation is source review against that helper,
its existing gate caller and actual trace, plus `git diff --check`. Future required
runs can measure freshness in the canonical layout; cache deletion, compiler
flags, reduced memory claims and admission bypasses are not justified here.
