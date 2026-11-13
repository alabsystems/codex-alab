<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Review context after the first qualified prefix

Remote main `40e22add9584e1bebc24957ebed33e3e9b55c303` contains upstream
`053e5b1ca914981275db97b1262a391d16e42556`. The canonical sync completed its
native gate and published through Agh, composing concurrent main before rerunning
qualification. The full review branch merged this exact main commit with no tree
changes (`cd3581b`); this is the evidence for retaining existing source decisions
under the new base. It is not qualification of the full integration.

The current complete range to `39b42aaacdbb55b86528b4294443696985c348ee`
is recorded in `39b42aaacd-from-40e22add95-intake.json`: 403 commits, 71 source
decisions and 332 pending entries. Entries were carried only after exact SHA,
subject and changed-path agreement with the two earlier source records. The
original range records remain historical evidence. No runtime test result or
qualification was carried forward by this reconciliation; all candidate-specific
scenarios remain required. Validation must use the exact recorded main base and
upstream target, and reject stale or incomplete coverage before intake closes.
