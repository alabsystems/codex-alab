<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Native fixture diagnostics beyond scratch cleanup

Follow the improvement selected at landed `71b2e14837`. The actual suspended
source-import failure printed retained fixture roots under the contained
scratch, but they were gone when the original controller returned. Harness
TempDir retention cannot defeat the parent runner's normal scratch cleanup.
That makes an otherwise useful failure report point to unavailable evidence.

Decision: transfer bounded diagnostic snapshots through the existing qualified
unit-test output path. Prepare allocates an exclusive diagnostic root under
the admitted target, separately from result state that may itself be scratch.
The native Cargo runner validates this root and assigns only the selected
library/round's exclusive leaf through `ALAB_UNIT_DIAGNOSTICS_DIR`. It records
that path in the actual runtime receipt and replaces any ambient destination.
Keep Cargo's PID, parent, session, signal and descriptor behavior unchanged.

ALab Core's optional test-support exporter accepts only registered fixture
roots with explicit aliases. It opens anchored directories without following
symlinks, writes into an exclusive child, and bounds traversed entries, depth,
regular files and actual bytes read. Special files and links are omissions;
outside-root traversal and overwrites are refused. A manifest distinguishes
copied, omitted and errored entries. `ShutdownReturned` records only that the
native shutdown API returned, not durable writer success or snapshot consistency;
`Incomplete` covers skipped/failed teardown. Export immediately after the
existing native shutdown and external hook joins where available. Drop may
perform only a bounded incomplete export, never start asynchronous shutdown.
Preserve explicit always-retain requests as well as failure-only requests,
register their owned hook roots, and remove the obsolete TempDir leaks and
misleading retained-source path reports.

Diagnostics supplement actual results. Neither a manifest nor its contents
can prove a test passed or replace the existing artifact/runtime/summary
qualification. Do not parse arbitrary stdout paths and copy them outside
containment, disable scratch cleanup, change the sandbox profile, or retain
every build merely to recover one fixture. Existing unavailable application
certification and nested terminal sandbox operations remain deferred.

Validate the native runner's exact destination binding and unchanged execution
context, the exporter's file/path/resource controls, and an actual failed
native fixture through normal protected cleanup. Require its admitted
diagnostics to survive while the scratch disappears. Then verify affected
Core/Autopilot behavior and scoped lint on frozen source before landing.

The first focused exporter run at `78177779fa` completed with eleven passes
and one failure. The failing control could not create its non-UTF-8 fixture
filename: the protected macOS filesystem returned EPERM before the exporter
ran. Official failed report:
`codex-rs/target/alab-goal-full-libs.S5VwjK/collected.json`. Preserve that
outcome; do not change the profile or interpret it as an exporter failure.
Use native Unicode filenames for this host's actual copy control, separately
verify lossless raw-byte manifest encoding, and reserve native non-UTF-8
filename-copy evidence for a filesystem lane that supports those names.

The corrected exporter source `802663905d` passes all thirteen host controls
in 0.24s; official report `codex-rs/target/alab-goal-full-libs.IeJe1m/collected.json`.
Its first native probe fails compilation because the Autopilot dev graph did
not enable Core's optional test-support module. The official collector refuses
that compile status 101 as missing native test evidence. Enable the feature
only for Autopilot's dev dependency, remove two obsolete mutable hook bindings,
and require an actual recorded round before the probe starts another phase.
No production dependency feature is added. Log:
`~/.ubd/codex-native-diagnostics-probe.log` (probe `espQWq`, exit 1).

On the same source, the real native-runner result suite passes all 26 tests in
10.780s. The following gate-fixture suite reports 50 tests, six failures and
two errors in 228.966s under an extra outer protected wrapper. Its six
failures are permission refusals allocating the inner fixture scratch; two
missing fake-Cargo captures follow that refused startup. The runner-publication
suite does not execute. Preserve this failed workflow. These gate self-tests
own their inner containment and require an outer source-selected test context;
run them there while retaining their original inner profile. Do not disable
containment or change a permission rule. Log:
`~/.ubd/codex-native-diagnostics-python.log`.

On corrected source `262a54f64b`, the real protected native probe exits zero.
Both actual native children retain status 101 and one qualified failed copy;
the outer verifier succeeds only after normal scratch cleanup. It checks the
exact prepared/receipt leaf, requested producer state, copied witness, deleted
source scratch and outside-link omission. The shutdown export contains 73
regular files / 996,741 bytes; incomplete teardown contains 74 / 996,743 bytes.
Both have two omissions and zero export errors. State:
`codex-rs/target/alab-native-diagnostics-probe.3rWK7B`. Log:
`~/.ubd/codex-native-diagnostics-probe-corrected.log`.
These are successful diagnostic-probe assertions around intentional failures,
not passing outcomes for the deliberately failed children.

