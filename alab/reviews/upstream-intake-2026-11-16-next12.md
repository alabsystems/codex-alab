<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream review: twelve commits after a46c5be77e

This is a source review and a declaration of integration requirements. No Rust
tests, application certification, packaging or installed adoption ran for this
batch. The decisions are in
`alab/upstream/347143f140-from-a46c5be77e-intake.json`.

The incremental range is `a46c5be77eac0dc856f677a099a6a7702edd578b` through
`347143f140109e2736f788a2f70e6ef1216c498e`. It follows the existing 82-commit
review from ALab base `05930161537a4b46adb80aa2930d11bb641f3940`; it does not
replace that batch or change the eventual integration base. The staged source
import still ends at `d17887a8d1a7d3e800fb9fb4cb9dd419c147910b` and remains
unqualified. The next commit, `f5fb19fc4a45`, is outside this review.

The original source inspection used ALab `1b28ce62d57b`. Revalidation at
`610696ddc177d4725ac598295b62decd5fb3f5ce` found no intervening change to
`upgrade_submission.rs`, `exec_model_context.rs`, the packaged defaults or
`folder_trust.rs`. Owned code still uses `Sender<Submission>` and has no
`TimeProvider`, `TimeFuture`, `CurrentTimeRead` or `current_time().await` match.
The review does not retire any ALab behavior as equivalent.

| Commit | Decision and reason | Requirements before qualification |
| --- | --- | --- |
| `f579388d5213` | Adopt unused workspace aliases and redundant clone cleanup; `WritableRoot` negates the same predicate. | Preserve the final `alab-*` member glob and owned dependency pins. Never edit upstream lock entries. |
| `9d11b38fe77f` | Adopt cell-aware table selection. Arc table identity, visual column boundaries and record identity keep drags within logical cells. | Retain turn-scoped history projection and confirmed clipboard delivery; run wrapped, empty, stacked, reverse-drag and resize controls. |
| `e40b0e62e259` | Adopt a provenance comment only. | Preserve ALab replay metadata; this wording supplies no equivalent implementation. |
| `4aac2eacc5ae` | Adopt synchronous clock reads and removal of the experimental client clock protocol. Explicit external time without a provider fails closed; host providers and asynchronous sleep remain. | Carry generated removal through the maintained generation route. Run provider/failure controls without network-skip success. ALab accounting and recovery clocks retain their separate contracts. |
| `c5b6bf1f212c` | Adopt the editable composer while new-session bootstrap waits. Submission remains disabled until the destination owns input. | Preserve native folder trust, backend and goal rebinding, old-event-tail drain, selected-widget replacement, prepared input and prompt stash. Responsiveness supplies no grant or accepted-input evidence. |
| `52ff3d1a3539` | Adopt four exact Orbit originator mappings using existing catalog/cache partitions. | Retain ALab per-originator cache naming and account-bound discovery; run isolated cache and polling controls. |
| `14813d36167e` | Keep heartbeats opt-in, with interval zero by default. The stream adapter adds no worker task. | Verify idle/reset behavior, rejection of heartbeat invocation data, callback non-admission and actual lease retirement. Preserve provider defaults when a nonzero interval is selected. No current owned struct-literal adaptation was found. |
| `826b6e8199e6` | Adopt complete, warning-free cloud catalog reuse for matching auth and Apps resource identity, including empty results. | Retain partial-result retry/backoff, in-flight identity checks and actual account retirement. The pre-existing same-turn auth fast path is not a guarantee of same-turn resource invalidation. Packaged apps/plugins/tool suggestions remain off. |
| `42f950cd112a` | Adopt account/product/backend-scoped recommendation caching and one exact candidate snapshot per step. One-second reuse and the 250 ms marker deadline include authentication. | Run cache-clear, cancellation, failed-refresh retry and step/install-snapshot controls. Old initializers cannot republish after clear. Cached recommendations confer no install authority; disabled-worker controls remain required. |
| `0276c8143d4e` | Adopt Windows sandbox-service release intent. Both changed upstream recipe paths are wholly removed by ALab policy. | Reapply `removed-paths`. This adds no payload to the current macOS package; an authorized Windows shipping lane needs its own real service qualification. |
| `1c76c0bc088f` | Adapt owned transport to upstream `Box<Submission>` channels. Reserved channel slots become pointer-sized, with an allocation per submitted operation; no performance measurement was made. | Update `upgrade_submission.rs` channel/new/redirect/test conversions and send boundaries. Retain the entire original admission vector, residency, delivery/reply and trace through forwarding, cancellation and drop. Dequeue alone retains custody. Logical deferred state may remain unboxed after upstream's receiver unwrap. |
| `347143f14010` | Adapt supported execution capture/restore to the active context store's new value-to-`ResponseItemId` correspondence. | Execute supported handoff followed by local and prefix/remote compaction: active context exactly once, stale/removed values absent, queued updates retained. Preserve atomic prefix generation/length/auth-owner checks and suffix/world-state checkpoints. This is a source-derived continuity question, not a reproduced production loss. |

The full-capsule/all-root SourceV1 receiver route remains held. No route may be
enabled to obtain a preservation test result. The build-script verification
refusal recorded in `trust27009-adoption-build-script-refusal-2026-11-16.md`
still prevents the compiler-pin adoption and application gate. Neither intake
coverage nor these declared controls waives that requirement.

Validation of the declarations used the maintained `upstream_intake.py` against
the exact range and current feature catalog: twelve commits, no source/schema
errors and no overlap with active committed intake records. All 34 named test
candidates were found in the target upstream source or the observed ALab source;
that check is source inspection, not test discovery or execution. The individual
record entries retain exact patch hashes. Their canonical byte representation is
`git -c color.ui=false show --format=fuller --no-ext-diff --no-renames --binary SHA`.

The maintained `upstream_status.py` assessment over the original ALab base and
these two explicit intake files also passed: 94 commits reviewed, zero pending,
missing or duplicate/error entries. Both candidate and origin main lack the
review target, and the report leaves qualification and adoption unproven. This
is combined declaration coverage, not admission of a source containing the range.

Before any actual integration, recompute committed admission against its real
ALab base, merge current main, implement the two adaptations, bind the final
source's affected-feature scope and run its actual required scenarios and the
unchanged native gate. No completed feature-evidence bundle exists for this batch.
