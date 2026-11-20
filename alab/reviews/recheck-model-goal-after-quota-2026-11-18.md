<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep a model-blocked goal's chronology retry when quota changes

A failed chronology read preserves the model stop while the journal is
unavailable. The retry must survive loss of quota evidence so the original
point can read current goal, chronology and quota again. Use the existing
fresh-decision guard at both model-binding error callers. The later native
admission still requires current authority and exact goal custody.

Seven focused tests passed on merged revision `532f9a6cf406bc3730e6738cde4b86e76cb2be0f` with Trust 15698.
The native fixture makes a real model tool commit a block and returns HTTP 429
on the following provider request in the same turn. It checks that the native
usage wall and the committed source name that same turn. The test stages the
account binding through the existing fixture seam and uses the real quota
observation and wake APIs. It holds a recovery flush after the session Writer
runs, invalidates quota and makes the later chronology read fail on the real
journal lock. The delayed retry retains the same goal, source and generation.
After journal release, it restores the future reset with no new request.
Fresh quota then wakes one same-goal continuation before that reset.

The combined regression reaches the ordinary action path. An independent
source review checked both error callers and the selected controls include
the clean Land case. The runner, collector, coverage observer, source
comparison and command guardian all returned zero. All 12,638 source and
tool records match before and after. Verification covered the focused
Autopilot tests.

- Unit log SHA-256 `aa6ac464ddf6c3fe1ceb6b742f291dfcebd111a0bbd3213db0c8b55aff1b01c5`
- Source record SHA-256 `0c5f832d789bff7b4dc65087b9e33ad63142598e950b011f29468ad0fa1af4ca`
