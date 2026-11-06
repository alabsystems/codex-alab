// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Display observations and causal recovery evidence from accepted account usage reads. This widget owns
//! its cache; an account change cannot lend a forecast or reset to another one.

use super::ChatWidget;
use crate::app_event::RateLimitRefreshOrigin;
use chrono::DateTime;
use chrono::Local;
use codex_alab_tui::interface;
use codex_alab_tui::quota;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::RateLimitResetCreditStatus;
use codex_app_server_protocol::RateLimitResetCreditsSummary;
use codex_app_server_protocol::RateLimitResetType;
use std::collections::BTreeSet;

const EXPIRING_SOON_SECONDS: i64 = 24 * 60 * 60;
const STALE_SECONDS: i64 = 15 * 60;

#[derive(Default)]
pub(super) struct State {
    account: Option<String>,
    pub(super) observed_plan: Option<codex_protocol::account::PlanType>,
    pub(super) weekly: Option<quota::Weekly>,
    accepted_history: quota::History,
    pub(super) weekly_resets_at: Option<i64>,
    pub(super) five_hour_resets_at: Option<i64>,
    reset_credits: Option<RateLimitResetCreditsSummary>,
    resets_observed_at: Option<i64>,
    reset_details_observed_at: Option<i64>,
    reset_details_attempted_at: Option<i64>,
    inspector_requested_details: bool,
}

impl State {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn remember_resets(&mut self, incoming: Option<&RateLimitResetCreditsSummary>, now: i64) {
        let count_changed = self
            .reset_credits
            .as_ref()
            .zip(incoming)
            .is_some_and(|(old, new)| old.available_count != new.available_count);
        let expired_details = incoming.is_some_and(|summary| summary.credits.is_none())
            && self
                .reset_credits
                .as_ref()
                .and_then(|summary| summary.credits.as_deref())
                .is_some_and(|credits| {
                    credits.iter().any(|credit| {
                        credit.status == RateLimitResetCreditStatus::Available
                            && credit.reset_type == RateLimitResetType::CodexRateLimits
                            && credit.expires_at.is_some_and(|expiry| expiry <= now)
                    })
                });
        let details_changed = count_changed || expired_details;
        if details_changed {
            self.reset_details_attempted_at = None;
            self.reset_details_observed_at = None;
        }
        if incoming.is_some_and(|summary| summary.credits.is_some()) {
            self.reset_details_observed_at = Some(now);
        }
        self.reset_credits = incoming.map(|incoming| {
            let mut incoming = incoming.clone();
            // A count-only read cannot identify replacement credits after an expiry.
            // Its count supersedes old local expiry losses, even when the count agrees.
            if incoming.credits.is_none() && !details_changed {
                incoming.credits = self
                    .reset_credits
                    .as_ref()
                    .filter(|previous| previous.available_count == incoming.available_count)
                    .and_then(|previous| previous.credits.clone());
            }
            incoming
        });
        self.resets_observed_at = incoming.map(|_| now);
        if incoming.is_none() {
            self.reset_details_observed_at = None;
        }
    }

    fn details_fresh(&self, now: i64) -> bool {
        self.reset_details_observed_at
            .is_some_and(|captured| (0..=STALE_SECONDS).contains(&now.saturating_sub(captured)))
    }

    fn request_details(&mut self, now: i64) -> bool {
        let explicit = std::mem::take(&mut self.inspector_requested_details);
        let due = self
            .reset_details_attempted_at
            .is_none_or(|attempt| !(0..STALE_SECONDS).contains(&now.saturating_sub(attempt)));
        let needed = self
            .reset_details_observed_at
            .is_none_or(|captured| !(0..STALE_SECONDS).contains(&now.saturating_sub(captured)))
            || self
                .reset_credits
                .as_ref()
                .is_none_or(|summary| summary.credits.is_none());
        let request = explicit || (needed && due);
        if request {
            self.reset_details_attempted_at = Some(now);
        }
        request
    }

