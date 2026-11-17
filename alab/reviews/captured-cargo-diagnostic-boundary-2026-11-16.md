<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep captured child diagnostics distinct from outer Cargo evidence

The read-only audit found a false-refusal boundary in `unit_results.collect` on
`b38f8251047969c83f7d347bb9d32b65192a6721`, source SHA-256
`5e8f7292c29edc41a9a098ebee610832a382a2d18ed31704ee0eeaeeef958be1`.
Public-collector controls use actual log/status files, source-shaped synthetic
comparison state and receipts, and module-local synthetic descriptors. The three false-refusal controls
leave the provided round and aggregate statuses at 101, with one explicit FAILED
copy and a final 0-passed/1-failed/0-ignored summary. No Cargo or native runner
invocation occurred in this audit.

Actual control process outcome: eight methods, three assertion failures, zero
errors/skips, exit 1 in 0.008 seconds. A plain or SGR-colored child Cargo footer
inside captured failure details is counted as an outer failing package and
refuses attribution. A quoted child signal diagnostic produces reader
`Refusal137`, although both provided Cargo statuses remain 101. Five controls
pass: ordinary failed/passing comparisons, result/summary-shaped detail examples,
and refusal of foreign-package or signal diagnostics placed after the final
harness summary. This demonstrates false rejection and signal misattribution,
not false acceptance or a measured native process death.

The present merged log does not identify which process wrote diagnostic-shaped
bytes. The native runner's receipt is emitted before exec: it binds the selected
artifact, copy, round and execution context, but does not record the harness's
kernel-observed terminal status. Matching a prepared executable or argv cannot
authenticate the writer of a quoted line. A test can also quote a complete
failure footer; treating the next or last completed-looking footer as a body
boundary could hide genuine outer diagnostics before that quote. Requiring one
summary would reject valid captured examples without supplying the missing
producer information. These limits concern the current protocol, not text
parsing in general.

Decision: retain fail-closed behavior and this concrete false-refusal evidence.
Do not ship a footer-stripping heuristic or claim the issue is fixed. The smaller
observed-test-name identity change is a separate topic. A classification fix
requires an outer-producer boundary: for example, Cargo/Targo terminal events
binding each package/target/copy/round to its actual exit or signal, on a channel
the test process cannot write, with harness text separately framed. The producer
must preserve the current exec PID, parent, process-group, signal and descriptor
context contract. Turning the runner into a fork/wait wrapper would change that
contract. The native frontend producer is outside this repository; its event
contract must be established before relaxing this reader's refusal. Genuine
outer diagnostics before receipts and after the final harness footer must remain
detectable, and malformed or truncated inputs must remain refusals.

Evidence is retained as `<evidence>/codex-parser-detail-fault-control.py`,
`<evidence>/codex-parser-detail-baseline-b38.py`,
`<evidence>/codex-parser-detail-baseline.log` and
`<evidence>/codex-parser-detail-baseline-context.json`.
The driver SHA-256 is
`fd64d87eceef87665a13cedab4e96562802a330d8612d3c2e77351c843ad9acd`;
the raw log SHA-256 is
`2a70ad4ddaccce6063f7764a82f1e94547b076c7611c8dfd3015dfebf8263ad3`.
This decision adds no production/test changes and makes no new Rust, latest-main
qualification, application-certification or native-event observation claim.

Root verifies the baseline blob, driver/log hashes and eight-method result.
The common Git directory retains 5 payloads at
`alab-validation/b38f8251047969c83f7d347bb9d32b65192a6721/captured-cargo-boundary-906ji0cy/`,
index SHA-256 `8bc3375cc3f41f44d3f5cb33031cad827656ee88ce58efc6de1b3163b55f4e49`.
All payload sizes/hashes and the exact regular-file inventory are verified.
