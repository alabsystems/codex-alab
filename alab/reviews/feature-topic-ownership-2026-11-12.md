<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Assign existing unowned hook topics

The ownership check for the new MCP diagnostic hook exposed five existing
unmapped topics, covering 22 upstream marker occurrences. Assign them to the
existing contracts that describe their implemented behavior:

| Topic | Feature owner | Reason |
| --- | --- | --- |
| `daybreak` | `provider-recovery` | Bounded automatic safety recovery revalidates thread and execution authority. |
| `daybreak-discovery` | `model-discovery` | Explicit selection refuses a failed catalog discovery. |
| `goal-clock` | `exec-handoff` | Native accounting baseline data survives admitted transfer. |
| `update` | `build-and-adoption` | Immutable ALab identity leaves installation with atpkg across release numbering. |
| `upgrade-input` | `exec-handoff` | One joined reader retains input, parser state, quarantine, and startup replay. |

The inventory also names the relevant implementation paths and adds the
corresponding invariants and scenarios. The Daybreak recovery mapping covers
the ALab recovery adapters, not ownership of the upstream classifier. The
discovery adapter currently lives in the marked `exec/src/daybreak.rs` source;
the inventory records that actual location.

The existing validator reports complete ownership for 79 tracked upstream
hook topics across 11 features, with no structural errors. The five repaired
topics have respectively 11, 1, 2, 3, and 5 marker occurrences. The validator's
existing unit test also passes. The new `test-diagnostics` topic was assigned
to `qualification-fixtures` in its own implementation commit.

This checkout has no `upstream` remote, so `delta.sh` and the inventory command
that invokes it remain unavailable. Validation used the same validator
function against tracked working-tree `ALAB(...)` markers with the normal
ALab-owned-path exclusions. The result is retained in
`$HOME/.aterm/.ubd/codex-feature-ownership-repair-20261112.json`.

This repairs ownership metadata. It introduces no runtime behavior and makes
no new claim that the mapped scenarios have been semantically qualified.

## Scope of the reset invariant

Review of the completed mapping found that the existing provider-recovery
invariant required reset eligibility for every foreground retry. That was too
broad for the newly mapped Daybreak safety recovery: it accepts an eligible
server-offered treatment, rechecks current-thread authority, and enforces a retry
cap without redeeming a quota reset.

The invariant now says: "Reset-credit redemption requires verified reset
eligibility." The other recovery invariants continue to cover authority,
cancellation, pacing, and bounded retries. This wording follows the separate
eligibility check in `alab-quota-recovery/src/unix.rs` and the safety-retry guards
in `alab-tui/glue/chat_hand.rs` and `tui/src/app/safety_buffering.rs`; it changes
no code or recovery behavior.

The direct inventory validation still covers 79 topics and 11 features without
errors, and its existing validator test passes. Canonical inventory generation
still requires the absent upstream remote, as recorded above.

The subsequent [local topic-inventory change](local-topic-inventory-2026-11-12.md)
removes that unnecessary dependency from `--topics`. The normal inventory CLI
now validates all 79 topics here without a remote; history-based delta reports
retain their upstream requirement.
