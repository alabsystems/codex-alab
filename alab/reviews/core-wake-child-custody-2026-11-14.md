<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Core wake child custody — 2026-11-14

Decision: move the nine ordinary-retry wake parents onto the existing bounded
marked child runner. Qualified implementation source
`c38fc5ca6e8f61e9fee13e37eece33c0dd5bca59`, based on
`37fa88284f8d8ff54da08b4f54c5bdadd6b08f96`, passed the recorded focused Core
selection and affected scoped lint. This is test-harness custody maintenance;
no new behavioral RED or production defect is claimed.

The adapter in `codex-rs/alab-core/glue/responses_retry_wake_tests.rs` registers
the shared module locally inside existing test-only owned code. It directly
awaits Tokio `Command` custody from a new unpaused current-thread parent runtime,
requires literal `ALAB_ORDINARY_RETRY_WAKE_OK:{case}\n` on stdout, and applies
`Report::assert_test_success`. The configured literal ends in an actual newline.
Successful exit, one passed test with zero failed/ignored/measured tests and
both error-free real pipe EOFs remain required. No enclosing timeout cancels
the collector's asynchronous cleanup.

The 90-second internal custody deadline leaves startup and runtime-destruction
headroom beyond the child's existing 60-second semantic hang guard. The runner
retains its separate five-second bounded kill/reap/drain cleanup. The complete
child body and attributes are unchanged: it owns its runtime, pauses only its
simulation clock, retains its inner guards, and destroys its runtime before
printing the completion marker. The private `TempDir` remains owned through
runtime creation, awaited custody and cleanup, the success gate and the final
inactive assertion.

All nine cases remain `sleep`, `no-sleep`, `old-sleep`, `no-advice`, `cancel`,
`inactive`, `connection-sleep`, `connection-inactive` and `extreme`. Exact
module-derived child selection and ordered `--exact`, child name, `--nocapture`,
`--test-threads=1` are preserved. So are `current_exe`, `env_clear`, private
HOME/CODEX_HOME/TMPDIR, mandatory byte-preserving controlled PATH,
`RUST_MIN_STACK=16777216`, null stdin and both inactive assertions. Optional
`DYLD_LIBRARY_PATH`, `LD_LIBRARY_PATH`, `CARGO_BIN_EXE_test_stdio_server`,
`CARGO_BIN_EXE_codex-code-mode-host` and `CARGO_BIN_EXE_codex_code_mode_host`
restores retain their existing byte-preserving behavior.

The implementation changes two owned files: the adapter (+15/-10) and a
four-line reasoned `allow(dead_code)` solely on the shared default `run` entry.
The initial source `0eba71fdfab1409ce2af17bd6bb92b97fca6e75e` passed the same
10 Core tests in 0.27 seconds but exposed that actual owned unused-entry warning
in its marker-only embedding. The annotation addresses that warning while
preserving active default-entry callers elsewhere. Independent review accepted
its scope; every shared-helper function body and the collector algorithm remain
byte-identical.
No upstream hook, dependency, lockfile or compiler pin changed.

## Recorded validation

Source-selected Trust 15698 ran the explicit unverified runtime-test lane with
source-matching V8, identity-bound runtime helpers and the native C unit runner
prepared. Core execution `14509` used `unit_tests --record-results` with filter
`responses_retry::alab_capacity_retry::wake_tests::` and `--test-threads=1`.
One library was recorded. The focused parser separately confirmed all 10 required
exact test names: nine parents plus the inactive shared-child guard, passing in
0.29 seconds. The record collector alone does not prove that selection.

| Scoped all-target Tippy group | Unique canonical artifacts | Verdict |
| --- | ---: | --- |
| Core | 1,382 | Owned/spanning/failing/scripts empty; 8 upstream diagnostics set aside |
| Four-package API/WebSocket group | 674 | All lists empty; none set aside |
| State contracts (execution `13372`) | 809 | All lists empty; none set aside |

The final controllers exited zero and every lint build completed successfully.
Core and API/WebSocket artifacts use canonical `target/tippy`; state uses
`state-contracts/tippy`. Committed-source feature inventory passed
with 88 hook topics and 14 features. The earlier 509 distinct runtime tests
remain evidence at their actual `fdb0db4bb540b06c8f8e6b86126d89b370d456e8`
source, subsequently landed before this base; they were not repeated or
relabeled for a function-only lint annotation.

The complete private archive is retained at
`.git/alab-validation/c38fc5ca6e8f61e9fee13e37eece33c0dd5bca59/core-wake-custody/`,
including `record.json` and `recorded-run/`. All 15 indexed artifact lengths and
hashes were checked; recorded-run retains 22 files. Initial-source evidence is
archived separately under that source's `core-wake-initial/` directory. Raw
native preparation and test receipts can contain environment details and remain
private. Only safe counts and hashes are recorded here.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `fb66ac5ab0939e4631ae337eb77a811edb3385f7cfb848ada03aa7d7e47400e4` |
| `source.diff` | `ff8eafc4489f7e8a1f8a06e18219cecf32ac049abe19748a2ad73de258ad511a` |

These are runtime-test and lint results. Application certification, installation
and native accepted-actor qualification are separate; the existing Trust 15698
macro-admission gate block is unchanged. Publication identity and receipt remain
in the private validation ledger.
