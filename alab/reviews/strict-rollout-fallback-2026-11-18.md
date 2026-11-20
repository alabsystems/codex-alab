<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Reject incomplete paginated fallback history

The rollout loader can skip a malformed record and return the items it understood
with a positive parse-error count. Autopilot discarded the count when the native
store declined a read and it fell back to the rollout. A malformed record can
hide the source turn or a later goal change, so the remaining records provide
insufficient evidence for a recovery decision.

Reject the fallback read when the loader reports unreadable records. Existing
callers can defer or retry through their unread-history paths. This change keeps
the current source selection and public APIs. Store reads that already discarded
parse errors, writer identity and inherited lineage still need their own checks.
The complete fix needs evidence from the canonical writer and every history
segment. An independent source review found no blocker in the fallback change.

Five focused Autopilot tests passed on `beed1788afa1367f7c77f711dc3e6e11b28a9281` using Trust 15698 and the
maintained unit collector. The new case uses a real paginated Core thread and
asserts the typed unsupported result before it exercises the fallback. Malformed
JSON and an undecodable record each cause the fallback to fail. Truncating the
fixture to its original length restores the same serialized history. Other
checks cover person stops, unread restart history, journal contention and
healthy paginated parsing. The first run stopped at a compile error in the
fixture because rollout items lack direct equality. The final run compares
serialized items. Test, collector, coverage, source and guardian all returned
zero. All 12,594 source inputs and tool records match before and after.

- Unit log SHA-256 `5b863f5ef35c16f80452bac9fd182ea493c8b099e29c03933fd4454f34a677f2`
- Source record SHA-256 `37fc0fd2c5730225152143361aad7d16a6e6cdb3b001b500fef3ba87abcea1a2`

The previous full gate on 5c953cf6 stopped at 940 existing lint warnings.
Application certification remains pending.
