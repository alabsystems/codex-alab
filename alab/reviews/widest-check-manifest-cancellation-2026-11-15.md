<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Widest check of the merged tree

Decision: run the maintained executable gate on committed merged main
`599ed29083688a6b937b7d96750eca2ec3b13ba3`, then advance available independent
runtime checks when certification cannot proceed. Keep the source fixed during
each broad run. A separate owned worktree holds the demonstrated manifest
cancellation fix, so its component checks cannot change the broad runs' inputs.

The actual `qualification.py` invocation runs unmodified `alab/check.sh` through
the qualified source-selected controller, with Trust 15698 and the existing
protected profile. Original command 56392 fully exits 2 after 6m10s. All-target
Tippy completes in 5m43s with zero ALab findings and 41 upstream diagnostics set
aside. Certified CLI construction refuses in-process proc-macro dependencies
under Trust's current TCB boundary, then reports missing structured proof
transport, authenticated coverage and terminal target inventory. It measures no
proof obligations and produces no application certificate.

The canonical qualification observation records status 2 and
`qualified_source: null`; before and after source, tree and origin base are
identical and clean. This remains a failed gate. The executable's later runtime,
ALab raw-test and hooked upstream-unit phases do not run. No compiler exception,
alternate proof policy, unverified shipping build or installation is introduced.
The certification operation is deferred at the observed compiler boundary.

The gate's nine raw payloads and exact committed wrapper/policy sources are
retained under the common Git directory at
`alab-validation/599ed29083688a6b937b7d96750eca2ec3b13ba3/widest-native-gate-uh29shqv/`,
index SHA-256
`a37bf851dd9a07f87d9c88a06540a1bd37ed3993795befcd886015f233173ad0`.
The canonical gate log SHA-256 is
`66d1a90121e78290316a1a93c3792245c4340f0f7e23798634ecdb1361007f8a`.

The independent maintained `python_tests.py` workflow also runs at this fixed
source through the qualified outer controller. It discovers all 86 root test
modules once and records 1,172 unique case IDs, digest
`053c079bf4bc18e2791fe48564555c8b8b0edec4b245d65aa2b713e71f07fc60`.
All three required native runner methods pass first in 3.610s. The remaining
1,169 methods complete in 1295.667s. Original controller 66652, the actual
workflow and the after-source check all fully exit zero.

The maintained summary records `success` and `full_scope` true, every declared
case started and finished, and no integrity, missing, unexpected or unfinished
IDs in either phase. There are 1,163 fully successful methods, eight whole-method
skips and one partially exercised parent with a non-UTF8 filename subtest skip.
There are no failures, errors, expected failures or unexpected successes. The
five optional-native opt-ins were naturally absent; the controller did not
unset them. Those eight opt-in-dependent methods and the partial subtest parent
receive no pass credit. Nine underlying Cargo invocations include expected
negative controls; their nonzero statuses are retained separately from the three
successful native unittest methods.

Source and selected tools, including the native adapter, have identical before
and after observations. Root verifies the raw manifest against both start and
finish multisets and both phase summaries. Fifty raw payloads, native context
records, frozen controllers and exact source/tool observations are archived at
`alab-validation/599ed29083688a6b937b7d96750eca2ec3b13ba3/maintained-python-workflow-e8bdsd9k/`,
index SHA-256
`ea92ecd2013900a8d878e8b000f1492e2ac3ac759aa278cbf82b81d7e50b9d6e`.
The completed-result JSON SHA-256 is
`f30dd168bf077d07620f1d6e41f96394ea40dc8820fc4eb9134f9a772cfed2e5`.

After the Python controller exits and the shared adapter's hash is rechecked,
the independent protected Rust supplement runs the default library union for
all twelve ALab library packages. The binary-only unit runner is built and
authenticated by the maintained helper. No filter, feature override,
`--include-ignored`, `--nocapture`, compiler exception or application build is
added. Existing source selection, target admission, four jobs/four threads,
canonical 65,536-file limit and helper stack setting are retained.

Original native controller 85345 fully exits zero. Raw Cargo and maintained
collection both return zero, and the normal guardian records completed owned
process-group cleanup with normalized exit zero. Twelve library markers,
summaries and native receipts reconcile 2,309 passing case rows, zero failures,
16 declared ignores, zero measured cases and zero filtered cases:

