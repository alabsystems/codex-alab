# Runtime helper identity and separate build graphs

The gate previously accepted any executable Cargo artifact with the expected
binary name and `bin` kind. A binary's libtest harness has that same name and
kind. Another package or source tree can also produce that name. Require the
tested workspace's exact package ID, manifest and binary source, a non-test
profile, and one regular executable resolving inside its admitted target.
Resolve package identity through Cargo metadata rather than parsing package IDs.
Keep original helper aliases and the CLI startup check after this validation.

The metadata observation now precedes helper compilation and is also used by
the existing library-result preparation. Plain helper calls retain it only in
temporary target-local scratch, removed on success and failure together with
any proxy test home. Recorded runs retain metadata and the separate raw helper
build streams. The ALab runner still builds from the authoritative ALab tree;
the library selection, complete feature graph and actual pristine reruns remain
unchanged.

## Why the proposed build consolidation was declined

A prior gate spent 23m45s preparing helpers and libraries. That measured total
did not establish avoidable work. On `23a30cca0c2566cd35d837c663345401be926e61`,
Trust 15698's native unit graphs show that combining the native helper pair and
CLI changes every runtime root's dependency closure:

| Binary | Separate units | Combined units | Unit identities with changed features |
| --- | ---: | ---: | ---: |
| `test_stdio_server` | 924 | 955 | 66 |
| `codex-code-mode-host` | 761 | 774 | 50 |
| `codex` | 1,335 | 1,330 | 12 |

For example, the native helpers gain `rustls/prefer-post-quantum`, and the CLI
gains `tonic/router`, `server` and `transport`. Different Cargo array indices
were normalized before comparison; all 187 manifest, configuration and lock
inputs remained unchanged. These are compiler-input differences, not observed
runtime failures. Preserve the separate builds; no performance saving or binary
equivalence is claimed. Proving changed helper semantics would be a separate
task, unnecessary for this identity fix.

The graph commands used exact-source native project admission, the maintained
protected profile and `--unverified build --locked --unit-graph`. They compiled
nothing. An initial attempt correctly refused writing beneath a read-only
driver root; the corrected driver wrote into the already admitted target.

## Validation

The real gate fixtures cover valid artifacts mixed with actual same-name
harness and foreign-provider executables; wrong package, manifest, source,
profile and kind; missing and ambiguous artifacts; non-executable files,
directories and escaping paths/symlinks; candidate claims in pristine output;
and CLI validation before startup. Plain-mode failure fixtures check metadata
and proxy-home cleanup. Existing library receipts, build-directory consistency,
V8 inputs and contained comparisons remain covered.

All 84 tests in the full gate-fixture suite and the affected V8, library-result,
disk-guard and proxy-home suites passed. A private control using the old selector
at `23a30cca0c` accepted and executed helpers for seven invalid identity/profile
variants, producing seven expected regression failures and no test errors.
Shell syntax and whitespace checks also passed.

A separate native compatibility run selected all three real runtime binaries
and started the exact CLI artifact using Trust 15698, the maintained profile and
the existing qualified Rust source `9254aebdc2a00b356bf0fae46a8aca707ead88dc`.
The frozen selector's hash matches this change's selector. That is compatibility
evidence for actual Cargo output, not a new full-source qualification.

Detailed commands, logs, graph differences and receipts are retained outside
source at `$ATERM_STATE/.ubd/helper-preparation-20261112.noindex`.
Source V1 activation remains held.