    pub(super) fn resets_at(
        &self,
        now: i64,
        clock: crate::clock_format::ClockFormat,
    ) -> Option<interface::ResetCredits> {
        let summary = self.reset_credits.as_ref()?;
        let reported = usize::try_from(summary.available_count.max(0)).unwrap_or(usize::MAX);
        let details_fresh = self.details_fresh(now);
        let mut seen = BTreeSet::new();
        let mut known_count = 0;
        let expiries: Vec<i64> = summary
            .credits
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|credit| {
                credit.status == RateLimitResetCreditStatus::Available
                    && credit.reset_type == RateLimitResetType::CodexRateLimits
                    && seen.insert(&credit.id)
            })
            .take(reported)
            .filter_map(|credit| {
                known_count += 1;
                credit.expires_at
            })
            .collect();
        // Detail age can withhold future warnings, but cannot restore a credit
        // whose known deadline passed. A later count read
        // supersedes those rows in remember_resets.
        let expired_count = expiries.iter().filter(|expiry| **expiry <= now).count();
        let expiring_count = expiries
            .iter()
            .filter(|expiry| {
                details_fresh
                    && **expiry > now
                    && **expiry <= now.saturating_add(EXPIRING_SOON_SECONDS)
            })
            .count();
        let next_expiry = expiries
            .iter()
            .filter(|expiry| details_fresh && **expiry > now)
            .min()
            .and_then(|expiry| DateTime::from_timestamp(*expiry, 0))
            .map(|expiry| {
                expiry
                    .with_timezone(&Local)
                    .format(&format!("{} %Z", clock.date_time_format()))
                    .to_string()
            });
        Some(interface::ResetCredits {
            available_count: reported.saturating_sub(expired_count),
            expiring_count,
            expired_count,
            next_expiry,
            stale: self.resets_observed_at.is_none_or(|captured| {
                !(0..=STALE_SECONDS).contains(&now.saturating_sub(captured))
            }),
            // The backend may cap its detail list. Returned deadlines remain
            // useful, but cannot identify the next expiry among unreturned rows.
            details_known: details_fresh && summary.credits.is_some() && known_count == reported,
        })
    }

    pub(super) fn next_reset_change(&self, now: i64) -> Option<i64> {
        let summary = self.reset_credits.as_ref()?;
        let stale_at = self.resets_observed_at?.saturating_add(STALE_SECONDS + 1);
        let details_stale_at = self
            .reset_details_observed_at
            .map(|captured| captured.saturating_add(STALE_SECONDS + 1));
        let details_fresh = self.details_fresh(now);
        let expiry_changes = summary
            .credits
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|credit| {
                credit.status == RateLimitResetCreditStatus::Available
                    && credit.reset_type == RateLimitResetType::CodexRateLimits
            })
            .filter_map(|credit| credit.expires_at)
            .flat_map(|expiry| {
                [
                    Some(expiry),
                    details_fresh.then_some(expiry.saturating_sub(EXPIRING_SOON_SECONDS)),
                ]
            })
            .flatten();
        std::iter::once(stale_at)
            .chain(details_stale_at)
            .chain(expiry_changes)
            .filter(|deadline| *deadline > now)
            .min()
    }
}

fn quota_denied(snapshot: &codex_app_server_protocol::RateLimitSnapshot) -> bool {
    snapshot.spend_control_reached == Some(true)
        || snapshot.rate_limit_reached_type.is_some()
        || snapshot
            .individual_limit
            .as_ref()
            .is_some_and(|limit| limit.remaining_percent <= 0)
}

fn ordinary_permission(response: &GetAccountRateLimitsResponse) -> Option<bool> {
    match response.ordinary_usage_allowed {
        Some(false) => Some(false),
        Some(true)
            if response.rate_limit_upsell.is_none() && !quota_denied(&response.rate_limits) =>
        {
            Some(true)
        }
        _ => None,
    }
}

fn usable_quota_buckets(response: &GetAccountRateLimitsResponse) -> Vec<String> {
    crate::app_server_session::app_server_rate_limit_snapshots(response.clone())
        .into_iter()
        .filter(|snapshot| ordinary_permission(response) == Some(true) && !quota_denied(snapshot))
        .map(|s| s.limit_id.unwrap_or_else(|| "codex".into()))
        .collect()
}

impl ChatWidget {
    pub(crate) fn alab_quota_read_started(&self, id: u64) {
        if interface::active() {
            codex_alab_tui::session::shared()
                .quota()
                .start_read(id, std::time::Instant::now());
        }
    }

    pub(crate) fn alab_quota_read_accepted(
        &self,
        id: u64,
        response: &GetAccountRateLimitsResponse,
        origin: RateLimitRefreshOrigin,
    ) {
        if !interface::active()
            || !self.has_chatgpt_account()
            || !self.alab_usage_origin_current(origin)
        {
            return;
        }
        let Some(account) = response
            .account_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        else {
            return;
        };
        let usable = usable_quota_buckets(response);
        let denied = crate::app_server_session::app_server_rate_limit_snapshots(response.clone())
            .into_iter()
            .filter(quota_denied)
            .map(|s| s.limit_id.unwrap_or_else(|| "codex".into()))
            .collect();
        codex_alab_tui::session::shared()
            .quota()
            .accept_observation(
                id,
                account,
                usable,
                ordinary_permission(response),
                denied,
                std::time::Instant::now(),
            );
    }
    pub(crate) fn alab_invalidate_reset_credit_display(&mut self) {
        if interface::active() {
            self.alab_allowance.reset_credits = None;
            self.alab_allowance.resets_observed_at = None;
            self.alab_allowance.reset_details_observed_at = None;
            self.alab_allowance.reset_details_attempted_at = None;
            self.alab_refresh_interface();
        }
    }

    pub(crate) fn alab_include_reset_credit_details(&mut self) -> bool {
        interface::enabled()
            && self.has_chatgpt_account()
            && self
                .alab_allowance
                .request_details(Local::now().timestamp())
    }