| Library suffix | Passed | Ignored |
| --- | ---: | ---: |
| api-tests | 75 | 0 |
| aterm | 235 | 3 |
| autopilot | 1,074 | 6 |
| cloud-contracts | 0 | 0 |
| core | 357 | 0 |
| input | 50 | 0 |
| kimi | 6 | 0 |
| quota-recovery | 89 | 0 |
| record-contracts | 7 | 0 |
| release | 17 | 2 |
| state-contracts | 7 | 0 |
| tui | 392 | 5 |

Counts derive from unique captured case rows and complete per-library libtest
summaries; record mode's deliberately empty `collected.passed` array is not a
pass counter. Every native receipt retains both aliases for the freshly
authenticated CodeMode host (SHA-256
`aa688a9ae031632af6d621455bd891abcbbcb4a06261e6732eb263d70db691d9`).
Source-before and source-after records are identical, and the unit adapter keeps
SHA-256 `b3ba555c1b547cf89fc7931e15fc70b5e92ffe803f417b973e451f4bb0998d0c`.
This is a twelve-library supplement; integration, binary-test, doctest and hooked
upstream-unit coverage is not inferred. Ignores remain ignores.

The native packet retains 132 raw payloads, 42 exact committed source blobs,
all twelve native receipts, metadata/artifact logs, per-case observations,
guardian completion and fifty bounded collector-named diagnostic manifests at
`alab-validation/599ed29083688a6b937b7d96750eca2ec3b13ba3/maintained-alab-libraries-xaubaqoj/`.
Index SHA-256:
`82ff173a5471a89d5d0e3281089bf5d7eaf10e34132c0cbb8aba6f9c5049203a`.
Root verifies every hash and size, the exact regular-file set, twelve receipts,
counts and byte-identical source records. Source copies come from the recorded
Git blobs; large compiled caches are preserved in place.

The adjacent manifest audit demonstrates cancellation becoming advisory success
after a secondary descriptor-close error. The [manifest cleanup record](manifest-cancellation-cleanup-2026-11-15.md)
records the five failing baseline controls, shared-policy fix and 62 passing
component methods. The fix stays in the isolated worktree until these broad
baseline runs finish. Those methods overlap the broad workflow and are not added
to its unique-case count.

Root then runs the committed updated reader from `3a04ae83ed03` against the
original native union's state, log and status, writing a fresh private output.
It returns zero and accepts all twelve libraries with no failed copies. The
result is byte-identical to the original native collection (SHA-256
`6491e48b24254e8b9ff6725b53ceb797e77aec6e6ca45d1129621b0776e05ffa`).
No native test or original evidence is replayed or modified. The original native
unit-log SHA-256 is
`7d4ebb55c71aee5f5ce16e81d3f982e58f7df965d9bc27c66f39c5a219cd0890`.
Four recollection payloads, including the updated reader's exact source, are at
`alab-validation/3a04ae83ed03e56eb5f2a5d4850c226f7c0a117e/wide-native-recollection-og69q57n/`,
index SHA-256
`b609d5b245068fae6e527530803c43efe7cad56b1c3f3ad4ddbef95b512a0209`.

While these runs execute, main advances to
`257042ffca82669dbb5ae6cdef3b321befc7b04d` with WebSocket valid-status recovery,
its API tests and guide. The committed incoming review's raw packet is unavailable
in this clone. Source inspection establishes that its recorded GREEN source
`677a68a6eba05212d9ba6cf4ec3b106a0f7584c2` and incoming main have identical
Rust trees and compiler/native-tool inputs, but that does not independently
authenticate its reported runtime counts here.

Decision: retain the broad 599 results under their original source and validate
the changed API scope locally after composition. Main and the tested Python-only
fix merge as `46a36e122f441c15305bb1dc9eca64f91f09a909`. Its Rust subtree is
identical to incoming main, and the manifest fix's two code/test hashes remain
identical to their passing component checks. No conflicts require resolution.

The actual targeted controller 53910 then fully exits zero on that combined
source. Default-feature library tests pass 78 public API contracts in 5.33s and
268 `codex-api` units in 2.01s: 346 passed, zero failed, ignored, measured or
filtered. Both native library receipts and maintained collection succeed with no
failed copies. Counts come from captured case rows and complete summaries.
Scoped all-target Tippy completes in 54.67s with raw and classified status zero:
zero owned, spanning, failed-target, missing-script and set-aside findings. No
pristine comparison is required.

The typed guardian completion, raw unit status, collector, case observer, lint
and source check all return zero. Before and after source/tool observations are
byte-identical; the source remains clean at `46a36e122f44`, and the native adapter
keeps its recorded hash. The private controller also checks readable, correctly
typed lint verdict arrays before numeric comparisons; failures cannot become
empty zero counts. A nonfatal peak-stat persistence refusal remains in the raw
log; no peak-stat measurement or permission workaround is claimed.

