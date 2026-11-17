<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Corrected quota recovery: native library verification — 2026-11-16

## Decision and result

Execute the corrected quota-recovery Rust library left unverified in
[the reset-decision review](reset-decision-notices-2026-11-16.md). Its historical
run passed 87 of 95 tests before fixture corrections; a later resource-authority
refusal prevented that corrected suite from executing. Those earlier observations
remain attributed to their sources and are not passing evidence for this run.

On initial committed source `322c991e98ad16e817fce4a570aee9a3519c0439`, the complete
`codex-alab-quota-recovery` library passed all 95 tests. Failed, ignored, measured
and filtered counts were zero. Six individually checked methods covering visible
inventory/preservation, automatic redemption/restoration, unavailable inventory,
quiet below-threshold preflight, cancelled redemption, and timed-out redemption
passed exactly once. The initial library execution took 94.90 seconds; its configured production
45-second attempt-timeout path was allowed to finish within the test's 55-second
outer bound. That is not an independent exact 45-second stopwatch measurement.

Scoped all-target Tippy for the same package passed. Owned, spanning, failing,
missing-build-script and set-aside findings were zero. Native unit, lint and outer
controller statuses were all zero. This closes the corrected Unix quota-library
execution gap; no production defect or new source fix was demonstrated.

Remote main then advanced to `2f987b4c5250920e6740d59675fc9d5805b1d8fb`.
Its eight peer paths add a distinct policy-preservation outcome and three quota
regressions, bound shallow version metadata, and update documentation/TUI tests.
All incoming paths were merged byte-for-byte; the 18 provider JSON inputs stayed
unchanged. Because quota code changed, the earlier 95-test pass was not transferred
to the moving tree.

On composed source `8db4108e6887c6186e0650f5ed2b3892d7e8a819`, the complete
library passed 98 tests, with zero failures, ignores, measurements or filtered
methods. The six original required methods and all three new preservation,
malformed-usage and changed-account methods passed once each. Raw library duration
was 96.66 seconds. Scoped all-target lint again returned zero in every classification,
including set-aside findings; unit, lint and outer statuses were zero.

The incoming version-ledger fixture suite ran on that exact composed source:
47 methods reported, 45 passed and two existing certified pre-promote hook cases
skipped. Those cases remain skipped until their qualifying application-compiler
condition is met; no hold, authority or skip was changed. The fixture command
returned zero. This is focused Python coverage, not release/application qualification.

## Lane, inputs and custody

The maintained project selector chose committed Trust 15698, compiler source
`20565c3255adf151f9eb920e89cdf83c38f84abe`, through the prescribed native frontend.
The test command used `alab_project_run`, `alab_project_selected` and the maintained
`unit_tests --record-results` path with only `codex-alab-quota-recovery` selected.
It retained locked, explicit unverified library-test execution and one test thread.
That package requires no V8 archive, code-mode host, stdio fixture or Codex CLI.
Lint used the canonical separate target/tippy directory, the same package and
normal plus test profiles. No compiler, target or authority override was used.

The scope uses Wiremock loopback APIs, private temporary homes, synthetic JWTs,
explicit test authentication managers and real Unix custody files/locks. It does
not query a person's account or redeem live credits. The unchanged factories retain
ReqwestDefault proxy behavior; this run does not prove proxy-free execution in an
arbitrary environment.

Tests read 18 checked-in JSON files from `alab/smoke/fixtures/provider` at runtime.
For each run, their pre-run hashes, post-run hashes and committed Git blobs matched; file
identities stayed stable during independent capture. The source reader is bound
alongside them. Fixture contents were not printed. Runtime fixture paths are
explicitly protected during publication composition, in addition to the selected
local dependency closure, workspace inputs, compiler pin and native runner.

Independent live capture bound the quota executable and native helper runner to
native artifact digests, prepared and current five-field snapshots, launch paths,
working directory, execution receipts and diagnostic-file identities before any
rebuild. The two native phases retain separate executable, fixture-manifest and
source bindings. Recorded stdout is the actual complete library run, not a list of
source-declared tests. Raw evidence and independent reviews are retained under the
session's private `quota-recovery-native-20261116` validation stage.

In the initial phase an unrelated GUI test held an exclusive memory reservation while scoped lint was
queued. The original command waited for native admission, then completed. No claim,
pool, lease, compiler or target was changed to bypass that reservation. This run
received no resource-authority refusal; it does not establish that another
worktree's previously refused authority was repaired.

## Limits and deferred work

This is one Unix library under its selected feature/dependency context, not a single
combined execution with earlier library suites. It does not execute Core forwarding,
TUI presentation/history, the full feature/application gate or a packaged-binary
smoke. No new application binary was certified, packaged, installed or adopted.
The source-selected application certification prerequisites remain unavailable;
existing human holds and the compiler pin remain unchanged.

The next investigated parser candidate, positional arrays inside response.failed
error data, is already documented as inherited behavior. No compatibility change
or new defect claim was made without a separate policy decision. The original
opaque HTTP 400 backend cause also remains unknown. Existing recovery policy and
product code are unchanged by this verification topic.
