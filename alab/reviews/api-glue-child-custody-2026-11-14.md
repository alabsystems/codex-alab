<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# API glue child custody — 2026-11-14

Decision: migrate five legacy `codex-api --lib` child parents to the existing
owned bounded runner. This is test custody maintenance: those parents used
unbounded synchronous `Command::output`, while the earlier noisy-child RED
established the separate capped-reader backpressure defect. No new RED or
production diagnostics/recovery defect is claimed for this migration.

Validated source `f3b785bb4ed07970ab80fd163e83acb0293f4e59` passed standalone
`codex-api --lib`, the full three-package API library closure and scoped
all-target lint in native execution `54390`, exit zero. The standalone check
exercised the crate's explicit test features without requiring the broader
public-contract package closure.

## Five parents, unchanged semantic children

Paths below are in `codex-rs/alab-core/glue/`; these tests belong to
`codex-api`, including the recovery bridge test loaded through `api_bridge`.

| Parent | Adapter | Budget |
| --- | --- | --- |
| `api_request_recovery.rs::http_bridge_retries_only_the_opaque_400_and_keeps_server_advice` | Synchronous guard, parent current-thread runtime | 30 s |
| `api_request_diagnostics.rs::diagnostics_correlate_shape_and_failure_without_content_or_arbitrary_headers` | Synchronous guard, parent current-thread runtime | 30 s |
| `api_request_diagnostics.rs::real_http_endpoints_emit_correlated_diagnostics_only_when_active` | Existing Tokio parent awaits runner | 30 s |
| `api_sse_diagnostics_tests.rs::http_sse_semantic_failures_retain_attempt_ids_and_captured_context` | Existing Tokio parent awaits runner | 30 s |
| `websocket_request_diagnostics_tests.rs::websocket_diagnostics_follow_the_selected_terminal_result` | Existing Tokio parent awaits runner | 60 s |

For both synchronous tests, child detection precedes creation of the parent
runtime. The original child keeps its synchronous activation, subscriber and
semantic assertions. The three async tests construct no nested runtime. Exact
module-derived names, `--exact`/`--nocapture`, `CHILD="1"`, environment
inheritance and child-return convention are retained; no new test-thread flag
is introduced. Global/thread-local tracing isolation, active/inactive cases,
selected/superseded errors, original context and privacy assertions remain in
their original child bodies.

The budgets replace absence of a process deadline. HTTP parents use the
existing 30-second convention; 60 seconds matches real WebSocket parent
conventions and accommodates four serial lifecycle cases. Existing inner
phase/deadline/clock behavior is unchanged. No semantic guard is relaxed.

## Shared runner and thin test surface

Every parent awaits `test_child_process::run(command, budget)` and applies
`Report::assert_test_success`. The runner drains both real EOFs while retaining
32 KiB prefix plus 32 KiB tail per pipe. Successful status and the last parsed
one-pass/zero-failure summary replace local substring checks, including the
bridge's former weaker `contains("1 passed")` gate. Internal timeout/I/O
failure retains partial evidence and owns bounded five-second kill/drain/reap.
External cancellation only requests `kill_on_drop`; no asynchronous reap or
descendant custody promise is added by these adapters.

The six-file change consists of four owned test-parent files, a marked
`cfg(test)`/path hook at `codex-api/src/lib.rs:138–140`, and explicit Tokio dev
features at `codex-api/Cargo.toml:40`. The existing dependency now declares
`io-util`, `process` and `test-util` for standalone API tests. It does not rely
on `codex-alab-api-tests` unifying those features in the combined closure.
Production behavior/features, dependency versions, lockfile and compiler pins
are unchanged. Shared collector code is unchanged.

The retained audit identifies other parents with environment-clear/COMPLETE
markers, WebSocket-client ownership or contained executable environments.
Those are separate scopes; this batch does not claim their migration or
validation. Existing real noisy/held/malformed-summary/zero-selection controls
remain the shared runner's earlier evidence, not a new RED for these parents.

## Actual validation and custody

At the same source, execution `54390` reported:

| Check | Passed | Runtime |
| --- | ---: | ---: |
| Standalone API units | 260 | 1.93 s |
| Full closure: public API contracts | 53 | 5.30 s |
| Full closure: API units | 260 | 1.97 s |
| Full closure: HTTP-client units | 137 | 2.10 s |

The full closure has 450 passing tests; the standalone 260-unit check is a
separate repeated package check. Every binary reported zero failed, ignored,
measured and filtered tests. Scoped all-target Tippy exited zero with empty
`owned`, `spanning`, `failing` and `scripts` lists, `set_aside=0`, and a
successful build-finished record. All 673 compiler artifact paths are unique
and within the canonical `target/tippy` child.

Complete evidence is retained in the root repository at
`.git/alab-validation/f3b785bb4ed07970ab80fd163e83acb0293f4e59/api-glue-child-custody/`.
`checks.exit` contains `0`; the ownership inventory is complete with 86 hook
topics and 13 features. Semantic qualification remains separate. Audit and
migration notes are retained at
`.git/alab-validation/api-glue-child-next-20261114/`.

| Artifact | SHA-256 |
| --- | --- |
| `checks.log` | `d40dce9998c12e9165e5b714f540dfb08dda0ac08e9f995bc00437235dd86911` |
| `lint.json` | `9fdb3f29371b5e70904766071eb715d21c0574ad07964837f08fa83aae0b9c6f` |
| `verdict.json` | `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300` |
| `source.diff` | `d63ab0ecd45305c10f920c1a56a97ee9a4ca0aca0094d4e542e3a4d900a11380` |
| `inventory.txt` | `16aad1ea0a08aff573b08a715c3e9f9d0bbc534245394adb5bc2db10f747750d` |
| `remaining-glue-child-audit.md` | `8f76966b20a6da2c8ef00b57d1b6b8e93403f742d6f9526f5a2ea1743b23d017` |
| `api-glue-child-migration-notes.md` | `ef53bc6d0b712d8f1789ad98a3a0a9cec503aa41214591fe9610ba729669376f` |
| Test-hook source | `0c2d65d15384194c869dc36b895a553b2a578e744377b2cf1913911bf7a7ef5a` |
| Crate manifest | `e891fba59c13237a5795ac0169b743fc7a8bf5a1b48b776ee47ebfa1d2d8edb3` |
| Unchanged shared helper | `5b4dcd42ca23f1c99a827a10e7a3703180c442bfb8c8f2142539b9e40e8cee81` |

Source and retained results were checked against these counts, verdicts,
artifact targets and hashes. Test subprocess custody establishes no native
credential admission, application certification, package or install claim;
the existing application qualification blocker is unchanged. Publication
identity and receipt belong in the separate private validation ledger.
