<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Measure prerequisite preparation against its starting main

The previous packet started at `c8339b136d81e8ff603f1632d10fc25868193a4a`
and landed at `3cfff84e1226bcc3b6eadcca43b6991082c66ab0`. Incoming peer main
`06faff5df371` has the same helper/check/framework blobs as the starting main;
its Rust inputs match the landed packet. Compare the actual shell blobs while
holding the landed fixture framework, managed Python, qualified native adapter
and other fixture inputs constant. This isolates our prerequisite change from
the incoming Rust and Python work.

The comparison found a regression in an unchanged existing test. With an absent
target directory, the original helper reaches the inert compiler and preserves
its expected status 23, argv, environment, inherited FD and detached session.
The landed helper instead fails at unconditional helper-scratch creation before
the compiler starts. Actual child unittest statuses are baseline zero/one pass
and landed one/one assertion failure. The confirming controller's zero verifies
that negative observation; it does not turn the failed test into a pass.

Separate filesystem probes also identify unnecessary work. The fix computes one
requirements plan and lets each caller allocate only the directory it needs.
No unconditional target mkdir masks first use. Artifact retention is explicit:
plain unit runs authenticate captured Cargo JSON without writing a discarded
copy; recorded/comparison runs and the raw gate retain their evidence.

| Observation | Starting helper | Landed helper | Fixed helper |
|---|---|---|---|
| Existing absent-target process-wiring test | Pass | Fails before compiler | Pass |
| Unrecorded helper-free scratch allocation/removal | 0 | 1 empty directory | 0 |
| Unrecorded host artifact log discarded by cleanup | Absent | Present | Absent |
| Recorded/comparison and raw artifact logs | Retained where applicable | Retained | Retained |

The first probe observes a 971-byte extra fixture log. The independent fixed-pair
probe observes 1,006 bytes removed; different fixture paths explain the size
difference. These are measured fixture bytes, not production-size estimates.
The pure plan preserves the single package-classification table, trusted V8,
locked/configured builds, authenticated artifacts, both aliases, caller cwd and
traps, MCP handling, proxy isolation, result collection and complete raw scope.

Each experiment has four alternating paired trials in four scenarios: 32 timed
shell invocations and eight separate filesystem probes. Observers run only in
the latter. Conditions reuse generated fixture files after the first trial;
this does not measure cold/warm Rust compiler caches. All library sample
statuses are zero, and their Cargo work is unchanged:

| Library scenario | Metadata calls | Build calls | Test calls |
|---|---:|---:|---:|
| Helper-free, unrecorded | 0 | 0 | 1 |
| Host, unrecorded | 1 | 1 | 1 |
| Host, recorded | 2 | 2 | 2 |

The recorded row includes adapter preparation and the test's no-run invocation.
The `version` counter records Cargo/compiler-host identity queries from
`native_v8_env`, not CodeMode-host version probes.

In the original raw-host arm, identical valid fixture V8 inputs are seeded
outside timing; the host starts absent and both aliases start wrong. The
starting helper fails all four timed trials with the missing-host diagnostic.
The landed helper passes all four and actually invokes the fixture host. Both
retain the exact three-package default raw Cargo argv. The repaired raw phase
performs more successful work and reaches later checks, so its total duration
cannot be divided by the early-failing baseline to claim a speed change.

In the fixed comparison, all 40 shell invocations return zero. Raw scope,
package union, both authenticated aliases, actual fixture-host execution and
Cargo call counts remain unchanged. Caller cwd, EXIT and cleanup-trap controls
pass in every sample. Recorded logs remain; raw logs remain until their owning
gate scratch cleanup.

Median milliseconds for the three observations after first use are descriptive:

| Scenario | Starting → landed, first experiment | Landed → fixed, second experiment |
|---|---:|---:|
| Helper-free | 105.089 → 108.552 | 111.839 → 108.577 |
| Host, unrecorded | 324.815 → 318.890 | 321.620 → 316.745 |
| Host, recorded | 825.676 → 768.623 | 734.696 → 709.843 |

