<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Native library fixture custody and result framing

The widest available merged check exposed three failing Autopilot fixtures and
two TUI subprocesses that leaked their own libtest output into Cargo's outer
result stream. The decision was to repair the test producers and their native
ordering, preserving the production mutation guards and the collector's refusal.

The genuine full Python workflow at `f810` is recorded separately in
[the merged Python review](merged-wide-python-2026-11-16.md). The selected Python
checks below do not replace that workflow or extend its source claim.

The original library run used clean source
`f810fd98e874d60aa1f11e5e496e013bdb36cf02`. Original process `30013` completed
with outer status **1**; its typed guardian receipt records the supervised
controller's normalized status **1**. The maintained `unit_tests` outcome was
**101**, the log writer returned **0**, the collector returned **1**, and the
source-after check returned **0**. Post-collection helper authentication and the
independent case observer were **not run**. These are separate outcomes, not a
successful collected library result.

The three actual failing cases were:

- `extension::goal_resume_tests::a_usage_resume_preserves_a_goal_changed_while_it_waits_for_the_mutation_permit`:
  the existing 60-second detector expired in the first `unchanged` case, with
  59.937 seconds in “finish person's goal mutation.” The test parked a queued
  automatic resume, then awaited the person's effects behind it on the same FIFO
  native goal permit without polling the resume.
- `extension::goal_resume_tests::a_policy_wall_survives_generic_goal_admission_retries_but_not_a_goal_change`:
  its ten-second “generic native goal retry” wait expired. It tried to reuse an
  already-consumed exact `GoalSetOutcome`; the native outcome boundary correctly
  permits its effect once.
- `extension::goal_attention_custody_tests::a_detached_active_read_keeps_a_later_blocked_goals_attention`:
  the existing 300-second detector expired. Its start observer held Core before
  task registration while the test awaited Active effects that submitted another
  ordered idle request to the same blocked routing loop. The later start-gate
  `RecvError` followed the timeout dropping its sender.

The TUI library's two exact-child tests,
`session_tests::model_source_publication_and_record_writer_share_one_lock_order`
and `session_tests::vanilla_mode_starts_and_flushes_the_identity_owned_session_writer`,
inherited stdout. Each successful child printed its own one-case row and summary
before the parent's same-name row and genuine library footer. The collector
refused “one unit-test copy reported more than one outcome without distinct
provenance.” Neither a parser exception nor inferred per-child result was used
to turn that original capture into a pass.

Commit `6aacbbe1c1` changes only the four ALab-owned fixture files:

- The usage fixture retains all nine controls. The person's actual native host
  scope holds its FIFO permit through SQL and exact effects, then retains custody
  until the competing goal identity or native input and expected state are staged.
  The queued resume proceeds after explicit retirement. Its goals, state, booking
  assertions, and per-case 60-second bound remain.
- The policy fixture consumes each original or replacement effect once while
  real Plan admission is declined without a provider request. After switching to
  Default it invokes the installed native goal extension's idle callback through
  `Harness::retry_idle_goal`. The policy/objective checks, ten-second retry wait,
  and 60-second outer bound remain.
- The attention fixture selects `RegularTaskStart` for its observer. Sampling
  remains held before provider work while Core can answer the effect's idle
  request as busy. All four real Active/Blocked attention scenarios, native
  completion, later-reason ownership assertions, and 300-second bound remain.
- The two TUI parents use the existing bounded ALab child helper. Exact selectors,
  environment markers, and ten-second bounds remain. Owned pipes are drained to
  EOF, the child is reaped, and one actual successful test is required. Successful
  child output stays private; failures retain bounded diagnostics in the parent's
  captured failure section. Both callers are ordinary synchronous test functions.

Independent source review approved the fixture choreography and shared helper
use. No production recovery policy, collector/parser, native adapter, feature
selection, or test deadline was weakened. Canonical four Cargo jobs, four libtest
threads, and the maintained unit helper's stack convention remain in use.

