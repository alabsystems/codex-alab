<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Native upgrade integration follow-up

## Decision

The completion-order fixture repair at `c4b4a8c4c4` passed 353 ALab TUI
library tests and 262 native TUI tests. Independent review found no further
defect in that change. Those runs deliberately leave four native integration
checks ignored: one requires a native signed-package admission host and three
require a compiled CLI with the early custody hook. Run those explicit checks
where their prerequisites exist rather than treating fixture success as
production upgrade qualification.

The source under test is `c4b4a8c4c4f1c75d5ace329c351ccde9f141c8b9`,
tree `561f2b40eb3a61b36218aafac0aebfe86e09bddc`, in the owned native-test
worktree. The following record is an integration observation, not release
qualification or evidence that the installed package contains this source.

## Native signed-package admission unavailable

The installed native executable is
`/Applications/aterm.app/Contents/MacOS/aterm`. The selected Codex package is
build `2026111101`, whose physical executable is under
`aterm/pkg/store/codex/2026111101/bin/codex` in the user's Application Support
directory. The existing ignored test
`upgrade_admission::tests::signed_public_package_through_original_native_child`
was run against these exact paths and that build number.

That package's `alab-build.json` records version `0.5.0` and source
`49f3c2ba233ba8f6891d6918309cd035b9257d30`; its `codex-package.json` also
records `0.5.0`. It predates the current source's early CLI custody hook.

The test failed before the native child connected: it exited with status 2.
A direct help query established that this installed native host reports
`pkg admit-executable` as an unknown verb. The retained test log is
`$HOME/.aterm/.ubd/codex-signed-admission-20261111.log`.

Signed-package admission remains unqualified on this host. Keep the production
refusal and continue with the separate real-CLI custody checks; neither an
unsigned fixture nor a different command is a substitute for native admission.
No package selection, credential, or installed executable was changed.

## Real CLI custody

Built the normal `codex-cli` binary from the source above with Trust's
unverified lane, `--locked --offline`, in the retained owned worktree. This
is the normal product feature graph, without the fixture-only `test-support`
feature. The build log is
`$HOME/.aterm/.ubd/codex-cli-custody-build-20261111.log`.

The normal build passed. Its direct capability response reports target
protocol 2, custody protocol 2, product version `0.0.0`, and no adopted
custody on an ordinary launch. The target binary SHA-256 is
`4ab49be3688fbf35428f78aff7651db1660e202d78194710acab01739941da47`;
the test binary SHA-256 is
`4ddd8d04a8c65d044df9ac22f44e27fca27b4ead944f8a72f88d849d1b1c0b46`.
Both hashes were unchanged after the checks.

All three selected integration cases passed: 3 passed, 0 failed, 0 ignored.

- `upgrade_admission::tests::compiled_target_probe_matches_the_retained_image`
  validates the real target's response against its retained physical image.
- `upgrade_exec_custody::tests::real_cli_adopts_queued_custody_before_its_runtime_starts`
  requires unchanged PID, actual early-startup adoption, the original lease
  held while the target runs, and release after the target exits.
- `upgrade::prepared_tests::committed_plan_executes_the_original_admitted_image_after_the_launch_shim_changes`
  commits the prepared plan through Shared, replaces the launch target,
  and requires the originally admitted image to execute with its custody.

The run used the existing test binary with `--ignored --test-threads=1`,
excluding only the unavailable signed-package test above. It supplied the
new source CLI through `ALAB_TEST_UPGRADE_TARGET`, its actual product version
through `ALAB_TEST_UPGRADE_TARGET_VERSION`, and the repository's 8 MiB test
stack. The tests copied that CLI into temporary package fixtures. The log
and identity receipt are `codex-cli-custody-integration-20261111.log` and
`codex-cli-custody-integration-20261111.json` under `$HOME/.aterm/.ubd`.

These are real CLI execution/custody checks with fixture admission. They
do not establish signed-package admission, a published automatic upgrade,
or adoption of this source by the installed package. No compatibility or
admission assertion was weakened.

## Next qualification boundary

Rerun the exact signed-package case when the installed native host supplies
`pkg admit-executable`. Do not substitute a fixture for that boundary. The
current host failure is recorded above; the independent CLI custody work
was completed without waiting for a host update.
