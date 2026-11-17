<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Goal regression and mail-reference lint cleanup

The broader startup check includes Autopilot production and integration targets.
It exposed ten preexisting owned diagnostics in three unchanged base files.
Result/Option `expect` calls now use the existing explicit panic convention, and
the mail row-reference closure is the equivalent method pointer. Success paths,
assertions, goal identity/budget checks and error handling are unchanged.

Independent bounded review found these edits behavior-equivalent. No test was
added for this mechanical cleanup.

Validation used the source-selected Trust compiler and the existing protected
project runner. The application was not built or certified. Ordinary retained
startup and actual Native SDK/UI activation remain unselected and unqualified.
The live-update objective is incomplete.

The checked final combined input is
`4725aa93487769acb2f69df567c70aab3def3bdf2f04937abc3b236eda8447e1`
on `f56a12c11957d8218e59f403847147019d4b9641`. Eight-package production/test
Tippy found zero owned, spanning, failing or missing-script diagnostics; 16
upstream diagnostics were set aside. A manual audit of warnings in changed
upstream paths found only three duplicated original ClientRequest patterns,
unchanged from the base. Exact logs and terminal statuses are preserved in the
workspace `retained-startup-*` records. These dirty-input checks are not relabeled
as a clean-commit application build.

Checked code hashes (SHA256):

| Path | SHA256 |
| --- | --- |
| `codex-rs/alab-autopilot/src/supervisor.rs` | `a4ee9ffa6240913aa6c4ac57fd1858f90b4ee89a3bb64871a0226ebf8d48f257` |
| `codex-rs/alab-autopilot/tests/autopilot/goal_recovery.rs` | `b11f9639b7c53378a608e2cb2c3eea1f2edf7d5753f1083a8ed02d2e21ee7995` |
| `codex-rs/alab-autopilot/tests/autopilot/policy_continuation.rs` | `41b691847e5e4c750f5b7ecfbae80637dce8e6769c8b0e771ca0e9a8b9f61e36` |
