<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Request-recovery tool scope and CLI status

Request-recovery qualification accepted extra native custom-tool work because it
counted only function calls and outputs. Separately, a cancelled observation could
save `passed: false` and `interrupted: true` while its public CLI exited 0. On
this POSIX host, Python maps no-code, false, zero and bounded integer multiples
of 256 to success.

Decision: count the six native call tags and three native output tags declared
by `ResponseItem`, then require the one expected pair to remain a function call
and function output with the original ID, name, raw arguments, output and order.
Reasoning, messages, catalog metadata and nonfatal warnings retain their behavior.
No suffix guessing or runtime Rust-source parser is added.

The CLI now parses arguments before entering the operation guard, preserving
help and argument-error exits. It uses the existing `cli_status.py` policy to
refuse success-shaped cancellations and protect failed diagnostics. `main()`
retains its API exception identity; the CLI passes its already-parsed arguments
to that same body. SystemExit values that indicate process failure and
KeyboardInterrupt preserve their identity.

The test-only source `300c886506c70dc15a1b797291813d986a49fe68` keeps production
bytes from `4f8fdf7e54468ae6d95221ca511817f6c8db8f13`. Original run 34571,
completion 2c57f7, exits 1: 49 distinct methods finish, 41 pass and eight have
failed subtests, with 46 assertion events and zero fixture errors. The twenty
new methods are inside 49; subtests are not additional methods.

After the two production commits, original run 25043/completion bf734c exits 0
on `bf1b89148445a4f221673393d0f5d4a07bde013c`: the same 49 methods all pass,
without skips or failures. Both runs retain 861 exact Git input bindings and
three stable executable maps. The frozen tests and observer are identical;
only `request_recovery.py` changes in the captured source maps.

An independent controlled CLI observation executes the original final-entry AST
and real main body with only the case callback replaced. Five incomplete
cancellation configurations change actual child status from 0 to 1. Controls
remain 47 for a nonzero cancellation, -2 for KeyboardInterrupt, and 0 for healthy
execution and help. The same forwarding observer drives both sides. An earlier
fixed-arity observation is retained as baseline-only; no fixed run of that
observer or preparation failure is claimed. These are nine observations per
paired source, not additional qualifying test methods.

The sealed private common-Git archive is
`alab-validation/bf1b89148445a4f221673393d0f5d4a07bde013c/request-recovery-integrity-private-vibr28x1`:
1,964 payloads, 26,109,179 bytes and 1,734 Git bindings; index
`b521fb8cb65c8d6a866b709807e8606f47fd1d4e6d3843179a07596784dad525`.

Concurrent main changes were normally merged into
`0d86850b8634193f624ee9b9e4cdc50c61b4527c`, including the newly shared cleanup
helper, bounded package metadata, review-event ordering and weekly binary
identity checks. Original run 55605/completion dd21f0 exits 0: 96 unique methods
pass across request recovery 49, shared capacity 5, quota wait 16, package
metadata 8 and review 18. The request 49 are included, not added again.
All 867 captured inputs match Git and remain unchanged with three stable tool
maps. Its private seal is `request-recovery-merged-python-private-glbr54sp`
under that source: 891 payloads, 12,703,002 bytes, 867 Git bindings; index
`7ac99cc24d39af421bd60cacb23043d2cf7c80193cb279955b87502ffd39341e`.

Main advanced again with the weekly cleanup repair and workspace-routing tests.
After a normal merge into `bc883a60e11738f6a73c129709f1d955567167ee`, original
run 92138/completion 0d2c76 passes the same 96 methods. A separate maintained
weekly run, 27292/completion f6d218, passes 52 methods. All 148 method IDs are
distinct; both original outer statuses and all six maintained-runner children
are 0, without skips.
Each capture retains 868 exact Git inputs and three stable executable maps.
The final combined private seal is
`alab-validation/bc883a60e11738f6a73c129709f1d955567167ee/request-recovery-final-composition-private-d6g8f9cr`:
1,767 payloads, 25,801,964 bytes and 1,736 Git bindings; index
`24d86fc5825912255c3f1e119d0ba0a89d1eb3223f4d19bb4015856ba352edc5`.

The next identified gap was case teardown replacing a pending interruption.
A separate historical 24-observation API proof at 4f8 recorded sixteen such
faults. Concurrent commits `0a8f2ff072` and `0edcd3b2fe` repaired quota and weekly
cleanup through the canonical shared helper before a duplicate patch was needed.
Source review confirms exact cancellation priority, close/export after shutdown
failure, skipping the unsafe join, and withholding a passing case until cleanup
succeeds. The current 96/52-method runs verify that composition. The historical
observer asserted the old bug, so it was not replayed or represented as a paired
fixed proof. The private alternative helper draft was not applied.

Source, result counters, original statuses, inventories and owner-only archive
permissions are independently reviewed. All directories are 0500 and files 0400.
These runs use owned inert binaries, callback/listener substitutes and fixture
traces. No Codex binary, provider account, reset credit, compiler, application
gate or certification is exercised; existing holds remain.
