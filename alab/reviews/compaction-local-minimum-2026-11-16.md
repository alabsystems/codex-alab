<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Legacy compaction's captured local minimum — 2026-11-16

## Decision and reason

Cover the real owned `compact_retry_at` selector and `compact_wait` consumer
when provider advice expires after a positive caller delay has been captured.
Library arithmetic tests cannot detect losing the compaction glue's
`local_minimum` flag or discarding its selected wait at consumption. Add one
outer test and one private selector to the existing isolated Core wake harness.
Existing case bodies and the launcher/collector contract remain unchanged;
the child dispatcher adds the new selector. This is expected-pass coverage,
with no production defect or failing production baseline claimed.

## Observable contract

Construct the actual typed error and ten-second provider advice under paused
time. Advance eight seconds, read the real `error.retry_delay(1)`, and require a
positive two-second value. Model three seconds of scheduler preemption before
selection; verify the advice is expired and a fresh delay read is zero. Pass
the previously captured positive value to the real compaction selector. This
models preemption; the actual synchronous caller has no awaited operation
between its delay read and selector call.

Require exact publication at selection plus two seconds and one real selection
counter increment, with the consumer not yet entered. Model one second of
reporting, then pass the expired caller fallback and the owned selected wait to
`compact_wait`. Require Pending at entry and one millisecond before the selected
deadline, with pacing still published. Completion must be no earlier than the
selected deadline and no later than ten milliseconds after it. This allows
timer-delivery rounding while decisively rejecting a delay restarted after
reporting. Consuming the future must retire the actual pacing guard.

The existing injected host clock stays constant. The fixture uses paused time,
not physical sleeps or a host-performance bound. It does not execute the full
compaction request, actual `notify_stream_error`, history/input admission or a
provider exchange. Exact child isolation, private homes, the 90-second parent
watchdog, 60-second inner timeout and runtime-drop-before-marker gate remain.

## Qualification

Initial source `fd2fa52e6b358b7bd4c43f36c735af05290e4fc5` supplied a literal
positive selector argument. Static review prompted the stronger real delay
provenance above. Stop its queued frontend before changing source: controller
exit 143 and the admission log confirm the helper build did not start. Preserve
this administratively aborted phase and its records separately; it has no test
outcomes and is not a failed production baseline.

Qualified source `9558e0826c7bcc5af7527ad0853f292d8aa8d1e4` passed all 13
focused module methods once, including the new compaction method: zero failed,
ignored or measured; 3,261 other Core tests filtered out. Scoped all-target Core
Tippy had zero owned, spanning, failing or missing-script findings. Eight
upstream warnings were set aside. Unit, compiler and controller statuses were 0.

The qualified frontend selected committed Trust 15698. Tests ran in the explicit
unverified library lane, one ordinary runtime round and one test thread.
Maintained preparation supplied the real stdio/code-mode-host prerequisites and
source-pinned V8 data. Source and target stayed frozen through capture and
independent qualification: Core image, native runner, two prepared support
images, three aliases, receipts, diagnostic identities, both Core lint profiles
and 1,382 canonical artifact paths matched. Binding support images is not proof
of their launch; V8 archive and binding are build-input data. No application
certificate is claimed. The private `compaction-local-minimum-20261116` stage
retains the aborted and qualified phases separately; counts are not aggregated.

## Publication and limits

The owned change is limited to tests and this review record. The original and
qualification baseline is `45741dd67b0819128e5a2c9046b29416936f7658`; its phase
context remains immutable if expected-main publication later composes disjoint
already-main work. Preserve that work and the tested source attribution exactly,
with independent source/control/artifact/composition reviews before cleanup.

The original opaque HTTP400 cause remains unknown. The certified application
build prerequisite and human hold remain unchanged; this work does not install
or adopt an unverified application or alter the pre-existing peer session.
