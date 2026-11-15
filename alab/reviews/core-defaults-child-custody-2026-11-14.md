<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Core defaults child custody — 2026-11-14

Decision: migrate the vanilla/defaults mode test's self-reexec parent to the
existing bounded child runner. Qualified source
`2595e459d1bcd741423e844115e1eb87cd3e6ff3` is based on
`d8009ae2aadc18c855209ac76242ae17cb458f34`. Focused recorded execution and
scoped all-target lint passed; a guarded focused repeat also passed at the same
source. Independent static review accepted
the two-owned-file change (+17/-15). The actual package is `codex-alab-core`,
library `codex_alab_core`, rather than upstream `codex-core`. This addresses
unbounded test custody and a loose success substring; it claims no new RED,
production defect or production defaults change.

The already test-only `alab-core/src/tests.rs` locally registers the unchanged
collector through `../glue/test_child_process.rs`. The selected test remains
plain and synchronous. Its complete CHILD presence guard and active child branch,
through the final return, are byte-identical; the existing process-global
isolation comment is also unchanged. Activation, watch notifications, vanilla
mode, background-configuration protection, binary identity and restored defaults
assertions continue in the isolated child. No child runtime, clock operation,
completion marker or extra test is introduced.

Only after the child branch has returned can the parent create its new unpaused
current-thread runtime. It launches `current_exe().unwrap()` with the exact
two-entry ordered argv `--exact`, literal
`tests::vanilla_switch_restores_upstream_defaults_and_behavior`, and inherits
the environment while overriding only `ALAB_TEST_VANILLA_CHILD=1`. There is no
`env_clear`, marker, `--nocapture`, test-thread flag or working-directory change.
Preserving inherited terminal variables also preserves packaged terminal-overlay
selection; this fixture establishes no environment isolation. The outer runner
must leave CHILD absent to exercise the parent, as the existing presence-guard
contract requires.

The parent directly awaits default unmarked `run(command, 30s)` and applies
`Report::assert_test_success` with that same literal name. Successful OS exit,
a recognized successful summary with exactly one passed and zero
failed/ignored/measured tests, and both error-free real pipe EOFs are required.
The runner bounds retained output independently of footer scanning, uses null
stdin matching the old `Command::output` default, and owns its additional
five-second bounded kill/reap/drain cleanup. No outer timeout cancels the future
and no child task is detached. External cancellation retains only the existing
`kill_on_drop` request and promises no asynchronous reap. The synchronous child
has no inner semantic hang guard requiring a longer parent budget.

The only manifest change adds `io-util` to the existing owned Tokio dev feature
list in `alab-core/Cargo.toml`. Process, macro, runtime and timer features were
already admitted; this adds the collector's missing standalone test I/O surface.
Production features remain unchanged. No upstream hook, dependency/version,
lockfile, compiler pin or shared collector algorithm changes. Standalone package
selection must exercise that admission without unrelated workspace dev-feature
unification.

## Actual validation

Source-selected Trust 15698 ran the explicit unverified runtime-test lane for
`-p codex-alab-core --lib`, selecting the one existing exact outer name
`tests::vanilla_switch_restores_upstream_defaults_and_behavior` with outer
`--test-threads=1`. Both native executions completed with exit zero:

| Execution | Exact passes | Seconds | Filtered out |
| --- | ---: | ---: | ---: |
| `67921`, initial runtime and lint | 1 | 0.01 | 282 |
| `90569`, guarded focused repeat | 1 | 0.00 | 282 |

This is one distinct test executed twice. Each run independently confirms its
exact pass line, parsed summary with zero failed/ignored/measured tests, and
one recorded library. Library attribution alone does not establish the test
result. The inner child uses the same exact name and is not another registered
guard test. Native unit-runner identity was selected for both runs; this owned
package needed no Core-specific V8 or stdio/code-mode runtime helpers.

The guarded selected shell explicitly checked and recorded
`ALAB_TEST_VANILLA_CHILD` absent before `unit_tests`, ensuring that the existing
presence guard reached the parent adapter. Ordinary native receipts record
helpers rather than the complete inherited environment; no full environment
receipt is claimed. Independent lane review checked controlled Cargo
configuration, arguments, build scripts and the native unit-runner path and
found no CHILD injection. No further repeat was required.

Initial scoped standalone all-target Tippy completed under its separate
canonical `target/tippy` target. Independent stream checks confirmed 76 unique,
existing canonical compiler artifact paths, both normal and test profiles for
`codex_alab_core`, and successful build completion. The classifier's `owned`,
`spanning`, `failing` and `scripts` lists are empty, with `set_aside=0`.
Controller, unit and lint statuses are zero. Feature inventory passed with
88 hook topics and 14 features. Source stayed unchanged between executions;
the guarded runtime repeat required no repeat of successful scoped lint.

The complete private archive is retained at
`.git/alab-validation/2595e459d1bcd741423e844115e1eb87cd3e6ff3/core-defaults-custody/`,
including `record.json`, `archive-index.json`, `recorded-run/` and
`guarded-recorded-run/`. All 40 indexed artifact lengths and hashes were
independently checked. The initial and guarded result directories contain
17 and 11 regular files respectively. Raw native/test receipts and contained
environment details remain private. Only safe counts and hashes are recorded
here.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `9ff4b2e5166a2f33da05c4c8c00ef1884a990af4a14f760dbd77b344829c6c55` |
| `source.diff` | `a55d247a3794c59a2595e09336fe90b02efd0958be38d3d8a8fca34997ab4bcb` |

Runtime tests and scoped lint qualify this fixture adapter. Application
certification, packaging, installation and native accepted-actor qualification
remain separate; the existing application macro-admission gate block is
unchanged. Publication identity and receipt belong in the private validation
ledger.
