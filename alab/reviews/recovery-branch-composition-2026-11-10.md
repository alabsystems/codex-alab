<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Recovery branch composition

Reviewed `alab/recovery-main-20261022T2152` at
`bf40a8465b0ffd91ce4081e8a3501ecbd5315772` against candidate `f7d83e9db2`.
The 27 absent commits contain 13 non-merge topic commits. Their intended
behaviors already have evolved implementations in current source. This merge
retains those implementations; it does not activate the older snapshots.
Qualification remains pending and this review is not a test result.

| Original commits | Intended behavior | Current implementation and regression evidence to run |
| --- | --- | --- |
| `111fc5d3ec`, `e83818ac35` | A superseded wait cannot install its timer after native cancellation. | `extension.rs` schedules announced waits through `schedule_decided_act` with decision authority; timer generation and native point checks reject stale work. `waits_shown_tests.rs` covers native starts and reported waits. Preserve current wake origins and typed waiting records. |
| `3097001fe0` | Upgrade rollback retains the exact selected thread's owed act. | `UpgradeHandoff::drop` checks runtime pointer identity, accepted turns, original point and timer generation. `take_owed_act_carrying` installs rollback identity before cancellation. `upgrade_handoff_tests.rs` includes native/policy changes and replacement runtimes. |
| `c33e874dac` | Same-mode retry-policy changes reconsider existing waits; attention retires them. | `watch_claims` reevaluates live threads on semantic policy changes; `retire_decided_wait` removes old automatic acts. `policy_wake_tests::retry_settings_reconsider_an_armed_wait_in_the_same_mode_and_under_hold` exercises both retry and entitlement settings, held and unheld, and restoring the exact original deadline. |
| `347368ad06`, `36a855fd25` | Recognize the exact additional organization quota alias and attribute its hook. | `capacity::streamed_quota_alias` and `only_the_exact_additional_quota_code_changes_when_active`; `responses_error_quota_tests.rs` and marked codex-api hook. Retain current typed provider classification. |
| `c39dd8927d` | Metadata notifications do not imply provider recovery. | `retry_notice.rs` retains the notice for five metadata notification variants only while the enhanced interface is active. `retry_notice_tests.rs` distinguishes enhanced, inactive, real progress and terminal paths. Preserve this explicit inactive-interface boundary. |
| `1698563363`, `f815c479c1`, `683d9d3af8` | Reconnect and metadata-only resume replay an actual live capacity wait. | `capacity_status_replay.rs`, its unit tests and `capacity_reconnect_integration_tests.rs`. `LiveCapacityWait` checks generation and deadline synchronously and retains typed capacity reason; replacement-guard tests reject stale cleanup/update. |
| `e9c48c8030` | Overflow delivery retains event order. | `thread_event_fifo.rs` owns one worker and a generation-scoped pending queue. Existing FIFO tests cover ordering, channel identity, retirement and failure. Do not restore older per-event sends. |
| `003641a87d` | Format the pending sources. | Formatting is superseded by current source, not a behavior to replay. |
| `5512f31595` | Restore with registry-before-generation lock order, without recursive registry lookup. | `UpgradeHandoff::drop` takes `threads` before `shared.cancel_upgrade`, then uses `put_back_from` and `sync_record_owed_from` under that custody. This also retains newer runtime identity, timer generation and relaunch-signal checks absent from the old branch. Current resume/upgrade race regressions must pass in the combined gate. |

The merge records the explicit supersession decision rather than claiming that
ancestry itself proves any behavior or qualification. All listed paths are
relative to `codex-rs/alab-autopilot/src`, `codex-rs/alab-core/{src,glue}` or
`codex-rs/alab-tui/glue` as appropriate.
