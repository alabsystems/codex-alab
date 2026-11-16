<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Observe guardian loss on both sides of command completion

This records initial topic `6bad0fa139`. Its post-retirement numeric process-group
checks were later shown insufficient to identify the original processes. The
[exit-observer repair](guardian-original-process-exits-2026-11-15.md) supersedes
that part of the test contract; the original outcomes below remain historical.

Decision: close the remaining test gap with actual SIGKILL controls around the
normal completion receipt. Caught cancellation and I/O-failure tests do not
exercise process death. Preserve the production guardian, custodian and receipt
implementation; this topic changes only the existing lifeline test module.

The first case starts the actual normal `--result-file` CLI. A supervised
command and a real lingering writer publish atomic readiness markers, while an
unreleased command gate prevents normal completion. The parent observes the
empty receipt claim, kills only its still-owned guardian child, and reaps raw
status `-9`. Without releasing the command, it then observes the surviving
custodian retire the original writer group. The receipt stays empty and cannot
be parsed as a completed outcome. The command's wait status remains unknown
to this parent and is recorded as such.

The paired case uses a thin fixture wrapper around the actual `run_recorded`
function. After readiness, the parent releases the supervised command while
its writer still lingers. The wrapper publishes another atomic marker only
after `run_recorded` returns zero. The parent first verifies the completed
command-zero receipt and retired writer group, then kills/reaps that still-owned
wrapper with raw status `-9`. The complete receipt remains byte-identical. This
separates observed supervised-command completion from the receipt writer's
eventual process outcome without racing the CLI's natural exit.

Private fixture observations preserve raw guardian status separately from the
receipt's normalized command status. Readiness and release markers establish
the tested phases; polling intervals and one-minute hang detectors supply no
semantic timing assertion. Failed assertions release inert workers through
their private cleanup marker. Destructive signals target only the parent's
owned, unreaped `Popen` child; observed command PIDs/groups are never killed by
the test parent.

On base `7948fa2eae59cf771c3504dbf5241051a44aa5f5` plus this test-only topic,
managed Python `-I -B` passes both new cases in 0.214s, exit zero. All fourteen
existing receipt/result tests also pass in 0.615s, exit zero. Both commands
fully exited. Python syntax and `git diff --check` pass. Logs:

- `<evidence>/codex-guardian-receipt-loss-controls.log`
- `<evidence>/codex-guardian-receipt-loss-results.log`

The retained fixtures include inert source files, readiness records, guardian
logs and separate outcome observations. These cases cover the two witnessed
endpoints, not arbitrary termination during publication, power-loss durability
or hostile filesystem mutation. Existing post-write fault tests retain that
separate scope. These are local process-lifetime observations, with no source
admission, application certificate or installed-artifact claim.