The corrected source-selected outer Python workflow passes all 50 gate tests
in 330.280s and all four runner-publication tests in 11.924s. Each fixture's
original inner profile remains intact; all previously refused startup cases
execute and pass. Log:
`~/.ubd/codex-native-diagnostics-gate-python-outer.log`.
The earlier 26 runner/result passes retain their original source attribution;
their implementation and test inputs have not changed. All original controllers
have exited before further source changes. Remove the one remaining obsolete
mutable teardown hook binding reported during probe compilation.

Merge incoming main `702431f83a` as `edd25b986e` after all earlier controllers
exit. Its original-parser recovery-clock changes do not touch diagnostics or
the saved Pace model-source codec. An independent hostile review of the complete
diagnostics diff finds no blocking defect in traversal, actual-read/metadata
bounds, destination isolation, process context or result qualification.

On that clean composition, the same Cargo dependency graph passes Autopilot
1,073 tests (six ignored) in 217.26s, Core 357 in 0.56s, and all seven record
contracts in 0.01s. Official artifact/runtime/summary report:
`codex-rs/target/alab-goal-full-libs.TnzLdu/collected.json`. The two additional
ignored tests are the deliberately failing opt-in probe children, whose actual
failure/export path was separately exercised above. Log:
`~/.ubd/codex-native-diagnostics-composed-full.log`.

Combined all-target Tippy for Core, Autopilot, unit runner and record contracts
exits zero but reports two owned test warnings, so its verdict is not clean:
an obviously invalid literal's redundant UTF-8 assertion and a chunk-decoding
idiom in the new encoding test. Remove the redundant assertion and use fixed
two-byte chunks; preserve the independent exact hex expectation and decode
round trip. No production/Auto/runner/record-contract finding is reported.
Three upstream fetch_update deprecations match integrated `ce3031018ef1`.
Log: `~/.ubd/codex-native-diagnostics-composed-tippy.log`.

Final source `e0fac882c6` changes only that pure test assertion/iteration.
All 357 Core tests pass again in 0.56s; official report:
`codex-rs/target/alab-goal-full-libs.jdk3g9/collected.json`. Combined four-crate
all-target Tippy completes in 4.18s with zero ALab findings and the same three
unchanged upstream deprecations. Unchanged Autopilot, runner, contract and
exporter implementation retain the composed and native evidence above.
Logs: `~/.ubd/codex-native-diagnostics-{core-final,final-tippy}.log`.

The first mandatory native-context run executes three tests and fails their
prepare steps in 2.640s: the private controller assigned a nested fixture
target while inheriting the main workspace's Cargo build-output directory.
The artifact guard correctly refuses those outside-target binaries. Keep the
failed log/evidence (`alab-diagnostics-native-context.SjGrun`, 13 files).
Coordinate that controller's build-output directory under its declared fixture
target; retain the source, compiler, command arguments, protected profile and
60-second deadline. No runtime or permission rule changes.

The corrected real Cargo context run passes all three mandatory tests in
3.293s, zero failures/errors/skips, on the same clean final source. It checks
native PID/parent/session/group, descriptors and signals; the prepared diagnostic
directory replaces the known ambient value, while every remaining environment
entry matches that invocation's native prefix. Existing cfg-runner conflicts,
triple-runner precedence, missing receipt refusal and Python-startup detection
also pass. Private evidence: `alab-diagnostics-native-context.f64ZSC`, 32 files.
Logs: `~/.ubd/codex-native-diagnostics-native-context{,-corrected}.log`.
These are the three actual native controls, not a claimed rerun of unrelated
pure timeout-capture controls. All original controllers have fully exited.

Decision: land the bounded diagnostic handoff and remove obsolete retention
leaks. Genuine failed native fixtures preserve useful, clearly labeled evidence
through normal scratch cleanup without gaining test qualification authority.
No upstream hook, dependency version, compiler/profile exception, application
certificate, package installation or agent restart is introduced. Existing
unavailable certification/nested-terminal work retains its original limits.

Next improvement selected from the observed context failure: let the native
context fixture coordinate the independent Cargo output layout it owns, and
test an inherited build directory from another target. Today its caller must
coordinate both paths. Move that responsibility into the fixture while retaining
the artifact-containment guard and exact native-context comparisons. This is a
concrete test-infrastructure follow-up, not a reason to relax target admission
or repeat unchanged application tests. No implementation is included here.