Root independently verifies the two-library row totals, collector, native
receipt, unique successful Cargo build-finished event, all lint-verdict fields
and unchanged source records. These are local merged-tree results; they do not
depend on the unavailable incoming raw packet or turn the earlier failed
certification gate into a pass.

The targeted packet is retained at
`alab-validation/46a36e122f441c15305bb1dc9eca64f91f09a909/api-composition-o5jdy_2h/`,
index SHA-256
`bdc40c786d92002106dfa3de2c158f11a75cc5755d520e55ea3a7ef081b7dc3b`.
Its 825 payloads include 792 exact Git source blobs bound to the captured input
hashes, native metadata/receipts, raw unit and lint output, frozen controllers,
and the typed completion. Root checks every size and hash, the exact regular-file
set, and all 792 source bindings. The completed-result JSON SHA-256 is
`ccf94d2bda16474d76fce79c5793b95d67b62fec09d314c1411844f75b080d42`.

Publication initially loses a fast-forward race without changing remote main.
The late update strengthens two Autopilot integration modules without changing
production code. The composed source `f436df42b25580155d99c0b96217906cbc1c9bf6`
then runs both complete changed modules under the source-selected Trust 15698
and existing protected profile, using maintained `all_alab_tests` preparation.
`goal_recovery::` passes seven methods with 156 other methods filtered;
`policy_continuation::` passes three with 160 filtered. The source-derived names
are disjoint: ten distinct passes, zero failures or ignores. These are focused
integration selections, separate from the earlier library union.

The ordinary Cargo statuses, focused-result observers, final source check and
typed guardian all return zero. Frozen source/tool inputs remain equal; mutable
origin references are retained separately. Helper metadata/artifacts authenticate
the supplied CodeMode host. Alias provenance comes from the actual maintained
helper execution and native artifact selection, rather than an integration
process environment receipt. The packet retains 978 payloads and 954 exact
committed source blobs at
`alab-validation/f436df42b25580155d99c0b96217906cbc1c9bf6/autopilot-module-composition-wf2id2gq/`,
index SHA-256
`ec40ca3d99f9c3c79f571ab615095a63a77534021c2e051270742e84d3194240`.
The completed-result SHA-256 is
`909f5ef742eb9bdcbcccacb6f05e275bb0a702b48a34a41267b2395fd9127ba0`.

Further independently landed mail, record-custody, delta-report and intake work
advances main to `698d9b8cad1b0750b7e15440e49430e54c132888`. Composition at
`a0a3f3e2bfe0956dc7670234fc9ad4b1bb3fb470` preserves those commits. None changes
this fix's implementation/tests, native test-helper code, gate script or compiler
policy/lockfiles. Intake and feature metadata belong to the incoming work.
The fix retains its tested hashes. A bounded final composition check passes all
36 directory cleanup methods, 26 native collector fixtures and 26 current delta
report methods: 88 methods, zero failures. Logs are
`<evidence>/codex-widest-final-{cleanup,collector,delta}.log`. These scopes overlap
prior runs; their counts are not aggregated into a new global total. Incoming
unrelated native changes are not relabeled as having run in the earlier packets.

A separate read-only installed-tool audit identifies Trust 27009 with a macOS
isolated proc-macro loader. Its isolation option defaults off, and whether the
verified frontend enables it automatically or certifies this application is
unmeasured. The official bundled-source inventory and audit are retained as
`<evidence>/codex-trust-27009-capability-audit.json`, SHA-256
`5642b79d41ef831ac8243d6d030d4c68eafdc91c6996dc087e73bfe08ff30ac5`.
The 15698 result is specific to the committed pin; neither unverified incoming
27009 tests nor capability source text establish a replacement certificate.
Compiler adoption and proof-policy changes are not part of these observations.

The final publication fetch observes main at
`950cbf5a9129cca320ab07f2efae185bf00f35ff`, adding API recovery code,
its tests and documentation. Those paths do not overlap the manifest fix,
its tests, the native test-helper code or compiler policy/lockfiles. Merge
them without replaying the unrelated broad checks; the API result above
remains bound to `46a36e122f44`, not this later incoming implementation.

The final report adds documentation after the checks. Existing compiler, proof
and permission boundaries remain unchanged. The failed application certification
and later unexecuted canonical gate phases remain distinct from all independent
runtime successes above.
