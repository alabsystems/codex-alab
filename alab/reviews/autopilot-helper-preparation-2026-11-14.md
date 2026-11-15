<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Autopilot runtime-helper preparation — 2026-11-14

Decision: make standalone `codex-alab-autopilot` unit-test preparation explicitly
supply its Code Mode host fixture. Qualified implementation source
`7476baefdbf49aa05e3ef56b06a66e4ebd722a77`, based on
`f09d49d05a77d3f1387dadb64eccda0491929e29`, passed tooling controls, five exact native
library cases and scoped all-target lint. These results belong to that source.
Independent static review accepted the three-owned-file change (+190/-50):
`alab/unit-tests.sh`, `alab/tests/test_gate_v8_inputs.py` and
`alab/tests/test_check_lint_step.py`.

The actual Core fixture builder optionally discovers `codex-code-mode-host`,
and Code Mode host is enabled by default. Missing discovery can fall back;
a raw standalone pass therefore does not establish prepared-host identity.
The inspected Autopilot library graph uses the Code Mode process client. V8
is needed to prepare the separate host runtime graph, rather than established
as a direct Autopilot-library compile requirement.

The maintained helper derives requirements from the complete package selection
before preparing helpers. Standalone Autopilot builds and selects only the
host. Core/app-server selections retain stdio server plus host; mixed selections
prepare that union once, independent of package order. Library selections remain
unchanged; no extra Core package or zero-selected Core suite triggers preparation.

Preparation uses the existing actual-tree/native-target V8 resolver, private
Cargo cache, guarded explicit unverified helper build and current metadata.
The unchanged `native_helpers.py` selector binds workspace, target, provider,
manifest, source, non-test profile and executable path. Both original and
underscore host aliases are bound to the selected artifact, overriding stale
inherited values without guessing a conventional path or qualifying old output.

The resolver, configuration parsing, proxy cleanup, runtime environment/layout
and metadata/selector block remain unchanged. The entire MCP CLI preparation
and record/compare tail is byte-identical, including separate ALab-owned native
unit-runner metadata/build/selection and pristine-tree configuration ownership.
No manifest, lockfile, compiler pin, application production code or proof policy
changed.

## Tooling controls

The new controls invoke the maintained shell with retained fake Cargo/V8
boundaries and the actual helper selector. They cover standalone host-only
preparation, forwarded configuration, stale aliases, both Core/app-server mixed
orderings, metadata/build/harness-profile/V8 refusal before library entry, and
standalone record mode with host plus identity-bound runner.

Against the old `unit-tests.sh`, the new standalone fixture reaches the expected
fake-Cargo refusal, status 61, for missing verified V8 inputs; the failing
unittest invocation exits 1. This is an inert preparation-wiring RED, with no
production defect or native Rust failure claimed. Candidate GateV8 controls
passed 10 tests in 15.348 seconds. Full CheckLintStepTests passed 50 tests in
410.561 seconds (execution `5006`). All 60 passed with no failures or skips.

The tooling lane reselected a genuine cached unit runner against current
metadata after checking unchanged runner/lock/compiler input bytes against
source `2595`. Its separate `tooling-runner-reuse.json` records that decision.
No earlier runtime evidence is relabeled for this source.

## Native qualification at the implementation source

Source-selected Trust 15698 explicit unverified execution `27670` completed
with exit zero. Standalone `codex-alab-autopilot --lib` ran each exact name
with fresh recorded state and outer `--test-threads=1`:

| Exact name under `recovery::tests::` | Seconds |
| --- | ---: |
| `native_opaque_handshake_fallback_preserves_provider_deadline` | 1.49 |
| `native_repeated_opaque_http_400_exhausts_retries_without_repeating_completed_work` | 0.85 |
| `normalized_bad_request_keeps_provider_deadline_across_restart` | 0.00 |
| `recorded_raw_bad_request_requires_its_pacing_record` | 0.00 |
| `a_crash_record_still_marked_busy_cannot_generically_repeat_a_policy_refusal` | 0.00 |