The experiments are separate paired blocks, not one three-way timing trial.
First-use observations and raw samples remain available. Machine load changes
from 1.3 to 2.7 busy cores in the first block and 2.5 to 7.0 of 18 in the second;
memory pressure stays normal and thermal state nominal. Four pairs under this
load establish neither statistical equivalence nor a native/application speedup.
Decision: remove the demonstrated unnecessary operations and fix the semantic
regression; do not optimize away required authentication based on noisy timings.

Commit `2644d189f809e159099c4a1ac480af7a76288630` contains the correction.
The existing regression passes in 0.343s. All 89 unique direct-consumer methods
pass in 353.404s: 57 gate fixtures, 16 disk-guard cases, two proxy-home cases,
ten V8-boundary cases and four publication controls. Focused checks are subsets
of that coverage. Original command `6983` fully exits zero. Eight source hashes,
the frozen helper and reused adapter hash match before and after execution.

Actual source-selected native validation at that exact commit also passes.
The maintained comparison helper runs the original native Autopilot case in
1.34s, with 1,078 filtered. Its collector accepts one passing copy/library;
both conflicting aliases seeded inside the protected profile are replaced by
the authenticated host. Recorded evidence remains present: the native artifact
log is 804,904 bytes and metadata 1,024,459 bytes. Those retained sizes are not
an estimate of discarded plain-run output. Original command `64410` and its
typed completion receipt exit zero; source status is clean before and after.

All evidence is under the common Git directory's `alab-validation/`:

| Record | Source prefix / directory | Index SHA-256 |
|---|---|---|
| Existing-test regression, 54 payloads | `3cfff…/helperless-target-regression-9itcmy6s` | `fa3fcd117338d36aae685db2d4b8097442e5e96fb4fa63a2aceb4331fe3f0726` |
| Starting/landed measurement, 3,158 payloads | `3cfff…/paired-helper-before-landed-l3707x52` | `08fe583ceea187aa36f05c2203cd4125cfb830432d2c8627314840cae7601703` |
| Landed/fixed measurement, 3,178 payloads | `3cfff…/paired-helper-landed-fixed-zic2dmbj` | `87533beec2ae8e264eeb63a42902756f3225c10385eea176003b61a085e493b0` |
| Consumer checks, 14 payloads | `3cfff…/lazy-prerequisite-consumers-q0_0zl1k` | `86f8f2fd91c20d8a23065b478313ec0d9127c8732fdd4594cce91e9389713322` |
| Actual native comparison, 24 payloads | `2644…/lazy-prerequisite-native-f7oqzmhr` | `845fc43e2946191ee79e4aa3f39bf4e810ac8130f754a463c97f917d064e9eaa` |
| Description notes | `3cfff…/measurement-description-notes-bljf76im` | `d441ee5b41b22c7416151d0306b9495fe28b694604aaa44892154af32a33e186` |

Independent audit verifies both measurement indexes and all payload hashes,
recomputes counts and medians, checks alternating order and observes filesystem
effects before deletion. The retained first README misnames the version probe;
its generic scope note about a frozen fix belongs only to the second experiment.
The separate description notes correct those labels without changing executed
bytes, original inventories, measurements or source attribution. The fixed
condition is explicitly base `3cfff` plus frozen helper content `6b6b95a4…`,
later committed unchanged at `2644`, rather than the helper committed in `3cfff`.

Fresh main `433b18b5a6c5` adds only two recovery-verification review documents.
Merge `8bb69584e2f1` preserves all eight tested source hashes and all Rust inputs;
no new replay is justified by that documentation merge. No application timing,
complete gate or certificate is inferred, and unchanged proof/permission holds
remain as previously recorded. All measurement and checking commands settle
before publication.
Final independent audit also verifies every consumer/native payload, reapplies
the artifact selector, reconciles all 89 case identities and the actual native
pass, and confirms all eight tested source hashes against both `2644` and `8bb`.
