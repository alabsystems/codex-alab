<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Close the Python workflow after native fixture repairs

Follow the verification selected at landed `447d16e052`. Earlier broad workflows
retain their actual failures and setup errors; focused repairs and replays do
not replace those results. The native context fixture now owns coherent Cargo
output paths and preserves the strict artifact/process-context checks.

Decision: observe a new full Python workflow on one clean committed source.
Inventory all 82 current `test*.py` modules under `alab/tests`. Run 81 modules
sequentially in fresh isolated managed-Python interpreters through the existing
source-selected outer context. Their gate and containment fixtures retain their
own original inner profiles. Reserve `test_unit_runner_native.py` for a separate
protected native run, with the actual authenticated unit adapter, private Cargo
home and unchanged 60-second subprocess deadline.

Leave the five explicit native experiment variables unset: package-cache atpkg,
package-cache Trust, project-controller, package-source and failure-diagnostics
probe opt-ins. Preserve normal tool prerequisites. Record actual environment and
platform skips rather than forcing a historical count or treating them as passes.

Retain per-module output, ordinary unittest outcome, test rows, whole/subtest
skip events, fixture errors, failures and expected/unexpected outcomes. A private
observer may reuse the maintained named-result bookkeeping, but must not call
its admission validator or rewrite ordinary success merely because a suite
skips an opt-in. No global unittest monkeypatch or source mutation during the
workflow. The artifact index records module/helper hashes, source identity and
actual statuses; native context observations remain private.

This is local workflow regression evidence, not named-feature admission,
application certification, a replacement native gate or installed adoption.
Do not retry the known unavailable application certificate or nested-terminal
operation, change compiler/profile rules, or replay unrelated Core/Autopilot
libraries merely to increase a count. Resolve any demonstrated defect on its
own evidence after the frozen run finishes; preserve the original outcome.


## Observed full workflow

Frozen source `eec6d91a90a18dc21b0a9d4e2a732b7761f623e8` completed all
81 outer module processes with exit 0 in 1549.231 seconds. Every module has
separate raw output, ordinary unittest results and process metadata. Source
files and Python, native adapter and observer identities stayed unchanged;
another session's fetch changed remote references without changing this source.

The outer records contain 1057 distinct method identities: 1046 complete
passes, ten whole-method skips and one method with a skipped subtest. The ten
whole skips are eight explicit native experiments and two existing certified
release-gate notes. The subtest requires a filename this filesystem cannot
represent. No failure, error, unexpected success or expected failure occurred.
These skips do not count as exercised passes.

The separate protected native run retained three fully named successful
methods in 1.370 seconds. Its actual native C entry preserved process context,
FD/signal state and invocation environment, retained the Python startup negative,
and left the unrelated inherited build directory's sentinel untouched. The
original exec session had disappeared by the post-compaction check: its outer
controller exit code is unavailable. The unittest success and subsequent
sentinel success are directly retained; no replacement run or invented terminal
status fills that gap.

Together the two observations contain 1060 distinct method identities,
1049 complete passes, ten whole skips and one partially exercised method.
An independent reviewer reconciled every result/process record, all 84 source
hashes against the frozen commit, native context evidence and all 366 indexed
artifact files. Private evidence is under the common Git directory at
`alab-validation/eec6d91a90a18dc21b0a9d4e2a732b7761f623e8/full-python-workflow-y04i8633`.
The artifact index SHA256 is
`9ac4b3c5f40b1a531297752ff9ff3125107c4dd524b6ea2fe58a28074ef65544`.
Raw inherited environment observations stay private.

## Incoming main composition

A fresh Agh observation and maintained fetch agreed on
`c769ed91b987f6769d50dbe767b6a031272a5f5a`. Merge source
`16c715bbc39a51b1558fbbd592326852c40d779d` incorporates its six incoming
commits. The full Python workflow above remains evidence for its original
source. The incoming production delta changes quota-credit expiry checking and
future reset eligibility; it does not overlap goal recovery or native diagnostics.
An independent hostile review found no actionable defect: freshly selected
credit expiry is sampled after listing and rechecked before the first POST;
pending reconciliation keeps its durable key and generation while rechecking
identity and current quota policy. Strict future ordering handles timestamp
bounds without subtraction overflow.

Focused observations on the clean merged source:

- Runtime-dispatch Python: 5 passed, actual exit 0.
- Weekly-reset fixture Python: 16 passed, actual exit 0.
- Feature inventory: 91 hook topics and 14 features, ownership complete.
- Protected source-selected Trust library execution: 89 quota tests and 357
  ALab Core tests passed with no ignored cases or failures; actual controller
  exit 0 and maintained artifact/context collector accepted both libraries.

- Exact upstream Core weekly-compaction parent: 1 passed in 1.96 seconds;
  its private child exercised seven local/remote/preflight/retry/sampling paths.
  Actual controller exit 0 and maintained artifact/context collection succeeded.
- Scoped all-target Trust Tippy for quota recovery and upstream Core: actual
  exit 0, zero ALab findings, eight upstream diagnostics set aside by the
  maintained classifier, and no spanning, failing-target or build-script finding.
  No pristine comparison was needed.

The separate composition archive is
`alab-validation/16c715bbc39a51b1558fbbd592326852c40d779d/python-quota-composition-lzbfepfn`
under the common Git directory. Its 36 indexed payload files preserve raw logs,
metadata, native runtime receipts, actual process statuses and lint classification.
Index SHA256:
`394e9b998240c7573df0db316af49619b3824e39a2cc0938aae0f4ae2005d99b`.
The unchanged inherited guardian and native adapter keep their existing process
boundaries; these observations grant no additional execution authority.

## Decision and next improvement

Land this completed workflow and incoming-main composition record. Preserve the
original broad failures and the current honest skip/terminal-status limits.
No application-certificate or nested-terminal retry is justified by these
Python/fixture and quota changes. The unavailable certified application lane and
unselected native experiments remain separate work, not inferred successes.

Next, add optional durable terminal receipts to the existing normal, non-cache
command guardian. The missing native outer status above is a demonstrated
observability gap. A small opt-in receipt can retain the supervised command's
normalized status after existing writer retirement, leaving incomplete evidence
when it cannot finish. Keep the guardian's own later exit distinct. Reuse the
current cleanup protocol rather than adding another orchestration framework;
review cancellation and persistence failures before landing it. This follow-up
is isolated from the source tested in this record.
