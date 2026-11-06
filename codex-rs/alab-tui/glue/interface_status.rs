// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native-state adapter for the optional Codex Alab status line and account inspector.
//!
//! Compiled as `chatwidget::status_surfaces::alab_interface` so the presentation
//! crate needs no dependency on Codex's private widgets or account types. The
//! connected server's account display and authoritative account-usage snapshots
//! stay the only source of identity and allowance. No credentials are read here.

use super::ChatWidget;
use super::StatusAccountDisplay;
use super::StatusLineItem;
use chrono::DateTime;
use chrono::Duration;
use chrono::Local;
use codex_alab_tui::interface;
use codex_alab_tui::interface::InterfaceStatus;

#[cfg(test)]
#[path = "service_tier_tests.rs"]
mod service_tier_tests;

#[path = "account_details.rs"]
mod account_details;

pub(super) fn needs_branch() -> bool {
    interface::enabled()
}

impl ChatWidget {
    pub(crate) fn alab_model_with_service_tier(&self) -> String {
        codex_alab_tui::service_tier::model_name(
            self.model_display_name(),
            self.processing_tier_display_name(),
        )
    }

    /// A session header keeps its saved model paired with its saved reasoning,
    /// which can differ from the currently active collaboration model.
    pub(crate) fn alab_session_model_with_service_tier(
        &self,
        model: &str,
        tier: Option<&str>,
    ) -> String {
        let label = tier
            .filter(|id| self.config.features.service_tier_enabled(id))
            .and_then(|id| {
                self.model_catalog
                    .models
                    .iter()
                    .find(|preset| preset.model == model)
                    .and_then(|preset| preset.service_tiers.iter().find(|tier| tier.id == id))
                    .map(|tier| codex_alab_tui::service_tier::display_name(&tier.id, &tier.name))
            });
        codex_alab_tui::service_tier::model_name(self.model_catalog.display_name(model), label)
    }

