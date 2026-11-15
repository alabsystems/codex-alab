<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Exact-child output capture — 2026-11-14

Decision: share one test-only child runner between the owned API and SQLite
contract suites. Drain both real OS pipes to EOF while bounding retained output,
preserve timeout evidence, and require the final parsed libtest result plus
successful process exit. A retention cap must not stop pipe reads and turn a
noisy passing child into a false timeout.

Final source `41e1b4540eafd83a8059ed4331d55461f043fdbe` passed the complete
450-test API library closure and scoped all-target lint. The five state
contracts and their scoped lint passed at
`a4d99c0f6e10cf2d841c21e25faed1ab07e70f2b`; their source inputs are unchanged
by the final API-only regression deadline refinement. These are test-tool
results, with no application certification, package or install claim.

## Reproduced defect

Previous helpers used `AsyncReadExt::take(64 KiB)` before reading child stdout
and stderr. At the cap, reading stopped before real EOF. A child producing
more output could block on its remaining write before exiting or printing its
libtest summary.

RED source `cfe994a8ad2bcfb8c992abbd18fe071ee598e468` reproduced this in
native execution `65735`, exit 101. A small real-child positive control passed
before the 4 MiB-per-pipe case. That noisy case timed out after 30.02 seconds:
both captures reported 65,536 bytes, no summary and `truncated=false`; cleanup
retained signal-9 status with `cleanup_error=None`. The parent summary was
`0 passed; 1 failed; 0 ignored; 0 measured; 52 filtered out`. This establishes
a test-harness defect rather than a production/backend failure.

## Shared collector and success gate

`codex-rs/alab-core/glue/test_child_process.rs` is compiled only through
`cfg(test)`/path modules in the API and state-contract crates. Production,
dependencies, lockfile and compiler pins are unchanged.

The runner drains stdout and stderr concurrently with waiting for the child,
without a read cap. Each pipe retains a 32 KiB prefix and 32 KiB rolling tail,
while separately tracking total bytes, omitted output, true EOF and read errors.
An 8 KiB buffer is reused, with a cooperative yield every 16 successful reads
so other pipes and deadline polling continue. Retained raw-byte limits are
distinct from bytes drained or the size of formatted UTF-8 diagnostics.

Stdout has a separate 4,096-byte line scanner. Only complete column-zero
`test result: ` candidates are parsed. The last anchored candidate replaces
the prior result: malformed or oversized candidates clear stale success.
Counts and decimal duration syntax are validated. Stderr cannot supply the
success summary.

`Report::assert_test_success` requires successful exit, error-free real EOF on
both pipes, and a parsed `ok` result with exactly one passed and zero failed,
ignored and measured tests. An unmatched `--exact` filter is rejected even
though libtest exits zero. This gate validates trusted test output; it does not
authenticate arbitrary subprocess text.

## Timeout and adapter custody

Capture accumulators and optional exit status live outside the main timeout.
Reads commit evidence before another cancellation point. On internal timeout
or I/O failure, the runner requests kill unless the child is already reaped,
then uses one five-second deadline for final wait and unfinished-pipe drainage.
It does not reread failed or completed pipes. Failures retain PID/status,
partial prefix/tail/counts, EOF/error state and any cleanup error; incomplete
cleanup cannot pass the success gate.

External cancellation is narrower: dropping the entire runner can only request
`kill_on_drop`, with no asynchronous reap promise. Adapters await the runner's
own deadline. This change does not establish process-group/descendant custody
or alter application child management.

Eight additional API modules/ten parent paths joined the buffered representative
on the shared asynchronous runner: nine modules and eleven adapter call sites,
excluding the regression driver. Existing 30/60-second bounds, child environment
conventions and paused-clock behavior are retained. Previously unbounded
context/handshake parents now use 60 seconds and the internal HTTP-retry parent
uses 30 seconds. The state adapter retains its 30-second bound and detects
child mode before runtime creation, preserving child-owned runtime destruction
and activation behavior.

## Real-process controls

The regression first runs a 1 KiB-per-pipe child to establish startup, exact
selection, markers and summary parsing. Independent stdout/stderr writers then
produce 4 MiB per pipe. Assertions check full byte counts, bounded retention,
both prefix/tail markers and the real libtest summary after process reaping;
normal parent assertions do not dump the payloads.