Each exact name passed once with zero failed/ignored/measured tests and 969
filtered out. Each selection records one library independently of its exact
pass line and parsed summary. Current metadata/artifacts select only the host
runtime helper, both aliases match the native receipt, and native unit-runner
identity is verified for every selection. Three current binary SHA identities
cover the host, unit runner and library executable. The host is lazy: preparing
and selecting it establishes no claim that these tests invoked its process.

Scoped Autopilot all-target Tippy completed under its separate canonical
`target/tippy` target. Independent stream checks confirmed 1,393 unique,
existing canonical artifacts, both normal/test Autopilot library profiles and
successful build completion. Classifier `owned`, `spanning`, `failing` and
`scripts` lists are empty; three upstream diagnostics are set aside.
Controller, unit and lint exits are zero. Feature inventory passed with
88 hook topics and 14 features.

The complete private archive is retained at
`.git/alab-validation/7476baefdbf49aa05e3ef56b06a66e4ebd722a77/autopilot-helper-preparation/`.
All 980 indexed regular-file lengths and hashes were independently checked,
plus the three current binary identities. Evidence includes 11 retained inert
wiring fixtures (one baseline, ten candidate) and 76 recorded-run files.
Raw tooling/native receipts and contained environment details remain private;
only safe counts and hashes are recorded here.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `30446b1f720c0fbb63985067b114fb8bcdc5f9cf0641ab13ba7025a59311c813` |
| `source.diff` | `c1e3f0bfbb0a4b3b649c0e1cc6757e09df0850088d2d4c57b74b53b555335d86` |

These qualify runtime-library preparation and behavior at the named source.
Application build/certification, installation, native accepted-actor qualification
and backend-cause claims remain separate; the existing application
macro-admission boundary is unchanged. Publication identity and receipt remain
in the private validation ledger.

## Recheck after peer merge

Composed source `0c176de3b8d2014efd8fb88d1817f496b57d0532` includes remote
`f2fc3c90a6bd3061b2c2551aa07c781e50045e7b`. The peer merge changed a compiled
`app-server-protocol` dependency's `CoreBioPolicy` mapping (`BioPolicy` to
`Other`), alongside quota-fixture and documentation changes. The production
dependency change warranted fresh native and scoped lint qualification.

Execution `82864` completed with exit zero. The same five exact cases listed
above each passed once, in that order, in 1.52, 0.89, 0.00, 0.00 and 0.00
seconds. Every summary reports zero failed/ignored/measured tests and 969
filtered out; every selection records one library. These are five distinct
cases with ten executions across the two qualified sources, not ten distinct
tests.

Fresh metadata/artifact selection and native receipts again confirm host-only
preparation, both host aliases and the native unit-runner identity. Three
composed binary identities match the current artifacts. The initial archive
and its recorded identities remain immutable evidence of the earlier run;
reused cache paths were rewritten by the composed build and do not retain the
old binary hashes. No old target binary was revalidated against historical
identities. Lazy host selection still makes no host-process invocation claim.

Scoped Autopilot all-target Tippy again reports 1,393 unique, existing canonical
`target/tippy` artifacts, normal/test library profiles and successful build
completion. Owned/spanning/failing/scripts lists remain empty, with three
upstream diagnostics set aside. Controller, unit and lint statuses are zero;
feature inventory remains 88 hook topics and 14 features. Tooling inputs were
byte-identical, so the 60 tooling passes remain attributed to `7476baef` and
were not rerun or relabeled for the composed source.

The composed archive is retained separately at
`.git/alab-validation/0c176de3b8d2014efd8fb88d1817f496b57d0532/autopilot-helper-composed/`.
All 87 indexed regular-file lengths and hashes and the three current binary
identities were independently checked; its recorded-run directory contains
76 files. Raw receipts remain private. The initial archive's record hash above
is unchanged.

| Composed artifact | SHA-256 |
| --- | --- |
| `record.json` | `3c1352397c89d949333aa39e498dc0843f8a5269274ac0beb50a0d2f0a6e891d` |
| `source.diff` | `3a15992e46807f19a4e9765c472311f22ff440e8861ab9d8f43d6ae3ea1097a8` |
