<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Recheck a wall's goal when its journal is busy

A blocked goal can still owe recovery after a provider error or full context.
When a chronology lock or database read failed during wall standing or final
compaction admission, Autopilot treated the unread binding as absent. The
fallback could then retire the wait and display the blocked goal as waiting
for a person, even though its row and journal generation had not changed.

Keep read failures distinct from a confirmed mismatch and use the existing
unread-goal recheck. Final admission releases acquired guards before scheduling
a retry. The successful admission still checks goal identity, person stops,
and the native goal epoch where applicable. The chronology guard remains held
through queue acceptance. Independent source review `400e03f6` found no blocker.

Five focused `codex-alab-autopilot` tests passed on `95f1466e08c58551d7d7865fbe849999b7e84f43` with
Trust 15698, the maintained unit collector and real helpers. The new regression
holds the real journal lock during wall standing and again just before
compaction admission. Both attempts leave a retry timer and produce no action
or goal attention. After release, the same blocked goal gets one compaction.
The other checks cover bound and unbound compaction, unresolved and settled
host generations, and correct retry accounting. The test advances evaluation
explicitly after release. Test, collector, coverage, source and guardian all
returned zero. The log includes upstream deprecation and linker diagnostics
and a refused peak-record write. All 12,593 source inputs and tool records
match before and after.

- Unit log SHA-256 `5c8c0d53821b9858c008fb3a758629b5a6816ee7b78762fe8fd4f27262b22b64`
- Source record SHA-256 `eb910aee3db6cbdda6f569532150d4fe274a5115825d7ca1335aff35c4f515c5`

This source decision is supported by focused regression coverage. The previous
full gate on 5c953cf6 stopped at 940 existing lint warnings. Application
certification remains pending.