The library supplement follows the qualified source-selected Trust 15698 route
with the committed compiler `20565c3255adf151f9eb920e89cdf83c38f84abe`, the existing
protected profile, actual authenticated V8/CodeMode prerequisites, and maintained
`unit_tests --record-results`. It selects all twelve testable ALab libraries with
default features and no test-name or include-ignored override. The binary-only
unit runner is built/authenticated separately. Cloud's empty library does not
establish its integration coverage. Release's two declared native sandbox tests
remain ignored; no certified application, release publication, or separately held
release-gate investigation is executed.

The canonical `alab/check.sh` enters the held live application-certification
stage and has no standalone phase switch. It was not invoked or altered for this
supplement. No complete-gate, full Tippy-classification or adoption claim follows
from these library checks.

## Intermediate combined check

The first repaired full library check is bound to committed combined source
`38cb21ccefb4dc5869a31895f28812338b23d673`, not to the original `f810` source.
This tree also incorporates independently owned changes and intervening main
commits. Original process `6386` completes at `26244b` with outer status 1.
The unit command returns 101; the maintained collector, helper authentication,
log writer and source check return 0. Both repaired goal-resume tests pass, as do
all 400 TUI cases, with five declared TUI ignores and no leaked successful child
rows. Attention fails in 0.29 seconds: its later real GoalService mutation refuses
an unsettled or unknown accounting write. The private strict observer returns 1
because the failed child's diagnostic contains an additional summary. That
observation refusal remains separate from the maintained collector's accepted
one-failure result; neither is rewritten into success.

This exposed another setup assumption: the fixture carried an unapplied Active
outcome across newer native-start accounting callbacks, then tried a Blocked
write. Commit `fb03df8290` consumes and drops that initial exact effect under real
Plan mode before arming the attention gates. It asserts no active Core turn or
provider request, restores Default, and preserves all four subsequent stale-read
barriers and assertions. No persistence state is reset and no guard is bypassed.
Independent review approves this change. Intermediate source/tool maps remain
identical before and after, with 11,192 source bindings to clean 38cb21.

## Selected Python composition regression

Original process `98614` completes at `487d16` with status 1 on clean 38cb21.
Four disjoint maintained-runner patterns finish 176 unique methods: feature 95,
versions 66, package version 7 and quota-wait smoke 8. There are 174 passes and
two existing whole-method certification skips. No assertion failures or unittest
errors occur; the strict version recorder retains status 1 for those skips.
The other three children return 0. This is a partial selected regression result,
not a full-workflow rerun or qualifying success for skipped certification work.
All 850 captured regular inputs match Git and remain unchanged, along with the
three recorded executable maps. The independently checked original packet index
is `42cf2909237a8a62bc873472634196fa2e181ae96eaddc0255652fe6cd22ccd3`.

## Third full check and focused diagnosis

Original process `72325` completed at `6f65e1` with outer/typed guardian status
**1**, on clean `fb03df8290a512e58e0ab204ef88a9eecef8cd8e`. All three repaired
Autopilot cases passed. A separate unchanged case,
`extension::upgrade_handoff_tests::model_blocked_identity_tests::model_goal_observed_native_call_id_collision_failed_duplicate_cannot_strand_aborted_usage`,
expired after 300.64 seconds while awaiting its first PostToolUse entry marker.
The original script drains stdin before creating that marker: its absence does
not establish that the hook never launched. The unit status was **101**; the
maintained collector, helper authentication, log writer and source check were
**0**. The private observer again refused the nested failed-child summary with
status **1**. All 11,192 source observations remained unchanged. The corrected
metadata pointer names actual state suffix `74858db71c3a`; unused generated
suffix metadata remains preserved rather than substituted for an executed run.

An actual SQLite-engine read of a fresh private copy of the retained DB/WAL/SHM,
with the original bytes unchanged, observed an Active goal with ten tokens used
and a 1,000-token budget. That final row does not establish chronology or which
wait stalled. Source review found no demonstrated lock inversion: token callbacks
do not take the native goal permit, and Core releases its session-state lock
before awaiting them. No production guard or deadline was changed on this basis.
The intermittent failure remains unexplained.

