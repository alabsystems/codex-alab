<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve runtime stops when evidence retention fails

At source `fdbcce2890dbb95428ddc00c3c8d200d866f7e0b`, the public runtime
controller could replace a selected child failure with exit 1 if diagnostic
copying or its final record write raised `OSError`. A real owned dispatch fixture
confirmed the consequence: a failed fake probe exiting 75 normally returned 75
and retained its record; placing an owned directory at `output/smoke/result.json`
made actual `shutil.copyfile` fail with `EISDIR`, changed the public exit to 1 and
prevented the top-level record. The maintained check's `stopped`/`passed` contract
propagates 75 and signal statuses, so retention lost meaningful stop authority.
Evidence: `~/.codex/.ubd/runtime-retention-repro-asvasisd/`; source and fixture
hashes were unchanged across these actual public CLI runs. The fixture was inert
and all its temporary repositories/processes were owned and cleaned up.

Decision: catch only ordinary retention `OSError` at the diagnostic-copy and
record-publication boundaries. Keep the original nonzero child status, including
raw signals until the existing CLI normalization. A copy failure sets `passed`
false, adds a separate phase-specific diagnostic error without overwriting the
original error or child statuses, and still attempts the top-level record. A
previously successful operation fails with exit 1. Both failures use the existing
CLI failure reporter so an ordinary diagnostic-output error cannot replace the
selected status. No new CLI classifier or general exception framework is added;
real cancellation and other uncaught exceptions still propagate.

Record publication owns a named temporary in the same output directory, writes
and closes it, then uses `os.link` to publish the fresh `result.json` name. The
link is the commit point: serialization, write and close must succeed before a
record becomes visible there, and no existing file, symlink or directory is
replaced. Only the owned temporary is removed. An `OSError` unlinking that temporary
after publication is advisory cleanup of a valid committed record; it does not
invalidate or erase the record, or change a successful command into failure.
Before publication, an ordinary cleanup I/O error likewise cannot replace a selected write error.
This is completed-write publication, not a new fsync/power-loss durability claim.

Five new regression methods use the real public Python controller and existing
owned dispatch fixtures. Actual copy-destination and record-target directory
obstructions cover exit 23, resource status 75 and an owned fake child sending
SIGTERM only to itself; the existing normalization produces 143 for that raw
signal. Healthy controls retain ordinary success and stop records. Successful
runtime/history fixtures followed by retention failure return 1 and cannot
publish a passing result. A separate public-main child injects `EIO` only after
real record bytes are written and flushed, records their byte count and parsed
`passed` value, then verifies that failed publication leaves no target or owned
temporary and cannot mask a prior 75. This controlled write-error seam does not
claim actual disk exhaustion. The unchanged history-retention and source/binary
binding tests remain in the selected checks.

The final regression bytes against old production produced ten expected assertion
failures across five methods, with no errors or skips. After the production fix,
all 94 unique methods in six selected modules passed in 9.399396666995017 seconds:
`test_check_runtime`, `test_reset_smoke_cli_status`, `test_cli_status_binding`,
`test_reset_notice_oracle`, `weekly_reset_test` and `test_cli_status_callbacks`.
There were no failures, errors, skips, expected failures or unexpected successes.
All 240 Python-source hashes and HEAD were identical before and after execution;
only `alab/check_runtime.py` changed between the baseline and candidate source
snapshots. The final test bytes are identical. Independent parsing matched all
94 log identities to the selection without duplicates. AST parsing and scoped
`git diff --check` passed. The first candidate invocation stopped during external
runner setup because it named a nonexistent test module; it executed no test
bodies, and its separate error note is retained. Correcting only that external
selection produced the completed candidate run above.

Retained evidence is `~/.codex/.ubd/runtime-retention-fix-cjkf25tw/`, including
`baseline.*`, `candidate-final.*` and `final-source-audit.json`. The candidate log
SHA-256 is `c3ca1b11a1a422dda841df8ff31528acba7ae7a3c7a8cb70fddcd3faf0dceaa2`.
These are local Python/owned-fixture checks: no actual Codex application, native
guardian, Rust compiler/tests, certified gate, live quota credit or release
publication was exercised. Native qualification retains its separate CPU-custody
status-75 and unavailable qualified-publisher blockers recorded by root.
