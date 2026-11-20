<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Publish qualification observations only after critical I/O completes

A gate can finish successfully before retaining its final observation fails.
The old recorder wrote directly to `record.json`. An error after complete JSON
had reached that name, including writer close failure, left a success record
that `inspect()` accepted despite the observer failing. Separately, ordinary
post-wait I/O errors replaced a known nonzero gate result with generic exit 1.

Decision: serialize to an owned temporary in the output directory, close it
successfully, then publish its inode with an exclusive hard link. Preserve the
existing refusal to overwrite a final record. Temporary cleanup after publication
is advisory for ordinary I/O errors; cancellation retains the existing priority.
Only an OSError after a normal wait returns a known nonzero child status gains
the domain error that lets the CLI report the original failure and retain that
status. Pre-wait errors, successful-gate errors, metadata contract failures and
cancellation keep their existing handling.

Tests were committed at `d5018931bd94f8f125c83e7efdf96da2faf1c768` before changing
production. All 11 new methods finished on unchanged production: one passed, four
failed and six failed-subtest, with 35 assertion events and no errors or skips. Two assertions
reproduced inspectable final success records after late write/close errors;
27 reproduced masking of known statuses 23, 75 and raw signal -15 (CLI 143),
including three exclusive-name collisions. One showed publication before close.
The other five required temporary-cleanup phases absent from the old code;
they are missing-phase controls, not reproduced cancellation masking. Healthy
execution and the existing close/snapshot cancellation controls did not fail.

The fix is `4e18870401`, based on main `846c05ee32003e1b0eb02d21c06a55dcc9296113`.
The new test bytes remain unchanged between RED and GREEN.
On the fixed source, all 150 selected methods passed, with no errors, skips or
failures: qualification 109, upstream status 27 and daily sync 14. All three
original processes, named-result collectors, guardians and source checks
completed with status 0. Both runs bound all 1,081 tracked ALab paths and 12
tool/helper/runner paths before and after; each run's bindings were unchanged.
The GREEN map SHA is `db8d4ad63c68706b8a5471321dd476c3f217d676c43aa929c4f81af255779676`.

Closed local packets: `codex-qualification-publication-red-d5018931-yz9upgo9.noindex`
and `codex-qualification-publication-green-4e188704-e8t1s8sv.noindex`.
The GREEN `closed-summary.json` SHA is
`76b613e30778384cace6eb9c42efd9aa50b6b5ee6fd15fbbaf7c300e487c3f20`.
The RED summary SHA is `0a1121f7921f48cd09b09ea97abf195a13bf073bb3bee10aea2fcd364368d6b3`.
These preserve original terminal statuses, exact named methods and raw output.


The regression fixtures use real private Git repositories, committed finite
shell gates, actual process waits and file operations. Write and close errors
are injected after complete real serialization/flush or physical close; they
are controlled late-failure reproductions, not evidence of a hardware fault.
The publication control requires successful close before exclusive linking,
including inode identity. Status/cancellation assertions execute both original
CLI entry guards. Existing qualification, cleanup, CLI, I/O, status-report and
daily-sync consumer coverage is retained.

Independent review found no blocker. Combined primary-write and temporary-unlink
failures were reviewed through the existing cancellation-selection policy, not
measured as a new combined-fault scenario. This change establishes publication
ordering for the public final name, not power-loss durability or authenticated
attestation. It does not restrict inspection of explicit copied record paths,
run a real native gate, certify an application, or promote/install a build.
The earlier native gate and real host/image prerequisite failures retain their
original source-specific outcomes.