Diagnostic-only commit `ebd173d8956be82667e1b45a5fe428421fe3c570` adds phase
tracking, hook-started/stdin-drained markers, actual callback/marker evidence
before explicit cleanup, and early failure for an unexpectedly finished nested
call without the required marker. It preserves success ordering, cleanup and
existing bounds. Independent review approved it. Original focused process
`41884` completed at `6f4ad2`, with exactly the selected identity case passing in
1.48 seconds. Autopilot reported 1,136 filtered cases; the other eleven library
artifacts ran zero cases. All seven component statuses and the typed guardian
were **0**, and source/tool observations matched. This is genuine focused
execution evidence, not full-library qualification or proof of the intermittent
failure's cause or resolution.

Further cause analysis is deferred until a reproduction carries the new
pre-cleanup phase data; the retained earlier trace does not support a production
change.

## Final unfiltered library and feature checks

Original process `48344` completed at `5bf612` with outer/typed guardian status
**0** on clean `b10ac08a796a95d72685466910e2ae122fe22a15`. The unit command,
collector, log writer, host/runner authentication, strict case observer and source
check all returned **0**. The complete twelve-library run reported **2,496 passes,
zero failures and 16 declared ignores**, with no measured or filtered cases.
All 11,194 captured source-file observations and eleven tool/controller
identities matched before and after.

| Library | Passed | Ignored |
| --- | ---: | ---: |
| API tests | 97 | 0 |
| Aterm | 259 | 3 |
| Autopilot | 1,131 | 6 |
| Cloud contracts | 0 | 0 |
| Core | 424 | 0 |
| Input | 50 | 0 |
| Kimi | 6 | 0 |
| Quota recovery | 98 | 0 |
| Record contracts | 7 | 0 |
| Release | 17 | 2 |
| State contracts | 7 | 0 |
| TUI | 400 | 5 |

The same source also passed the separately selected feature suite: original
`61755`, completion `c96686`, actual outer/child status **0**, **95 unique methods
passed**, and all 852 captured inputs remained unchanged. Its original packet
index is `70851d991e3e0884aa7361e1ef692704cce8ca3808178edcf6b52aeff54192e8`.
Neither this selected suite nor the focused native case is added to the full
library total or represented as a second full Python workflow.

## Preserved evidence

These are separate immutable milestones under `alab-validation/` in the
repository's common Git directory. Earlier failures and attribution refusals
remain failures:

- Original `f810fd98e874d60aa1f11e5e496e013bdb36cf02/`
  `wide-libs-original-private-88jp0ktv`; index
  `95ba70a4dd4833017fc70f4ea3df2dfa3b2bdede5afa9a33a33ea5ee26f8d0fd`.
- Intermediate `38cb21ccefb4dc5869a31895f28812338b23d673/`
  `wide-libs-intermediate-private-7c697r4s`; index
  `385d590406bbd8629ed3d54d0200f03bf47088f90a7a30c2b1856ca34ea00845`.
- Third full failure `fb03df8290a512e58e0ab204ef88a9eecef8cd8e/`
  `wide-libs-third-failed-private-61k66ol7`; index
  `cd5b697221bfd0decb47991546661d6a333f2fee37b6cedee7db208e61a88184`.
- Selected Python at `38cb21ccefb4dc5869a31895f28812338b23d673/`
  `wide-selected-python-private-0zh2grko`; archive index
  `cf1727b0ac4849b8448d0e453e479ce6e70b0ec2bbae3a3a4bccf525b5a24b7e`
  (separate from its original packet index recorded above).
- Focused native at `ebd173d8956be82667e1b45a5fe428421fe3c570/`
  `identity-focused-private-ghouvpm8`; index
  `bfab92344884adb99623d7ab7f94a20d6508f1560c5692a231b8253d03f68b21`.