    fn alab_interface_can_publish(&self) -> bool {
        interface::active()
            && !self
                .app_event_tx
                .voice_only
                .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(in crate::chatwidget) fn alab_configure_interface(&self) {
        if interface::available() {
            let enabled = self
                .config
                .config_layer_stack
                .layers_high_to_low()
                .filter(|layer| !layer.is_disabled())
                .find_map(|layer| layer.config.get("tui")?.get(interface::SETTING)?.as_bool())
                .unwrap_or(true);
            interface::configure(enabled);
        }
    }

    /// Keep a current snapshot while the presentation is off so toggling on
    /// never resurrects a prior account or relies on a later network poll.
    pub(in crate::chatwidget) fn alab_refresh_interface(&mut self) {
        self.alab_refresh_interface_at(Local::now());
    }

    fn alab_refresh_interface_at(&mut self, now: DateTime<Local>) {
        if self.alab_interface_can_publish() {
            let status = self.alab_interface_status_at(now);
            interface::set_status(status);
            self.alab_refresh_account_details();
            self.alab_schedule_quota_expiry_at(now);
        }
    }

    /// A disconnected server has no periodic reads. Expire only the displayed
    /// snapshot locally; native quota caches and recovery decisions remain intact.
    pub(in crate::chatwidget) fn alab_refresh_quota_freshness(&mut self) {
        self.alab_refresh_quota_freshness_at(Local::now());
    }

    fn alab_refresh_quota_freshness_at(&mut self, now: DateTime<Local>) {
        if !interface::enabled() || !self.alab_interface_can_publish() {
            return;
        }
        let snapshot_stale = self
            .rate_limit_snapshots_by_limit_id
            .get("codex")
            .is_some_and(|snapshot| {
                now.signed_duration_since(snapshot.captured_at)
                    > Duration::minutes(crate::status::RATE_LIMIT_STALE_THRESHOLD_MINUTES)
            });
        let stale = snapshot_stale
            || self
                .alab_allowance
                .weekly_resets_at
                .is_some_and(|reset| reset <= now.timestamp());
        let five_hour_stale = snapshot_stale
            || self
                .alab_allowance
                .five_hour_resets_at
                .is_some_and(|reset| reset <= now.timestamp());
        let resets = self
            .alab_allowance
            .resets_at(now.timestamp(), self.clock_format);
        if interface::status_snapshot().is_some_and(|status| {
            (stale && !status.usage_stale)
                || (five_hour_stale && !status.five_hour_stale)
                || status.reset_credits != resets
        }) {
            self.alab_refresh_interface_at(now);
        } else {
            // The native scheduler coalesces deadlines. An earlier frame must
            // re-arm this later one, just as native thread-usage settlement does.
            self.alab_schedule_quota_expiry_at(now);
        }
    }

    fn alab_schedule_quota_expiry_at(&self, now: DateTime<Local>) {
        if !interface::enabled() || !self.alab_interface_can_publish() {
            return;
        }
        let quota_expiry = self
            .rate_limit_snapshots_by_limit_id
            .get("codex")
            .map(|snapshot| {
                snapshot.captured_at
                    + Duration::minutes(crate::status::RATE_LIMIT_STALE_THRESHOLD_MINUTES)
                    + Duration::milliseconds(1)
            });
        let reset_expiry = self
            .alab_allowance
            .next_reset_change(now.timestamp())
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
            .map(|deadline| deadline.with_timezone(&Local));
        let weekly_reset = self
            .alab_allowance
            .weekly_resets_at
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
            .map(|deadline| deadline.with_timezone(&Local));
        let five_hour_reset = self
            .alab_allowance
            .five_hour_resets_at
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
            .map(|deadline| deadline.with_timezone(&Local));
        if let Some(expires_at) = quota_expiry
            .into_iter()
            .chain(reset_expiry)
            .chain(weekly_reset)
            .chain(five_hour_reset)
            .filter(|at| *at > now)
            .min()
            && let Ok(delay) = (expires_at - now).to_std()
            && !delay.is_zero()
        {
            self.frame_requester.schedule_frame_in(delay);
        }
    }

    pub(in crate::chatwidget) fn alab_interface_email_loaded(&mut self) {
        if self.alab_interface_can_publish() {
            self.alab_refresh_interface();
            self.request_redraw();
        }
    }

    /// Notifications are the identity boundary, including same-plan switches.
    /// Native request/generation checks reject old in-flight usage responses;
    /// remove the previously accepted snapshot here before the new email shows.
    pub(in crate::chatwidget) fn alab_interface_account_changed(&mut self) {
        if interface::available() {
            codex_alab_tui::session::shared().quota().invalidate();
            self.alab_allowance.reset();
            self.rate_limit_snapshots_by_limit_id.clear();
            if self.alab_interface_can_publish() {
                interface::set_status(InterfaceStatus {
                    account: "Account unavailable".to_string(),
                    ..Default::default()
                });
                self.alab_refresh_account_details();
            }
        }
    }

    fn alab_interface_status_at(&mut self, now: DateTime<Local>) -> InterfaceStatus {
        let account = match self.status_account_display.as_ref() {
            Some(StatusAccountDisplay::ChatGpt {
                email: Some(email), ..
            }) if !email.trim().is_empty() => email.clone(),
            Some(StatusAccountDisplay::ApiKey) => "API key".to_string(),
            _ => "Account unavailable".to_string(),
        };
        let plan = match self.status_account_display.as_ref() {
            Some(StatusAccountDisplay::ChatGpt { plan, .. }) => self
                .alab_allowance
                .observed_plan
                .or(self.plan_type)
                .map(crate::status::plan_type_display_name)
                .or_else(|| plan.clone()),
            _ => None,
        };
        // Native /statusline also supports legacy unknown-duration windows.
        // ALab's fields explicitly say weekly and 5h, so require those durations
        // instead of relabeling unrelated allowances.
        let snapshot = self.rate_limit_snapshots_by_limit_id.get("codex");
        // A failed periodic read retains the native cache for recovery decisions.
        // Age only its presentation, using the same strict boundary as /status.
        let snapshot_stale = snapshot.is_some_and(|snapshot| {
            now.signed_duration_since(snapshot.captured_at)
                > Duration::minutes(crate::status::RATE_LIMIT_STALE_THRESHOLD_MINUTES)
        });
        let usage_stale = snapshot_stale
            || self
                .alab_allowance
                .weekly_resets_at
                .is_some_and(|reset| reset <= now.timestamp());
        let weekly = snapshot
            .and_then(super::weekly_status_window)
            .map(|(window, _)| window)
            .filter(|window| super::matches_window_label(window, "weekly"));
        let five_hour_stale = snapshot_stale
            || self
                .alab_allowance
                .five_hour_resets_at
                .is_some_and(|reset| reset <= now.timestamp());
        let five_hour = snapshot
            .and_then(super::five_hour_status_window)
            .map(|(window, _)| window)
            .filter(|window| super::matches_window_label(window, "5h"));
        let weekly_reset = weekly.and_then(|window| window.resets_at.clone());
        let five_hour_reset = five_hour.and_then(|window| window.resets_at.clone());
        let usage_checked_at = snapshot.map(|snapshot| {
            snapshot
                .captured_at
                .format(self.clock_format.date_time_format())
                .to_string()
        });
        let has_weekly = weekly.is_some();
        let weekly_used_percent = weekly.map(|window| window.used_percent);
        let has_five_hour = five_hour.is_some();
        let permissions = self.status_line_value_for_item(StatusLineItem::Permissions);
        let named_permission_profile = self
            .config
            .permissions
            .active_permission_profile()
            .is_some_and(|profile| !profile.id.starts_with(':'));
        let collaboration_mode = self
            .collaboration_mode_label()
            .filter(|mode| *mode != "Default" || named_permission_profile);
        let mode = match (collaboration_mode, permissions) {
            (Some(mode), Some(permissions)) => Some(format!("{mode} / {permissions}")),
            (Some(mode), None) => Some(mode.to_string()),
            (None, permissions) => permissions,
        };
        let model = self
            .status_line_value_for_item(StatusLineItem::ModelWithReasoning)
            .map(|label| {
                if self.reasoning_display_name() == "default" {
                    let prefix = format!("{} default", self.model_display_name());
                    if let Some(tier) = label.strip_prefix(&prefix) {
                        return format!("{}{tier}", self.model_display_name());
                    }
                }
                label
            });
        InterfaceStatus {
            account,
            plan,
            weekly: has_weekly
                .then(|| {
                    if usage_stale {
                        Some("weekly stale".to_string())
                    } else {
                        self.status_line_value_for_item(StatusLineItem::WeeklyLimit)
                    }
                })
                .flatten(),
            weekly_reset,
            five_hour: has_five_hour
                .then(|| {
                    if five_hour_stale {
                        Some("5h stale".to_string())
                    } else {
                        self.status_line_value_for_item(StatusLineItem::FiveHourLimit)
                    }
                })
                .flatten(),
            five_hour_reset,
            usage_checked_at,
            usage_stale,
            five_hour_stale,
            quota: (has_weekly && !usage_stale)
                .then(|| {
                    self.alab_allowance
                        .weekly
                        .as_ref()
                        .filter(|recorded| {
                            weekly_used_percent.is_some_and(|used| used == recorded.used_percent)
                        })
                        .map(|weekly| weekly.at(now.timestamp()))
                })
                .flatten(),
            reset_credits: self
                .alab_allowance
                .resets_at(now.timestamp(), self.clock_format),
            branch: self.status_line_value_for_item(StatusLineItem::GitBranch),
            model,
            activity: self.status_line_value_for_item(StatusLineItem::Status),
            context: self
                .status_line_context_used_percent()
                .map(|used| format!("ctx {used}%")),
            mode,
        }
    }
}

#[cfg(test)]
impl ChatWidget {
    /// Age an accepted native snapshot without sleeping or replacing its recovery metadata.
    pub(crate) fn alab_set_usage_captured_at_for_test(&mut self, captured_at: DateTime<Local>) {
        self.rate_limit_snapshots_by_limit_id
            .get_mut("codex")
            .expect("accepted native account usage")
            .captured_at = captured_at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
    use chrono::TimeZone;
    use codex_app_server_protocol::RateLimitSnapshot;

    fn usage(used_percent: i32) -> RateLimitSnapshot {
        serde_json::from_value(serde_json::json!({
            "limitId": "codex",
            "primary": { "usedPercent": 10, "windowDurationMins": 300 },
            "secondary": { "usedPercent": used_percent, "windowDurationMins": 10_080 },
        }))
        .expect("account usage snapshot")
    }

    fn account(email: Option<&str>) -> Option<StatusAccountDisplay> {
        Some(StatusAccountDisplay::ChatGpt {
            email: email.map(str::to_string),
            plan: Some("Pro".to_string()),
            workspace: None,
        })
    }

    #[tokio::test]
    async fn fresh_provider_plan_supersedes_the_sign_in_label() {
        use codex_protocol::account::PlanType;
        let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
        chat.status_account_display = account(Some("owner@example.com"));
        chat.plan_type = Some(PlanType::Pro);
        let mut snapshot = usage(10);
        snapshot.plan_type = Some(serde_json::from_value(serde_json::json!("promax")).unwrap());
        chat.on_rate_limit_snapshot(Some(snapshot));
        let status = chat.alab_interface_status_at(Local::now());
        assert_eq!(
            status.plan,
            Some(crate::status::plan_type_display_name(
                serde_json::from_value(serde_json::json!("promax")).unwrap(),
            ))
        );
        assert_ne!(status.plan.as_deref(), Some("Pro"));
    }

    #[tokio::test]
    async fn an_obsolete_reset_picker_cannot_replace_history_or_reset_dots_beside_native_usage() {
        use crate::app_event::RateLimitRefreshOrigin;
        use codex_alab_tui::quota;
        use codex_app_server_protocol::GetAccountRateLimitsResponse;

        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("verified@example.test")), None, true, true);
        let now = Local.timestamp_opt(1_800_000_000, 0).single().unwrap();
        let original_reset = now.timestamp() + 3 * 86_400;
        let replacement_reset = now.timestamp() + 2 * 86_400;
        let reading = |used: i32, reset: i64, count: i64| -> GetAccountRateLimitsResponse {
            serde_json::from_value(serde_json::json!({
                "accountId": "selected-account",
                "rateLimits": {
                    "limitId": "codex",
                    "primary": { "usedPercent": 18, "windowDurationMins": 300 },
                    "secondary": { "usedPercent": used, "windowDurationMins": 10_080, "resetsAt": reset }
                },
                "rateLimitResetCredits": { "availableCount": count, "credits": [
                    { "id": "soon", "resetType": "codexRateLimits", "status": "available", "grantedAt": now.timestamp() - 86_400, "expiresAt": now.timestamp() + 3600 },
                    { "id": "later", "resetType": "codexRateLimits", "status": "available", "grantedAt": now.timestamp() - 86_400, "expiresAt": now.timestamp() + 4 * 86_400 }
                ] }
            }))
            .unwrap()
        };
        for (age, used) in [(1800, 23), (900, 25), (0, 27)] {
            let captured = now - Duration::seconds(age);
            let response = reading(used, original_reset, 2);
            chat.alab_account_usage_read_for_origin_at(
                &response,
                RateLimitRefreshOrigin::Periodic,
                captured.timestamp(),
            );
            chat.on_rate_limit_snapshot(Some(response.rate_limits));
            chat.alab_set_usage_captured_at_for_test(captured);
        }
        let before = chat.alab_interface_status_at(now);
        assert!(before.quota.as_ref().unwrap().projection.is_some());
        assert_eq!(before.reset_credits.as_ref().unwrap().available_count, 2);
        assert_eq!(before.reset_credits.as_ref().unwrap().expiring_count, 1);
        let store = quota::Store::new(&chat.local_settings.codex_home);
        let stored_before = store
            .read("account:selected-account", now.timestamp())
            .unwrap();

