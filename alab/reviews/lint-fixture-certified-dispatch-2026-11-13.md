<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Model the current application stages in the lint fixture

The full tooling sweep could not run the native-runner-dependent families
until the actual unit runner was built. With that component supplied, the
lint-step family exposed its old model of `check.sh`: fake Tippy did not
create its target tree, and the fixture had no certified application build
or runtime-check stage. The real gate was correctly refusing that fixture.

Keep the production gate unchanged. Copy the real certified-build dispatch,
project entry, and proof policy into the synthetic workspace. Its explicitly
fake native front dispatches to the existing fake Cargo. The fake compiler
requires `trust build --certify`, the exact policy path, locked input, and the
explicit CLI target, then writes an observation marked `not proof`. A separate
runtime-check stand-in records invocation; actual runtime scenarios remain
covered by `test_check_runtime.py`.

Application startup observations and failures are distinct from the native
unit helpers. The helper identity, path and startup tests must continue to
test their own boundary, after application validation. Add negative controls
for compiler refusal, missing report, application startup failure and runtime
failure: each must stop before unit checks. No fake output here can qualify
or package a real application.

The separate native-context family passed all three tests when supplied the
actual runner and source-selected Trust 15698 directly via its documented
`ALAB_NATIVE_CARGO` input. The earlier ambient `targo metadata` timeout is
retained as a failed observation, not assigned a cause by that successful
replay. The three tests ran in 9.354 seconds; their log is
`/tmp/codex-native-context-pinned.log`.

Validation: all 49 lint-step tests passed in 234.309 seconds, including the
new early-refusal matrix. All 14 independent proof-build tests passed. The
earlier canary correction has 12 passing containment tests, and the supplied
native runner passed all 19 unit-results tests. These full-family replays
resolve the reported failures and missing-artifact families from the earlier
873-test sweep; they are not an uninterrupted rerun of that entire sweep.

The final logs are `/tmp/codex-lint-dispatch-final.log` and
`/tmp/codex-proof-boundary-check.log`. Real application certification remains
blocked by the recorded compiler limitation; these fixtures cannot clear it.
