<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserved variant composition

Compare the 54 saved refs with **pre-archive main**
`faf104c11323e30daf127ecda02716d08e25a519`, not with the ancestry-preserving
archive merge. `git cherry <base> <saved-tip>` finds complete patch-equivalent
integration for 46 variants. The inventory records every original patch and
which variants need additional comparison. This establishes historical source
integration, not runtime qualification; the combined gate is still required.

The remaining eight variants contain only 19 distinct unmatched patch IDs.
`git range-diff <original>^..<original> <integrated>^..<integrated>` independently
compares the same-subject integrations below. Its equality result concerns the
actual patch, not a matching subject alone.

| Original | Integrated/current disposition |
| --- | --- |
| `0714cc0887` | Range-diff equals `57b8704108`; destination configuration and requirements ownership documentation is present. |
| `5c0b31353e` | Range-diff equals `8e6f9479c6`; isolated unavailable destination owners are present. |
| `3da30a3356` | Range-diff equals `abe5b85a30`; loader-observation assertion is present. |
| `d939d63346` | Range-diff equals `08a5fdf839`; actual Noop provenance is present. |
| `887dc69563` | Range-diff equals `c7cafb0314`; local bootstrap and native routes are present. |
| `9b53e9a8ef` | Range-diff equals `a2847ba243`; all three tip-modified file blobs are also identical to the candidate. |
| `6b027dc78f` | Range-diff equals `5eea42fbb1`; owned actor-family freeze/borrow contract is present. |
| `38e9e8cf3c` | Range-diff equals `660944e66f`; the original patch also reverse-checks against the candidate. |
| `ddda87cd88` | Range-diff equals `15613d89f6`; the original patch also reverse-checks against the candidate. |
| `eca9cdaf08` | Range-diff equals `cad095b9d6`; the original patch also reverse-checks against the candidate. |
| `30630711f6`, `45c7e2068a`, `c331bafc86` | Range-diff equals `4b2d8860d6`, `c08662d1f3`, `073434e281` respectively. Later `09cbacd5ca` deliberately removes the unused staged model builder. Retain that explicit cleanup; do not resurrect uncalled reconstruction code. |
| `8a0c378baa` | Integrated as `0f16d16c05`; range-diff differs only in surrounding imports. Current local-input occurrence fences and owner hook remain. |
| `171766f47f` | Integrated as `03e112a593`; range-diff differs only because the protocol fixture JSON was added separately by `d0a2343f8a`. The candidate retains that fixture plus later enrollment and actor-mailbox coordination. |
| `b4c5c34792` | Integrated as `011009c688`. The Jobs implementation differs by formatting/import splitting. Current lifecycle closes admission, aborts and joins configuration jobs before Stopped, with `shutdown_joins_configuration_backend_before_participant_stopped` coverage. Retain current session and mailbox integration. |
| `863f2703de` | Superseded by `731df0dfba`, `89f6df28f6`, `5bc592923d`: non-inlined boxed construction remains, and phased preparation now returns an erased initialization continuation before polling it. Current task-context/cancellation tests preserve the original intent and cover the stronger phase boundary. |
| `53e3400571` | Saved files are before/after hook inventories, not extra runtime code. Their added destination-preparer hook and joined-job lifecycle hooks are present in the current core. Retain the inventories in archive history as evidence. |
| `3047bfa2b7` | Saved hook inventory, one-off host-hook generation script and development handoff JSON. Current AuthManager already contains the real context-host field and forwarding hooks; the recorded source checkpoints are in main. Do not ship/run the one-off text-replacement script. The JSON records unfinished qualification and SourceV1=false; it grants no capability activation or test-pass claim. |

No archived draft enables SourceV1 merely by being reachable. Qualification and
production capability activation remain explicit, separate requirements.