- Final native at `b10ac08a796a95d72685466910e2ae122fe22a15/`
  `wide-libs-passed-private-q2ace5db`: 14,999 payloads, 201,678,468 bytes,
  11,194 Git bindings; index
  `5eddd17826d629096a0b1ee9f1ba790d636f92ec4fa0adac4fd3fee2b19efbcc`.
- That final native archive's immutable metadata correction is
  `wide-libs-metadata-addendum-private-hj7pk9kl` under the same source; index
  `ae6aec6ca076d0254f1799173e997f9d356aa7d1c640f0d05ca474cfee786de2`.
  It supersedes only an inherited exclusion of a green library sweep. The
  original seal remains unchanged; the correction affirms its genuine twelve
  libraries and retains the certification/integration/binary/doctest limits.
- Final feature at `b10ac08a796a95d72685466910e2ae122fe22a15/`
  `wide-feature-python-private-tgvyi660`: 864 payloads, 12,565,701 bytes,
  852 Git bindings; index
  `33a4e116dc7aa8dad638759bfb1a209c1e0f3e159b313a7ee91c0f5ca8f6ce63`.
- The separate status-only retained-database projection is
  `fb03df8290a512e58e0ab204ef88a9eecef8cd8e/identity-db-projection-private-38be5via`;
  index `2952bbb89359227391d69434d447daea4a9d97a4088fd3eaedc010efa4bd081c`.
  It binds three opaque original DB/WAL/SHM hashes without copying database files.
  Only the actual SQLite-engine result is authoritative; the earlier custom WAL
  decoder refused validation and produced no query result.

Archive contents, source bindings and owner-only permissions were independently
audited. Native environment, descriptor and context evidence remains private and
is copied and hashed as opaque bytes. Each seal has 0500 directories and 0400
files; no compiled target binary is copied. Original archiver session identifiers
lost across an archival compaction are not invented; durable seals and their
independent byte audits remain separate from the actual test-process outcomes.

## Latest remote composition

Remote main advanced to `8d0b2b0d32d66a66ae7422f9d55f4cc3aabe64ca` during
validation. Its API audio-error correlation arm, API tests and documentation were
normally merged into `22ac8ac1ac7a5bce793c3398e1dcdf9071b1f26c`. An initial
one-library observation (`63513`, terminal `b12057`) passed all 99 ALab API cases,
with six component statuses and the guardian at 0 and 11,195 stable source
observations. Its separate `wide-api-merge-private-518xmk3d` seal at that source
has index `7fabff84aca1612636cefa46212b63ebcedb03642154ca79a3595522fbd27f84`.
It is retained as one-library evidence and does not establish the documented
joint API dependency graph.

The README requires `-p codex-api -p codex-alab-api-tests` together for final
focused API regressions, preserving Hyper dev features. Original joint process
`39913`, start `cec96d`, runs that exact unfiltered pair on clean 22ac8ac1.
It completes at `3113d0` with actual outer and typed guardian status 0. Both
complete libraries pass: 99 ALab API cases and 281 API cases, 380 total, with zero
failed, ignored, measured or filtered cases. The six component statuses are all
0; all 11,195 source observations and recorded tool identities match before and
after. The source-selected pin, protected profile and B3 native runner remain
unchanged. These selected final-merge results are not a twelve-library execution
of 22ac and are not added to the b10 library total. Its private seal is
`22ac8ac1ac7a5bce793c3398e1dcdf9071b1f26c/wide-joint-api-merge-private-1cwz7a69`:
11,234 payloads, 138,420,326 bytes and 11,195 Git source bindings; index
`f2d45ae261a52e2da2466c54b5d3e91dda7e9d0b2337bd8626c9266bb49fe3f5`.

Record-mode `collected.passed` is intentionally empty; the final passing totals
come from genuine unique case rows reconciled with every native library summary.
This is unfiltered library test evidence, not integration coverage or a complete
application gate/certificate. The separate full Python source scope and all
previously held operations remain as recorded above.