        let obsolete = chat.show_rate_limit_reset_loading_popup();
        let latest = chat.show_rate_limit_reset_loading_popup();
        let response = reading(27, replacement_reset, 0);
        // This response passed the app's account/generation/read-order check,
        // but the native picker has already been replaced by the next request.
        chat.alab_account_usage_read_for_origin_at(
            &response,
            RateLimitRefreshOrigin::ResetPicker {
                request_id: obsolete,
            },
            now.timestamp() + 1,
        );
        assert!(!chat.finish_rate_limit_reset_credits_refresh(
            obsolete,
            vec![response.rate_limits],
            Ok(response.rate_limit_reset_credits.unwrap()),
        ));
        let retained = chat.alab_interface_status_at(now + Duration::seconds(1));
        assert_eq!(retained.weekly_reset, before.weekly_reset);
        assert_eq!(retained.quota, before.quota);
        assert_eq!(retained.reset_credits, before.reset_credits);
        assert_eq!(
            store
                .read("account:selected-account", now.timestamp() + 1)
                .unwrap(),
            stored_before,
            "an obsolete response cannot replace the current cycle on disk"
        );

        let response = reading(27, replacement_reset, 1);
        chat.alab_account_usage_read_for_origin_at(
            &response,
            RateLimitRefreshOrigin::ResetPicker { request_id: latest },
            now.timestamp() + 2,
        );
        assert!(chat.finish_rate_limit_reset_credits_refresh(
            latest,
            vec![response.rate_limits],
            Ok(response.rate_limit_reset_credits.unwrap()),
        ));
        chat.alab_set_usage_captured_at_for_test(now + Duration::seconds(2));
        let refreshed = chat.alab_interface_status_at(now + Duration::seconds(2));
        let quota = refreshed.quota.as_ref().unwrap();
        assert_eq!(quota.resets_at, replacement_reset);
        assert_eq!(quota.sample_count, 1);
        assert!(quota.projection.is_none());
        assert_ne!(refreshed.weekly_reset, before.weekly_reset);
        assert_eq!(refreshed.reset_credits.as_ref().unwrap().available_count, 1);
        assert_eq!(
            store
                .read("account:selected-account", now.timestamp() + 2)
                .unwrap()
                .as_ref(),
            refreshed.quota.as_ref(),
            "the latest picker updates native presentation and durable history together"
        );
    }