A parked child flushes partial markers and never returns to libtest. Its
two-second timeout must kill/reap it, drain both EOFs, preserve the markers,
report unsuccessful status without a summary, and finish cleanup without error.
Separate real children write an earlier valid-looking summary followed by a
malformed or oversized anchored candidate and exit zero; both must clear stale
success and fail the gate. A real absent-filter child must also be rejected.
Expected panic-hook suppression is isolated to the gate-controls child and
restored before further assertions or awaits.

The tiny nested malformed/oversized/zero-selection controls use five-second
inner deadlines under the gate-controls child's 30-second deadline. This leaves
room for the runner's five-second cleanup before the enclosing deadline can
cancel it. All these controls passed in the final API closure.

## Actual validation

API execution `47856` exited zero at final source
`41e1b4540eafd83a8059ed4331d55461f043fdbe`. State execution `36207` exited
zero at `a4d99c0f6e10cf2d841c21e25faed1ab07e70f2b`:

| Test binary | Source | Passed | Runtime |
| --- | --- | ---: | ---: |
| Public API contracts | `41e1b4540e` | 53 | 5.30 s |
| API units | `41e1b4540e` | 260 | 2.01 s |
| HTTP-client units | `41e1b4540e` | 137 | 2.11 s |
| Full SQLite state contracts | `a4d99c0f6e` | 5 | 0.30 s |

All binaries reported zero failed, ignored, measured and filtered tests. Both
scoped all-target Tippy classifiers have empty `owned`, `spanning`, `failing`
and `scripts` lists, `set_aside=0`, and successful build-finished records. All
673 API and 809 state artifact paths are unique within their run and lie under
their separate canonical `target/tippy` and `state-contracts/tippy` children.

The retained state-source composition records that only
`codex-rs/alab-api-tests/src/child_capture_tests.rs` differs between the state
test source and final code source. That driver is not a state-contract
dependency; the common runner, state bridge, dependency sources, manifests,
lockfile and compiler pin are byte-identical. State evidence is retained for
those unchanged inputs without claiming a new state execution at final source.

Evidence is retained in the root repository's `.git/alab-validation/`:

| Archive | Path below `.git/alab-validation/` |
| --- | --- |
| RED | `cfe994a8ad2bcfb8c992abbd18fe071ee598e468/child-capture-red/` |
| Final API tests/lint | `41e1b4540eafd83a8059ed4331d55461f043fdbe/child-capture/` |
| State tests/lint/composition | `a4d99c0f6e10cf2d841c21e25faed1ab07e70f2b/child-capture/` |

The RED exit marker contains `101`; final API and state markers contain `0`.
The final ownership inventory is complete: 86 hook topics and 13 features;
semantic qualification remains separate.

| Artifact | SHA-256 |
| --- | --- |
| RED `checks.log` | `1f47d5dbc856e6d8bc10e25f9aa851befb150269fed7db7fa381f987ece34a30` |
| RED `source.diff` | `8e673805d8abe7d52cb86ca0039863faa061a6f25847d1409b19f51b2fc9f7f5` |
| Final API `checks.log` | `c312d2acc4b1705ef966da0839bcccf7ae12bb18cc1f8d1c1dac66b690b2aef9` |
| Final API `lint.json` | `09ac0e436e217eed5f35e50508a7af98d247ca25f08b14c49337ac7fa26926a4` |
| Final API `source.diff` | `d82742e34dbbf77efc23e1472227ae6414a4a9c77ca65a296f03996b281fa990` |
| State `state-checks.log` | `de37051ac0a7355d61bb3ada144d115e4187073b06268a6639eb2f3913973af0` |
| State `state-lint.json` | `41eecc1d349201860bad7e43ecf67f0c4d2e13effd7b1757107882060d0c033f` |
| Both empty classifier verdicts | `7f390f2bfe1312792d12d567794e90b8e44e1c16eebb0e6678be1d29ffa58300` |
| State source composition | `53b2d55f6e89cc9ef30bcac4e1213575be37b2b0206d9f3e773f693ea07c9215` |
| Shared helper | `5b4dcd42ca23f1c99a827a10e7a3703180c442bfb8c8f2142539b9e40e8cee81` |
| Final real-child regression | `5eb9eaccd8d3d0794c2535b1a084dfb437a4cabbada23d438c579aa06fbd101a` |
| State adapter | `063ff7cf44f75edaec233ef1b422f6915551ddb85fdb595be532487c0407e72f` |

Source and archived results were independently checked against these counts,
verdicts, artifact targets and hashes. They establish test child capture and
evidence retention, not native credential authority or application certification.
The existing application qualification blocker is unchanged. Publication
identity and receipt belong in the separate private validation ledger.
