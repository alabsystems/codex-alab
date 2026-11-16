<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Reject inconsistent Python workflow outcomes — 2026-11-15

Decision: close a locally demonstrated result-integrity issue beside the recovery
workflow. The authoritative GitHub issue-list read returned HTTP403,
forbid.permission, side_effect none. That read is deferred; no other identity,
transport or permission workaround was tried. A local audit supplied a concrete
issue that can proceed without issue access or application certification.

At starting main `00041684045bd59af26923ab2c6e35b659c947c6`, a pure case can
report its original ID during discovery, selection, start and stop, but a foreign
ID during addSuccess. The workflow returns zero and success true while its own
phase summary contains unexpected_ids. Start/finish coverage alone and ordinary
result.wasSuccessful do not establish consistent callback accounting. The
retained baseline record hash is
`166a97e861bc63cc4f30eadb4f06233fe5795b71c608e024ec1b998e72998290`.

Qualification now requires the actual testsRun count and a success-ID multiset
that introduces neither unknown IDs nor duplicate successes. Each method must
start as its selected object and frozen phase ID. Success must belong to that
same active case before stop, with the same observed ID and no earlier outcome.
Stopping requires an observed outcome. Passing subtests alone are not a final
method outcome. Count and lifecycle checks retain the actual bad evidence rather
than replacing callback IDs or inventing success.

Ordinary skips, expected failures, failing/error subtests and multiple cleanup
errors remain ordinary unittest observations. A skipped subtest can supply its
parent's outcome only through the actual builtin subtest object, the active
case's live subtest link and its matching parent. An ordinary alias with a
lookalike test_case attribute cannot supply that result. Non-case fixture
holders are accepted only for standard inactive class/module error or skip
observations. In particular, fake fixture expected-failures and fixture skips
reported during a live method cannot create valid accounting.

The phase summary adds integrity_errors with fixed short codes. It copies no
subtest parameter ID, skip reason or exception traceback into those diagnostics;
normal unittest stderr remains available. Multiple subtest skips or fixture
observations mean that summing outcomes is not a method-completion check. Coverage
and qualification stay distinct: full_scope may remain true when all selected
cases started and finished, while inconsistent outcomes require success false
and process exit one. Native prerequisites retain their stricter all-success,
no-skip/no-expected-failure gate. These are reporting-consistency checks for
trusted unittest code, not a sandbox against arbitrary result-object mutation.

The controls exercise actual TextTestRunner callbacks with inert cases:

- Test-only source20a against the original runner: 24 methods, 21 successful
  methods and nine failed subtest assertions, zero errors; actual unittest CLI
  exit one. Foreign success identity, duplicate success and testsRun corruption
  all retain complete start/finish coverage before their qualification assertions
  demonstrate the defect. Record hash:
  `b78e022415047b01bb21494bff2d7c0e0df8201294f641df076b7c421800e385`.
- Test-only sourcefaaa against the original runner: 27 methods, 23 successful
  methods, 25 failed assertions across four methods, zero errors/skips. It adds
  absent terminal callbacks, same-ID alias success, post-stop success and passing
  subtests without a final outcome. Record hash:
  `ab1364e85932abf474145361d3822490ec1fd2bda9dffb0a58c3605d1e18c564`.
- The intermediate source637 still accepted ordinary aliases through a generic
  test_case link. Exact source injection demonstrates both skip and expected-fail
  aliases: one method, eight failed subtest assertions, zero errors/skips.
  Record hash:
  `e96922b1e226431c8289ea61f6f72273d7156dec25f8abe0425c587596adce73`.
- Exact source637 also accepts unsupported fixture-holder callbacks. Three
  focused methods retain two ordinary successful methods and eight failed
  assertions across the two malformed variants, zero errors/skips. Record hash:
  `a6e94b3d3cc2f210ae7c00427af7032bfb68524a0c1eb4502e8f0e9c6349eb71`.

The latter three RED controllers deliberately return zero after verifying a
failed in-process unittest result; that controller outcome is not called a
passing test result. Their exit-equivalent one is labeled as derived, not an
observed standalone unittest process status. Earlier RED and intermediate GREEN
observations remain source-bound and are not overwritten by later results.

At combined source `b0483286e10f180e93554e7e5a7ea2888c8c7ea2`, all 30 controls
pass with zero failures, errors or skips. The nine added methods also preserve
genuine multiple skipped subtests, expected failure, standard setup/teardown
fixture skips and multiple cleanup errors. The two-file record hash is
`e62fba236bcaaf2bd7154d0d503a625ea9148f9ee216a483c33a2d7114cf53a8`.

A separate pair of actual child CLI processes uses registered inert prerequisite
cases and injected discovery. The original source exits zero with success true
and unexpected success identity. Sourceb048 exits one with success false,
success_identity_mismatch and successful_callback_counts_mismatch; both retain
full start/finish coverage and the bad ID. This is CLI status evidence, not a
real native-prerequisite run. Its record hash is
`45f0c9e9aab6868ad1480fb49700cfecee3a47484138597b84565c848350f6ac`.

The same private inert probe preserves ten ordinary semantic rows (five
scenarios through two runners), including their selected summary fields and
lifecycle traces. The selected-object dictionary and Counter checks avoid per-case whole-list
scans; timing probes use three repeats per size.
At 3,000 remaining inert cases, median wall time is 24.531ms before and 29.767ms
after; at 1,000 it is 12.460ms and 9.136ms. Both machine samples show 12/12 cores
busy with normal memory pressure. These are descriptive accounting measurements,
not controlled application or full-suite latency claims. The compatibility
record hash is
`477388f1ca1c995abbfd1c69a92229c28b4f97549c6920359d94a233a88c87dc`.

A retained probe-template description incorrectly says the baseline lacks a
maintained runner. Supplemental metadata corrects that description: both source
revisions contain the runner. Executed probe bytes, source attribution, traces
and timing samples stay as observed. The correction record hash is
`6e35ba41d9e949d61be50b13692c70fae6f5f3327e99367d7f581167e31be529`.

Fresh main `c8339b136d81e8ff603f1632d10fc25868193a4a` adds guardian tests and
review/selection records. Composition `9530ebdb73f9dc01c79813c2b376ca2bd27d44ac`
preserves the checked runner and control source blobs. Through the repaired
tracker, all 49 selected real orchestration and guardian methods plus three
explicitly inert prerequisite methods run successfully: 52 starts/finishes,
zero failures/errors/skips and no integrity faults in either phase. This is a
selected integration, not actual native prerequisite or complete suite
qualification. The four-file record hash is
`11887c6458102ad9f5def310d119d9051da84da868df90c140e1f41f57e25cab`.

Actual merged-tree discovery finds 1,114 unique cases, no import/discovery error,
and no overridden run/call/id implementation among those cases. This inventory
imports modules but executes no discovered method or class fixture; it is not
1,114 passing tests. Its record hash is
`cbb620bee772c70f97a03417d71d86a33e032a13ece62b97cfcfe2314513c68f`.

Independent read-only reviews accepted the source, all 30 focused controls,
retained RED outcomes, actual CLI status comparison, ordinary semantic traces,
discovery inventory and all four selected-integration archive files. Decision:
land this reporting-integrity repair and preserve each observation at its exact
source; no whole-suite or native qualification is inferred.

Own changes are confined to ALab's Python runner, its tests and documentation. No Rust,
compiler selection, native admission, dependency, application installation or
certification change is claimed. The unavailable issue-list permission and
unchanged application-certification boundary remain deferred operations.