    pub(super) fn alab_request_reset_details(&mut self) {
        self.alab_allowance.inspector_requested_details = true;
    }

    pub(crate) fn alab_account_usage_read(
        &mut self,
        response: &GetAccountRateLimitsResponse,
        origin: RateLimitRefreshOrigin,
    ) {
        self.alab_account_usage_read_for_origin_at(response, origin, Local::now().timestamp());
    }

    pub(super) fn alab_account_usage_read_for_origin_at(
        &mut self,
        response: &GetAccountRateLimitsResponse,
        origin: RateLimitRefreshOrigin,
        now: i64,
    ) {
        // The app has already checked account generation and read ordering. A
        // command can still have been replaced while its response was in flight;
        // use the same pending IDs as its native completion handler before any
        // history or reset-credit state is changed.
        if self.alab_usage_origin_current(origin) {
            self.alab_account_usage_read_at(response, now);
        }
    }

    fn alab_usage_origin_current(&self, origin: RateLimitRefreshOrigin) -> bool {
        match origin {
            RateLimitRefreshOrigin::Periodic | RateLimitRefreshOrigin::Recovery => true,
            RateLimitRefreshOrigin::StartupPrefetch {
                reset_hint_request_id,
            } => self.pending_rate_limit_reset_hint_request_id == Some(reset_hint_request_id),
            RateLimitRefreshOrigin::ResetPicker { request_id }
            | RateLimitRefreshOrigin::ResetConsume { request_id } => {
                self.pending_rate_limit_reset_request_id == Some(request_id)
            }
            RateLimitRefreshOrigin::UsageMenu { request_id } => {
                self.pending_usage_menu_rate_limit_request_id == Some(request_id)
            }
            RateLimitRefreshOrigin::StatusCommand { request_id } => self
                .refreshing_status_outputs
                .iter()
                .any(|(pending_request_id, _)| *pending_request_id == request_id),
        }
    }

    pub(super) fn alab_account_usage_read_at(
        &mut self,
        response: &GetAccountRateLimitsResponse,
        now: i64,
    ) {
        if !interface::active() || !self.has_chatgpt_account() {
            return;
        }
        let account_id = response
            .account_id
            .as_deref()
            .map(str::trim)
            .filter(|account| !account.is_empty());
        let account = account_id
            .map(|account| format!("account:{account}"))
            .unwrap_or_else(|| "native-generation".to_string());
        if self.alab_allowance.account.as_deref() != Some(&account) {
            self.alab_allowance.reset();
            self.alab_allowance.account = Some(account.clone());
        }
        self.alab_allowance
            .remember_resets(response.rate_limit_reset_credits.as_ref(), now);
        // Use native duplicate filtering and its cache key verbatim. The last
        // matching snapshot is what on_rate_limit_snapshot ultimately inserts.
        let snapshot = crate::app_server_session::app_server_rate_limit_snapshots(response.clone())
            .into_iter()
            .rev()
            .find(|snapshot| snapshot.limit_id.as_deref().unwrap_or("codex") == "codex");
        if let Some(plan) = snapshot.as_ref().and_then(|snapshot| snapshot.plan_type) {
            let changed = self.alab_allowance.observed_plan != Some(plan);
            self.alab_allowance.observed_plan = Some(plan);
            if changed && interface::enabled() {
                // Native request IDs reject replies for an old account. This updates the
                // model catalog without opening a picker or selecting a different model.
                self.refresh_account_scoped_models();
            }
        }
        let weekly = snapshot.as_ref().and_then(|snapshot| {
            [&snapshot.primary, &snapshot.secondary]
                .into_iter()
                .flatten()
                .find(|window| window.window_duration_mins == Some(10_080))
        });
        self.alab_allowance.weekly_resets_at = weekly.and_then(|window| window.resets_at);
        self.alab_allowance.five_hour_resets_at = snapshot.as_ref().and_then(|snapshot| {
            [&snapshot.primary, &snapshot.secondary]
                .into_iter()
                .flatten()
                .find(|window| window.window_duration_mins == Some(300))
                .and_then(|window| window.resets_at)
        });
        self.alab_allowance.weekly = weekly
            .and_then(|window| {
                Some(quota::WeeklySnapshot {
                    used_percent: f64::from(window.used_percent),
                    observed_at: now,
                    resets_at: window.resets_at?,
                    window_minutes: window.window_duration_mins?,
                })
            })
            .and_then(|observation| {
                let history = &mut self.alab_allowance.accepted_history;
                history.observe(observation);
                // Optional email metadata cannot identify a workspace. Without
                // a backend identity, keep this generation's history in memory.
                if account_id.is_some() {
                    let _ = quota::Store::new(&self.local_settings.codex_home)
                        .observe_with_history(&account, observation, history);
                }
                // The exact native observation owns the current display. Memory
                // survives storage errors without borrowing a different stored cycle.
                quota::Weekly::for_observation(observation, history.view(now))
            });
    }
}

#[cfg(test)]
#[path = "allowance_tests.rs"]
mod tests;
