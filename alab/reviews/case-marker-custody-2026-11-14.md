<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Case-marker child custody, 2026-11-14

Decision: observe configured completion literals independently of retained child
output, then move three API case-parent helpers onto the shared bounded runner.
This is expected-GREEN test-evidence maintenance. The audited guard-skipping and
retention risks establish no new production defect or runtime RED.

Qualified code source: `fdb0db4bb540b06c8f8e6b86126d89b370d456e8`, based on
`05990d292b004deb90344f2a3fb370690bffd2e6`. Independent collector, caller and
real-child driver reviews accepted the actual source.

The shared `test_child_process.rs` keeps `run(command, timeout)` compatible by
delegating with no marker expectation. Its optional marked entry validates a
1..=4096-byte literal before spawning. A bounded KMP needle/fallback/overlap
state observes every raw stdout chunk before retention discards bytes. Stderr
has no observer. A real middle marker can therefore survive eviction from both
retained ends, while joining a prefix and tail cannot invent matching bytes.
Diagnostics add only marker-seen and marker-length metadata; they do not render
the configured needle.

The observation supplements the existing success gate: successful OS exit, the
last complete anchored successful summary with exactly one passed and zero
failed/ignored/measured tests, and complete stdout/stderr EOFs. Marker state
lives in the owned capture outside cancellable collection futures and survives
the same bounded kill/reap/drain cleanup. A timeout or I/O failure remains a
failure even if cleanup sees the marker. The five-second cleanup ceiling and
external cancellation's limited `kill_on_drop` guarantee remain unchanged;
external cancellation promises no asynchronous reap.

All three helpers remain synchronous and construct an unpaused current-thread
runtime only around their awaited runner call. Their parent callers remain
plain `#[test]` functions. Paths below are under `codex-rs/alab-core/glue/`.

| Helper | Child ceiling | Required stdout literal |
| --- | --- | --- |
| `responses_capacity_tests.rs::isolated_capacity_case` | 30 s | `ALAB_STREAM_CAPACITY_BOUNDARY_OK:{case}\n` |
| `responses_error_quota_tests.rs::active_parser_boundary_runs_in_its_own_exact_test_process` | 30 s | `ALAB_STREAMED_QUOTA_ACTIVE_BOUNDARY_OK\n` |
| `websocket_error_headers_tests.rs::isolated` | 60 s | `ALAB_WS_HEADER_BOUNDARY_OK:{case}\n` |

Each `\n` above is an actual trailing newline, preserving the exact case suffix.
The ordered argv remains `--exact`, module-derived child name, `--nocapture`,
`--test-threads=1`. Each helper keeps `env_clear`, its existing child-variable
name and value, and only present `DYLD_LIBRARY_PATH`/`LD_LIBRARY_PATH` restores
through `var_os`. Active/inactive assertions, fallible return conventions,
case values, child activation/argv guards and semantic assertions are unchanged.
Every test attribute, paused child runtime and inner semantic deadline remains
intact; no clock is resumed. The WebSocket marker still precedes runtime Drop,
so process status and real EOFs remain necessary completion evidence.

The new real-process control writes 96 KiB before and after a middle
marker, proves its absence from both retained ends, and accepts the real libtest
footer. A marked parked child must time out, be killed/reaped/drained, and retain
its observed-marker state. A separate exact controller rejects four completed
invalid cases: no marker; a longer wrong stdout marker with the exact marker
only on stderr; fabricated `QR` adjacency across a discarded retention gap;
and exit 17 despite an observed marker and fake passing summary. Expected gate
panics stay inside that controller process. Its unmarked invocation also checks
the default `None` behavior.

Changes stay in seven ALab-owned files: the shared collector, three parents,
the public driver/registration and `alab/features.toml`. No additional upstream
hook, dependency, feature, lockfile or compiler-pin change is required. The
qualification-fixtures invariants/scenarios now describe marker observation,
cleanup and the positive/negative controls. Committed-source inventory passes
with 88 hook topics and 14 features; inventory is separate from semantic proof.

Root's completed validation ran against the qualified source:

| Execution | Selection | Passed | Runtime |
| --- | --- | --- | --- |
| `36804` | Standalone `codex-api --lib` | 260 | 1.87 s |
| `36804` | Combined public API contracts | 54 | 5.28 s |
| `36804` | Combined `codex-api --lib` | 260 | 1.86 s |
| `36804` | Combined HTTP client library | 137 | 2.10 s |
| `36804` | Combined WebSocket client library | 53 | 0.51 s |
| `11147` | State contracts | 5 | 0.26 s |

Both executions finished with exit zero; all summaries have zero failed,
ignored, measured and filtered tests. There are 509 distinct tests:
54 + 260 + 137 + 53 + 5. The standalone 260 API tests are repeated qualification,
not another 260 distinct tests. Both scoped all-target Tippy classifiers have
empty `owned`, `spanning`, `failing` and `scripts` lists with `set_aside=0`.
Build completion succeeds, and all 674 API-group and 809 state-group unique
compiler artifact paths lie under their respective canonical lint targets.

The source-selected compiler is Trust 15698, tree
`9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`.
Tests used the explicit unverified lane; scoped lint ran without verification.
Neither used a guard override.
Each lint JSON stream contains two spanless compiler notes stating that
compilation ran without Trust verification and checked zero obligations. They
are notes, with no compiler warning/error diagnostic, and establish no formal
proof. These results qualify fixture evidence and custody; they make no native
accepted-actor, application certification, package or installed-update claim.

The complete archive is retained in the root repository at
`.git/alab-validation/fdb0db4bb540b06c8f8e6b86126d89b370d456e8/case-marker-custody/`.
All file hashes and lengths were checked against its `record.json`.

| Archived artifact | SHA-256 |
| --- | --- |
| `record.json` | `302293c1d2a48a95d0969f5b7758506e9e7c8bf41bb932060c7f54aa722a7190` |
| `api-checks.log` | `8b90bba2bc72bc732ef246a310efd7514e60d29283eae2a3c60ce85e442c9fd2` |
| `state-checks.log` | `7217b6648a0b1792df55956bccfd786bfa430279916045646d8b3ccd524ab889` |
| `api-lint.json` | `9b68cfb2b1e90d826259bc4470419d7b89d1954a5002e0fcc16498a4ad397310` |
| `state-lint.json` | `6d3ea842efdfe6370b43f97a5b7ca5653c0eaa44874190cf4c1e7e911961c007` |
| `api-verdict.json`, `state-verdict.json` | `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300` |
| `inventory.txt` | `82ff23e15084ae5b34ea469a336f37b5df5b633912b35fd724e9d4990c7a8269` |
| `source.diff` | `2001498c6de3ccc088ef08084c3d4ddad1c93c17015e2e3659ab74967f20c36e` |

Publication identity and receipt remain in the private validation ledger.
