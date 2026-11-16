<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Restore helperless first use and remove measured discarded writes

The measurement comparison isolates the shared-prerequisite change from incoming
peer work. Starting main `c8339b136d81` and peer `06faff5df371` have identical
`unit-tests.sh` bytes; peer `06faff5df371` and landed `3cfff84e1226` have identical
Rust inputs. Existing unit paths keep their metadata/build/test command counts.
The added raw Autopilot host preparation fixes a demonstrated missing runtime
dependency and is not an avoidable cost.

Two avoidable changes are concrete. Helperless unrecorded units allocate and
remove an unused scratch directory. Host unrecorded units also write a native
artifact JSON log immediately discarded by cleanup; the first paired fixture
observes 971 extra bytes. A separate unchanged existing test proves a semantic
regression: with an absent target, the c833 helper passes
`DiskGuardWiringTests.test_unit_cargo_is_guarded_inside_its_detached_session_with_argv_env_and_fd`,
while the landed helper fails at mktemp before reaching the inert compiler's
expected status 23 and process-context checks. Both private copies use identical
lib, guardian and test bytes; only the helper differs. The baseline unittest
process exits 0 with one pass; landed exits 1 with one assertion failure. The
private verifier's zero means the expected regression was observed, not that
the landed test passed. Evidence:
`<evidence>/alab-helperless-target-8o87mrou.noindex/outcome.json`.

Decision: compute one pure requirements plan per caller from the single package
classification table. Both allocation and preparation consume that same plan.
Helperless and V8-only plain units allocate no helper scratch; recorded and
metadata-requiring runs retain caller-owned directories. No unconditional target
mkdir masks the first-use failure. Preparation validates its planned bits before
I/O and retains locked/configured build, trusted V8 resolution, source/target
artifact authentication and both host aliases. Caller traps and cwd, MCP CLI
handling, proxy isolation, library result collection and raw Cargo scope remain.

An explicit retention bit removes only the measured plain-unit artifact write.
The captured JSON still supplies the existing authentication. Raw, record and
comparison callers keep their artifact logs. The independent fixed-helper probe
observes the landed helper's extra 1,006-byte file disappear while metadata stays;
recorded and raw logs remain. File sizes differ between private fixtures because
their paths differ, so metadata-size changes are not attributed to this fix.
All 40 measured Bash children exit 0, with command counts, caller traps/cwd,
scope and aliases preserved. Helperless scratch allocations change from 1 to 0.
Record:
`<evidence>/shared-helper-fixed-measure-47l9zlva/verified-observation.json`.
These are fake orchestration/filesystem observations, not application speedups.

On base `3cfff84e1226` plus this patch, the unchanged disk-guard regression passes
in 0.343s and a five-method focused subset passes in 5.083s. Original `6983` then
fully exits 0: all 89 unique consumer methods pass in 353.404s, with no errors or
skips. The complete suite comprises 57 gate-fixture, 16 disk-wiring, 2 proxy-home,
10 V8-boundary and 4 publication methods. Focused cases are subsets, not additional
unique coverage. Three new methods cover the pure requirement unions, no helper
scratch for helperless/V8-only runs, and a host snapshot taken before cleanup;
the existing record test still requires the retained artifact file.

Logs are `<evidence>/codex-lazy-prerequisites-{disk-regression,focused,full-consumers}.log`.
The pre-test `codex-prerequisite-lazy-scratch-context.json` beside them records
all source hashes and the reused qualified native adapter hash. Its adapter was
selected from the existing merged native metadata/artifacts, without a Rust
build. All recorded hashes and the benchmark's immutable fixed-helper copy
remain unchanged after testing. Bash syntax and diff checks pass; independent
read-only review approves the exact frozen helper and fixture sources.
Root verifies the fourteen-file consumer record under the common Git directory's
`alab-validation/3cfff84e1226bcc3b6eadcca43b6991082c66ab0/lazy-prerequisite-consumers-q0_0zl1k/`,
with index SHA-256
`86f8f2fd91c20d8a23065b478313ec0d9127c8732fdd4594cce91e9389713322`.
The separate 54-file existing-test RED record is alongside it at
`helperless-target-regression-9itcmy6s`, with index SHA-256
`fa3fcd117338d36aae685db2d4b8097442e5e96fb4fa63a2aceb4331fe3f0726`.
The failed landed observation remains failed; the fixed consumer pass does not
rewrite that source attribution. The exact-commit native Autopilot positive
observation remains pending before publication.

The exact-commit native comparison at
`2644d189f809e159099c4a1ac480af7a76288630` also passes: the actual maintained
helper replaces both conflicting aliases inside the protected profile and the
native collector accepts one original comparison case in 1.34s. Recorded native
artifact and metadata files remain intact. Original command `64410` and its
typed receipt exit zero. The [main-comparison record](prerequisite-main-comparison-2026-11-15.md)
contains source bindings, paired measurements, archived records and timing limits.

No production Rust, manifest, compiler, profile, permission or proof boundary
changes. No application, complete-gate or native Linux result is established.