    #[tokio::test]
    async fn automatic_reset_deadline_expires_displayed_usage_without_authorizing_recovery() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("verified@example.test")), None, true, true);
        while ops.try_recv().is_ok() {}
        let now = Local.timestamp_opt(1_800_000_000, 0).single().unwrap();
        let reset = now + Duration::seconds(1);
        let response: codex_app_server_protocol::GetAccountRateLimitsResponse =
            serde_json::from_value(serde_json::json!({
                "accountId": "selected-account",
                "ordinaryUsageAllowed": false,
                "rateLimits": {
                    "limitId": "codex",
                    "primary": { "usedPercent": 18, "windowDurationMins": 300, "resetsAt": reset.timestamp() + 3600 },
                    "secondary": { "usedPercent": 27, "windowDurationMins": 10_080, "resetsAt": reset.timestamp() }
                }
            })).unwrap();
        chat.alab_account_usage_read_at(&response, now.timestamp());
        chat.on_rate_limit_snapshot(Some(response.rate_limits));
        chat.alab_set_usage_captured_at_for_test(now);
        let before = chat.alab_interface_status_at(now);
        assert!(!before.usage_stale);
        assert!(before.quota.is_some());
        interface::set_status(before);
        chat.alab_refresh_quota_freshness_at(reset);
        let after = interface::status_snapshot().unwrap();
        assert!(after.usage_stale);
        assert!(
            after.quota.is_none(),
            "the prior cycle cannot retain a usage bar or forecast"
        );
        assert_eq!(after.weekly.as_deref(), Some("weekly stale"));
        let native = chat.rate_limit_snapshots_by_limit_id.get("codex").unwrap();
        assert_eq!(
            super::super::weekly_status_window(native)
                .unwrap()
                .0
                .used_percent,
            27.0
        );
        assert!(
            ops.try_recv().is_err(),
            "a display deadline cannot resume model work"
        );
    }

    #[tokio::test]
    async fn a_weekly_rollover_does_not_expire_a_fresh_short_window() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let now = Local.timestamp_opt(1_800_000_000, 0).single().unwrap();
        for (case, weekly_after, short_after, elapsed, weekly_stale, short_stale) in [
            ("short reset", 3600, 1, Duration::seconds(1), false, true),
            (
                "snapshot age",
                3600,
                3600,
                Duration::minutes(crate::status::RATE_LIMIT_STALE_THRESHOLD_MINUTES)
                    + Duration::milliseconds(1),
                true,
                true,
            ),
            ("weekly reset", 1, 3600, Duration::seconds(1), true, false),
        ] {
            let (mut chat, _tx, _rx, mut ops) = make_chatwidget_manual_with_sender().await;
            chat.update_account_state(account(Some("verified@example.test")), None, true, true);
            let response: codex_app_server_protocol::GetAccountRateLimitsResponse =
                serde_json::from_value(serde_json::json!({
                    "accountId": "selected-account",
                    "ordinaryUsageAllowed": false,
                    "rateLimits": {
                        "limitId": "codex",
                        "primary": { "usedPercent": 18, "windowDurationMins": 300, "resetsAt": now.timestamp() + short_after },
                        "secondary": { "usedPercent": 27, "windowDurationMins": 10_080, "resetsAt": now.timestamp() + weekly_after }
                    }
                })).unwrap();
            chat.alab_account_usage_read_at(&response, now.timestamp());
            chat.on_rate_limit_snapshot(Some(response.rate_limits));
            chat.alab_set_usage_captured_at_for_test(now);
            chat.codex_spend_control_reached = Some(true);
            chat.hold_rate_limit_recovery();
            while ops.try_recv().is_ok() {}
            let before = chat.alab_interface_status_at(now);
            assert!(!before.usage_stale, "{case}");
            assert!(!before.five_hour_stale, "{case}");
            assert!(before.quota.is_some(), "{case}");
            interface::set_status(before.clone());

            chat.alab_refresh_quota_freshness_at(now + elapsed);

            let after = interface::status_snapshot().unwrap();
            assert_eq!(after.usage_stale, weekly_stale, "{case}");
            assert_eq!(after.quota.is_none(), weekly_stale, "{case}");
            assert_eq!(after.five_hour_stale, short_stale, "{case}");
            assert_eq!(
                after.five_hour.as_deref(),
                if short_stale {
                    Some("5h stale")
                } else {
                    before.five_hour.as_deref()
                },
                "{case}"
            );
            let native = &chat.rate_limit_snapshots_by_limit_id["codex"];
            assert_eq!(native.captured_at, now, "{case}");
            assert_eq!(
                native.primary.as_ref().unwrap().used_percent,
                18.0,
                "{case}"
            );
            assert_eq!(
                native.secondary.as_ref().unwrap().used_percent,
                27.0,
                "{case}"
            );
            assert_eq!(chat.codex_spend_control_reached, Some(true), "{case}");
            assert!(chat.input_queue.rate_limit_recovery_pending, "{case}");
            assert!(ops.try_recv().is_err(), "{case}");
        }
    }

    async fn short_reset_display(primary: bool, resets_after: Option<i64>) {
        use codex_alab_tui::banner::Depth;
        use codex_alab_tui::banner::Theme;
        use ratatui::buffer::Buffer;
        use ratatui::buffer::Cell;
        use ratatui::layout::Rect;

        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("verified@example.test")), None, true, true);
        chat.alab_open_account_details();
        let now = Local.timestamp_opt(1_800_000_000, 0).single().unwrap();
        for (age, used) in [(1800, 23), (900, 25), (0, 27)] {
            let captured = now - Duration::seconds(age);
            let mut response: codex_app_server_protocol::GetAccountRateLimitsResponse =
                serde_json::from_value(serde_json::json!({
                    "accountId": "selected-account",
                    "ordinaryUsageAllowed": false,
                    "rateLimits": {
                        "limitId": "codex",
                        "primary": { "usedPercent": 18, "windowDurationMins": 300, "resetsAt": resets_after.map(|after| now.timestamp() + after) },
                        "secondary": { "usedPercent": used, "windowDurationMins": 10_080, "resetsAt": now.timestamp() + 3 * 86_400 }
                    }
                })).unwrap();
            if !primary {
                std::mem::swap(
                    &mut response.rate_limits.primary,
                    &mut response.rate_limits.secondary,
                );
            }
            chat.alab_account_usage_read_at(&response, captured.timestamp());
            chat.on_rate_limit_snapshot(Some(response.rate_limits));
            chat.alab_set_usage_captured_at_for_test(captured);
        }
        chat.pause_for_disconnect();
        chat.reconnect_failed();
        chat.hold_rate_limit_recovery();
        let native = chat.rate_limit_snapshots_by_limit_id["codex"].clone();
        while events.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let (requester, mut frames) = crate::tui::FrameRequester::test_channel();
        chat.frame_requester = requester;
        let scheduled_before = std::time::Instant::now();
        chat.alab_refresh_interface_at(now);
        let scheduled_after = std::time::Instant::now();
        // Refreshing the open inspector also asks for an immediate draw. Inspect
        // the future deadline separately from that intentional repaint.
        let scheduled = std::iter::from_fn(|| frames.try_recv().ok())
            .max()
            .expect("a local display deadline");
        let before = interface::status_snapshot().unwrap();
        assert!(!before.usage_stale);
        assert_eq!(before.quota.as_ref().unwrap().sample_count, 3);
        assert!(before.quota.as_ref().unwrap().projection.is_some());
        if resets_after.is_none_or(|after| after > 0) {
            assert_eq!(before.five_hour.as_deref(), Some("5h 82% left"));
        }

        let elapsed = resets_after.unwrap_or(60).max(0);
        let boundary = now + Duration::seconds(elapsed);
        while frames.try_recv().is_ok() {}
        chat.alab_refresh_quota_freshness_at(boundary);
        let after = interface::status_snapshot().unwrap();
        let expired = resets_after.is_some();
        let area = Rect::new(0, 0, 240, 1);
        let mut buffer = Buffer::empty(area);
        interface::render_status_current(area, &mut buffer, Depth::TrueColor, Theme::default());
        let footer: String = buffer.content.iter().map(Cell::symbol).collect();
        let details = crate::chatwidget::tests::render_bottom_popup(&chat, 100);
        if expired {
            assert!(footer.contains("5h stale"), "{footer}");
            assert!(details.contains("5h: stale · refresh needed"), "{details}");
            assert!(!footer.contains("5h 82% left"));
        } else {
            assert!(footer.contains("5h 82% left"), "{footer}");
            assert!(!details.contains("5h: stale"), "{details}");
        }
        assert!(!after.usage_stale, "the weekly cycle is still current");
        assert_eq!(after.weekly.as_deref(), Some("weekly 73% left"));
        assert!(after.quota.as_ref().unwrap().projection.is_some());
        assert!(footer.contains("weekly ["), "{footer}");
        assert!(details.contains("Estimated at reset:"), "{details}");
        assert_eq!(
            chat.rate_limit_snapshots_by_limit_id["codex"].captured_at,
            native.captured_at
        );
        assert!(chat.input_queue.rate_limit_recovery_pending);
        assert!(
            events.try_recv().is_err(),
            "local expiry sends no refresh request"
        );
        assert!(
            ops.try_recv().is_err(),
            "local expiry authorizes no model work"
        );

        let delay = resets_after
            .filter(|after| *after > 0)
            .map_or(std::time::Duration::from_millis(900_001), |seconds| {
                std::time::Duration::from_secs(seconds as u64)
            });
        assert!(scheduled >= scheduled_before + delay);
        assert!(scheduled <= scheduled_after + delay);
        // A passed short deadline cannot become an immediate repeating frame.
        while frames.try_recv().is_ok() {}
        let next_before = std::time::Instant::now();
        chat.alab_refresh_quota_freshness_at(boundary + Duration::seconds(1));
        let next_after = std::time::Instant::now();
        let next = frames
            .try_recv()
            .expect("the still-current weekly reading can age");
        let remaining = std::time::Duration::from_millis(900_001 - ((elapsed + 1) as u64 * 1000));
        assert!(next >= next_before + remaining);
        assert!(next <= next_after + remaining);
        assert!(frames.try_recv().is_err());

        if expired {
            let refreshed = boundary + Duration::seconds(2);
            let mut response: codex_app_server_protocol::GetAccountRateLimitsResponse =
                serde_json::from_value(serde_json::json!({
                    "accountId": "selected-account",
                    "rateLimits": {
                        "limitId": "codex",
                        "primary": { "usedPercent": 3, "windowDurationMins": 300, "resetsAt": refreshed.timestamp() + 3600 },
                        "secondary": { "usedPercent": 28, "windowDurationMins": 10_080, "resetsAt": now.timestamp() + 3 * 86_400 }
                    }
                })).unwrap();
            if !primary {
                std::mem::swap(
                    &mut response.rate_limits.primary,
                    &mut response.rate_limits.secondary,
                );
            }
            chat.alab_account_usage_read_at(&response, refreshed.timestamp());
            chat.on_rate_limit_snapshot(Some(response.rate_limits));
            chat.alab_set_usage_captured_at_for_test(refreshed);
            chat.alab_refresh_interface_at(refreshed);
            assert_eq!(
                interface::status_snapshot().unwrap().five_hour.as_deref(),
                Some("5h 97% left")
            );
            let details = crate::chatwidget::tests::render_bottom_popup(&chat, 100);
            assert!(details.contains("5h 97% left"), "{details}");
            assert!(!details.contains("5h: stale"), "{details}");
        }
    }

    #[tokio::test]
    async fn a_primary_short_reset_expires_only_its_display_at_the_exact_deadline() {
        short_reset_display(true, Some(60)).await;
    }

    #[tokio::test]
    async fn a_secondary_short_reset_expires_only_its_display_at_the_exact_deadline() {
        short_reset_display(false, Some(60)).await;
    }

    #[tokio::test]
    async fn a_short_window_without_a_deadline_keeps_its_fresh_reading() {
        short_reset_display(true, None).await;
    }

    #[tokio::test]
    async fn an_already_expired_secondary_short_window_does_not_borrow_weekly_freshness() {
        short_reset_display(false, Some(-1)).await;
    }

    #[tokio::test]
    async fn parked_account_updates_keep_native_cache_boundaries_without_publishing_selected_status()
     {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut parked, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        parked.update_account_state(account(Some("parked@example.com")), None, true, true);
        parked.on_rate_limit_snapshot(Some(usage(28)));
        parked.park_voice();
        let (mut selected, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        selected.update_account_state(account(Some("selected@example.com")), None, true, true);
        selected.on_rate_limit_snapshot(Some(usage(61)));
        let current = interface::status_snapshot();

        parked.on_account_email_loaded(Some("hidden-refresh@example.com".into()), None);
        parked.on_rate_limit_snapshot(Some(usage(80)));
        assert_eq!(interface::status_snapshot(), current);
        assert_eq!(
            parked.rate_limit_snapshots_by_limit_id["codex"]
                .secondary
                .as_ref()
                .unwrap()
                .used_percent,
            80.0
        );
        parked.update_account_state(account(None), None, true, true);
        assert!(
            parked.rate_limit_snapshots_by_limit_id.is_empty(),
            "the hidden widget still clears its native account cache"
        );
        assert_eq!(
            interface::status_snapshot(),
            current,
            "an account boundary in a hidden owner cannot clear the selected identity"
        );

        interface::set_enabled(false);
        parked.on_account_email_loaded(Some("hidden-off@example.com".into()), None);
        parked.on_rate_limit_snapshot(Some(usage(99)));
        assert_eq!(interface::status_snapshot(), current);
        selected.on_account_email_loaded(Some("selected-off@example.com".into()), None);
        assert_eq!(
            interface::status_snapshot(),
            current,
            "vanilla does not publish enhanced status"
        );
        interface::set_enabled(true);
        selected.refresh_status_surfaces();
        assert_eq!(
            interface::status_snapshot().unwrap().account,
            "selected-off@example.com",
            "enabling reads the selected owner's current native state"
        );
    }

    #[tokio::test]
    async fn local_quota_expiration_rearms_coalesced_frames_and_stops_when_stale_or_off() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("person@example.com")), None, true, true);
        chat.on_rate_limit_snapshot(Some(usage(28)));
        let captured = Local::now();
        chat.alab_set_usage_captured_at_for_test(captured);
        let (requester, mut frames) = crate::tui::FrameRequester::test_channel();
        chat.frame_requester = requester;
        let before = std::time::Instant::now();
        for _ in 0..2 {
            chat.alab_refresh_quota_freshness_at(captured + Duration::minutes(14));
            let deadline = frames
                .try_recv()
                .expect("a fresh snapshot arms/re-arms local expiry");
            assert!(deadline.duration_since(before) >= std::time::Duration::from_secs(60));
            assert!(deadline.duration_since(before) < std::time::Duration::from_secs(61));
            assert!(
                frames.try_recv().is_err(),
                "no immediate or recurring redraw"
            );
        }
        let boundary =
            captured + Duration::minutes(crate::status::RATE_LIMIT_STALE_THRESHOLD_MINUTES);
        chat.alab_refresh_quota_freshness_at(boundary);
        assert!(!interface::status_snapshot().unwrap().usage_stale);
        assert!(frames.try_recv().is_ok());
        chat.alab_refresh_quota_freshness_at(boundary + Duration::milliseconds(1));
        assert!(interface::status_snapshot().unwrap().usage_stale);
        while frames.try_recv().is_ok() {}
        chat.alab_refresh_quota_freshness_at(boundary + Duration::minutes(1));
        assert!(frames.try_recv().is_err(), "stale quota releases the timer");

        chat.on_rate_limit_snapshot(Some(usage(61)));
        assert!(!interface::status_snapshot().unwrap().usage_stale);
        while frames.try_recv().is_ok() {}
        interface::set_enabled(false);
        let status = interface::status_snapshot();
        chat.alab_refresh_quota_freshness_at(boundary + Duration::minutes(1));
        assert_eq!(interface::status_snapshot(), status);
        assert!(frames.try_recv().is_err(), "native Off adds no local timer");
    }

    #[tokio::test]
    async fn native_predraw_expires_a_disconnected_open_inspector_without_network_or_recovery() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("person@example.com")), None, true, true);
        chat.on_rate_limit_snapshot(Some(usage(28)));
        chat.alab_open_account_details();
        chat.pause_for_disconnect();
        chat.reconnect_failed();
        let fresh = interface::status_snapshot().unwrap();
        assert!(!fresh.usage_stale);
        let captured = Local::now() - Duration::minutes(16);
        chat.alab_set_usage_captured_at_for_test(captured);
        chat.hold_rate_limit_recovery();
        while events.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}

        chat.pre_draw_tick();

        let stale = interface::status_snapshot().unwrap();
        assert!(stale.usage_stale);
        assert_eq!(stale.account, fresh.account);
        assert_eq!(stale.plan, fresh.plan);
        assert_eq!(stale.weekly.as_deref(), Some("weekly stale"));
        assert_eq!(stale.five_hour.as_deref(), Some("5h stale"));
        let shown = crate::chatwidget::tests::render_bottom_popup(&chat, 100);
        assert!(shown.contains("Weekly: stale · refresh needed"), "{shown}");
        assert!(shown.contains(stale.usage_checked_at.as_deref().unwrap()));
        assert_eq!(
            chat.rate_limit_snapshots_by_limit_id["codex"].captured_at,
            captured
        );
        assert!(chat.input_queue.rate_limit_recovery_pending);
        assert!(
            events.try_recv().is_err(),
            "expiration sends no server request"
        );
        assert!(
            ops.try_recv().is_err(),
            "expiration cannot release queued model work"
        );
    }

    #[tokio::test]
    async fn quota_freshness_uses_the_native_boundary_without_changing_recovery_state() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("person@example.com")), None, true, true);
        chat.on_rate_limit_snapshot(Some(usage(28)));
        let captured = Local
            .with_ymd_and_hms(2026, 10, 20, 18, 24, 0)
            .single()
            .unwrap();
        chat.alab_set_usage_captured_at_for_test(captured);
        chat.codex_spend_control_reached = Some(true);
        chat.hold_rate_limit_recovery();
        let cache = chat.rate_limit_snapshots_by_limit_id["codex"].clone();
        let boundary =
            captured + Duration::minutes(crate::status::RATE_LIMIT_STALE_THRESHOLD_MINUTES);

        let fresh = chat.alab_interface_status_at(boundary);
        assert!(
            !fresh.usage_stale,
            "the native boundary is strictly greater than 15 minutes"
        );
        assert_eq!(fresh.weekly.as_deref(), Some("weekly 72% left"));
        assert_eq!(fresh.five_hour.as_deref(), Some("5h 90% left"));
        let stale = chat.alab_interface_status_at(boundary + Duration::milliseconds(1));
        assert!(stale.usage_stale);
        assert_eq!(stale.account, fresh.account);
        assert_eq!(stale.plan, fresh.plan);
        assert_eq!(stale.usage_checked_at, fresh.usage_checked_at);
        assert_eq!(stale.weekly.as_deref(), Some("weekly stale"));
        assert_eq!(stale.five_hour.as_deref(), Some("5h stale"));
        assert_eq!(stale.weekly_reset, fresh.weekly_reset);
        assert_eq!(stale.five_hour_reset, fresh.five_hour_reset);
        let retained = &chat.rate_limit_snapshots_by_limit_id["codex"];
        assert_eq!(retained.captured_at, cache.captured_at);
        assert_eq!(retained.primary.as_ref().unwrap().used_percent, 10.0);
        assert_eq!(retained.secondary.as_ref().unwrap().used_percent, 28.0);
        assert_eq!(chat.codex_spend_control_reached, Some(true));
        assert!(chat.input_queue.rate_limit_recovery_pending);

        // Vanilla retains native state without running the enhanced publisher.
        interface::set_enabled(false);
        interface::set_status(chat.alab_interface_status_at(boundary + Duration::minutes(1)));
        assert!(!interface::enabled());
        assert!(interface::status_snapshot().unwrap().usage_stale);
        assert!(chat.input_queue.rate_limit_recovery_pending);
        chat.on_rate_limit_snapshot(Some(usage(61)));
        assert!(interface::status_snapshot().unwrap().usage_stale);
        interface::set_enabled(true);
        chat.refresh_status_surfaces();
        let refreshed = interface::status_snapshot().unwrap();
        assert!(!refreshed.usage_stale);
        assert_eq!(refreshed.weekly.as_deref(), Some("weekly 39% left"));
        assert_eq!(refreshed.five_hour.as_deref(), Some("5h 90% left"));
        assert_eq!(refreshed.account, fresh.account);
        assert_eq!(refreshed.plan, fresh.plan);
    }

    #[tokio::test]
    async fn aged_unknown_windows_are_unavailable_instead_of_mislabelled_stale() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("person@example.com")), None, true, true);
        let now = Local::now();
        assert!(!chat.alab_interface_status_at(now).usage_stale);
        let mut snapshot = usage(28);
        snapshot.primary.as_mut().unwrap().window_duration_mins = Some(30);
        snapshot.secondary.as_mut().unwrap().window_duration_mins = None;
        chat.on_rate_limit_snapshot(Some(snapshot));
        chat.alab_set_usage_captured_at_for_test(now - Duration::minutes(16));
        let unavailable = chat.alab_interface_status_at(now);
        assert!(unavailable.usage_stale);
        assert_eq!(unavailable.weekly, None);
        assert_eq!(unavailable.five_hour, None);
        assert!(unavailable.usage_checked_at.is_some());
    }

    #[tokio::test]
    async fn account_switch_clears_old_weekly_before_new_email_is_published() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("first@example.com")), None, true, true);
        chat.on_rate_limit_snapshot(Some(usage(28)));

        let first = interface::status_snapshot().expect("first account status");
        assert_eq!(first.account, "first@example.com");
        assert_eq!(first.plan.as_deref(), Some("Pro"));
        assert_eq!(first.weekly.as_deref(), Some("weekly 72% left"));

        chat.update_account_state(account(None), None, true, true);
        let loading = interface::status_snapshot().expect("identity boundary status");
        assert_eq!(loading.account, "Account unavailable");
        assert_eq!(loading.weekly, None);
        assert_eq!(loading.five_hour, None);
        assert!(chat.rate_limit_snapshots_by_limit_id.is_empty());

        chat.on_account_email_loaded(Some("second@example.com".to_string()), None);
        let second = interface::status_snapshot().expect("verified second account");
        assert_eq!(second.account, "second@example.com");
        assert_eq!(second.weekly, None);

        chat.on_rate_limit_snapshot(Some(usage(61)));
        let refreshed = interface::status_snapshot().expect("second account allowance");
        assert_eq!(refreshed.account, "second@example.com");
        assert_eq!(refreshed.weekly.as_deref(), Some("weekly 39% left"));
    }

    #[tokio::test]
    async fn native_window_selection_and_account_meter_remain_authoritative() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("person@example.com")), None, true, true);
        let mut weekly_primary = usage(35);
        std::mem::swap(&mut weekly_primary.primary, &mut weekly_primary.secondary);
        chat.on_rate_limit_snapshot(Some(weekly_primary));
        let mut other_model = usage(99);
        other_model.limit_id = Some("other-model".to_string());
        chat.on_rate_limit_snapshot(Some(other_model));
        chat.on_rolling_rate_limit_snapshot(usage(100));

        let status = interface::status_snapshot().expect("account usage status");
        assert_eq!(status.account, "person@example.com");
        assert_eq!(status.weekly.as_deref(), Some("weekly 65% left"));
        assert_eq!(status.five_hour.as_deref(), Some("5h 90% left"));

        let mut unknown_duration = usage(35);
        unknown_duration
            .secondary
            .as_mut()
            .expect("secondary window")
            .window_duration_mins = None;
        chat.on_rate_limit_snapshot(Some(unknown_duration));
        assert_eq!(
            interface::status_snapshot()
                .expect("unknown-duration allowance")
                .weekly,
            None,
            "an unknown-duration secondary meter must not be called weekly",
        );
    }

    #[tokio::test]
    async fn api_key_switch_discards_subscription_allowance_while_interface_is_off() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(false);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.update_account_state(account(Some("person@example.com")), None, true, true);
        chat.on_rate_limit_snapshot(Some(usage(35)));
        chat.update_account_state(Some(StatusAccountDisplay::ApiKey), None, false, false);

        assert!(interface::status_snapshot().is_none());
        interface::set_enabled(true);
        chat.refresh_status_surfaces();
        let status = interface::status_snapshot().expect("current API key status after enabling");
        assert_eq!(status.account, "API key");
        assert_eq!(status.weekly, None);
        assert_eq!(status.five_hour, None);
        interface::set_enabled(true);
        assert_eq!(interface::status_snapshot(), Some(status));
    }

    #[tokio::test]
    async fn account_status_does_not_depend_on_configured_native_items() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.local_settings.tui.status_line = Some(Vec::new());
        chat.update_account_state(account(Some("person@example.com")), None, true, true);
        chat.on_rate_limit_snapshot(Some(usage(35)));

        let status = interface::status_snapshot().expect("independent ALab status");
        assert_eq!(status.account, "person@example.com");
        assert_eq!(status.weekly.as_deref(), Some("weekly 65% left"));
        assert!(status.model.is_some());
        assert!(status.mode.is_some());
    }

    #[tokio::test]
    async fn footer_omits_only_routine_builtin_default_mode_labels() {
        use crate::legacy_core::config::PermissionProfileSnapshot;
        use codex_protocol::config_types::CollaborationModeMask;
        use codex_protocol::config_types::ModeKind;
        use codex_protocol::models::ActivePermissionProfile;
        use codex_protocol::models::PermissionProfile;

        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::set_enabled(true);
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        chat.config.workspace_roots = vec![chat.config.cwd.clone()];
        chat.config
            .permissions
            .set_workspace_roots(chat.config.workspace_roots.clone());
        let now = Local.timestamp_opt(1_800_000_000, 0).single().unwrap();
        chat.active_collaboration_mask = Some(CollaborationModeMask {
            name: "Selected mode".into(),
            mode: Some(ModeKind::Default),
            model: None,
            reasoning_effort: None,
            developer_instructions: None,
        });
        for (profile, id) in [
            (PermissionProfile::workspace_write(), ":workspace"),
            (PermissionProfile::read_only(), ":read-only"),
            (PermissionProfile::Disabled, ":danger-full-access"),
        ] {
            chat.config
                .permissions
                .set_permission_profile_from_session_snapshot(PermissionProfileSnapshot::active(
                    profile,
                    ActivePermissionProfile::new(id),
                ))
                .unwrap();
            // The permissions half is upstream's own label for the profile; only the mode
            // half is ALab's.
            let label = chat
                .status_line_value_for_item(StatusLineItem::Permissions)
                .expect("a builtin profile has a permissions label");
            for (kind, expected) in [
                (ModeKind::Default, label.clone()),
                (ModeKind::Plan, format!("Plan / {label}")),
            ] {
                chat.active_collaboration_mask.as_mut().unwrap().mode = Some(kind);
                assert_eq!(chat.alab_interface_status_at(now).mode, Some(expected));
            }
        }
        for id in [
            "locked",
            "Default",
            "Default / Workspace",
            "Default / Full Access",
        ] {
            chat.config
                .permissions
                .set_permission_profile_from_session_snapshot(PermissionProfileSnapshot::active(
                    PermissionProfile::read_only(),
                    ActivePermissionProfile::new(id),
                ))
                .unwrap();
            for kind in [ModeKind::Default, ModeKind::Plan] {
                chat.active_collaboration_mask.as_mut().unwrap().mode = Some(kind);
                assert_eq!(
                    chat.alab_interface_status_at(now).mode,
                    Some(format!("{} / {id}", kind.display_name())),
                    "named permission profiles keep their exact identity"
                );
            }
        }
    }

    #[tokio::test]
    async fn native_activity_transitions_refresh_without_terminal_title_status() {
        for enabled in [true, false] {
            for title_items in [Vec::new(), vec!["thread-name".to_string()]] {
                let state = Box::leak(Box::new(interface::State::new()));
                let _scope = interface::scoped(state);
                interface::set_enabled(enabled);
                let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
                chat.local_settings.tui.terminal_title = Some(title_items.clone());
                chat.update_account_state(account(Some("person@example.com")), None, true, true);
                chat.on_rate_limit_snapshot(Some(usage(35)));
                chat.on_task_started();
                if enabled {
                    assert_eq!(
                        interface::status_snapshot().unwrap().activity.as_deref(),
                        Some("Working")
                    );
                } else {
                    assert!(interface::status_snapshot().is_none());
                }
                chat.on_agent_reasoning_delta("**Checking the current files**".to_string());
                if enabled {
                    assert_eq!(
                        interface::status_snapshot().unwrap().activity.as_deref(),
                        Some("Thinking")
                    );
                } else {
                    assert!(interface::status_snapshot().is_none());
                }
                chat.track_unified_exec_process_begin("call-1", Some("proc-1"), "sleep 5");
                chat.on_terminal_interaction("proc-1".to_string(), String::new());
                assert_eq!(chat.local_settings.tui.terminal_title, Some(title_items));
                assert_eq!(interface::enabled(), enabled);
                if !enabled {
                    assert!(
                        interface::status_snapshot().is_none(),
                        "vanilla does not publish enhanced activity"
                    );
                    interface::set_enabled(true);
                    chat.refresh_status_surfaces();
                }
                let waiting = interface::status_snapshot().unwrap();
                assert_eq!(waiting.activity.as_deref(), Some("Waiting"));
                assert_eq!(waiting.account, "person@example.com");
                assert_eq!(waiting.weekly.as_deref(), Some("weekly 65% left"));
            }
        }
    }
}
