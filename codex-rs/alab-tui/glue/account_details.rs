// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Account inspection uses the native bottom-pane lifecycle and list keymap. Its
//! live overview scrolls inside short viewports, so every reset remains reachable.
//! A quota update never replaces the view or resets an in-progress key chord.

use super::ChatWidget;
use super::interface;
use crate::app_event::AppEvent;
use crate::app_event::RateLimitRefreshOrigin;
use crate::clock_format::ClockFormat;
use crate::keymap::ListKeymap;
use crate::render::renderable::Renderable;
use codex_alab_tui::interface::InterfaceStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

#[path = "details_view.rs"]
mod details_view;
use details_view::DetailsView;

#[path = "accounts_pool.rs"]
mod accounts_pool;

#[path = "enrollment_pool.rs"]
mod enrollment_pool;

#[path = "account_move_pool.rs"]
mod account_move_pool;

const VIEW_ID: &str = "alab-account";

struct AccountDetailsHeader {
    clock_format: ClockFormat,
    show_account_pool: bool,
}

fn quota_line(label: &str, meter: Option<&str>, reset: Option<&str>, stale: bool) -> String {
    match (meter, reset) {
        (Some(_), _) if stale => format!("{label}: stale · refresh needed"),
        (Some(meter), Some(reset)) => format!("{meter} · resets {reset}"),
        (Some(meter), None) => format!("{meter} · reset unavailable"),
        (None, _) => format!("{label}: unavailable"),
    }
}

impl AccountDetailsHeader {
    fn paragraph(&self) -> Paragraph<'static> {
        let status = interface::status_snapshot().unwrap_or_default();
        let InterfaceStatus {
            account,
            plan,
            weekly,
            weekly_reset,
            five_hour,
            five_hour_reset,
            usage_checked_at,
            usage_stale,
            five_hour_stale,
            quota,
            reset_credits,
            credit_balance,
            individual_credit_limit,
            thread_credit_usage,
            thread_credit_notice,
            ..
        } = status;
        let mut lines = Vec::new();
        match interface::connection_state() {
            interface::ConnectionState::Connected => {}
            interface::ConnectionState::Reconnecting | interface::ConnectionState::Offline => {
                lines.push(Line::from("Cached account and quota data."));
                lines.push(Line::default());
            }
        }
        lines.extend([
            Line::from(format!("Account: {account}")),
            Line::from(format!(
                "Plan: {}",
                plan.as_deref().unwrap_or("Unavailable")
            )),
        ]);
        if credit_balance.is_some()
            || individual_credit_limit.is_some()
            || thread_credit_usage.is_some()
            || thread_credit_notice.is_some()
            || status.local_subscription
            || status.credit_history.is_some()
        {
            lines.push(Line::default());
            lines.push(Line::from("Usage credits"));
            lines.push(Line::from(
                credit_balance
                    .as_ref()
                    .map_or("Balance unreported", interface::CreditBalance::label)
                    .to_owned(),
            ));
            if let Some(limit) = individual_credit_limit {
                lines.push(match limit {
                    interface::IndividualCreditLimit::Current(label) => Line::from(label),
                    interface::IndividualCreditLimit::AwaitingRefresh => {
                        Line::from("Your monthly credit limit · refresh needed")
                    }
                });
            }
            if let Some(report) = thread_credit_usage {
                lines.push(Line::from(report.label().into_owned()));
                if let Some(comparison) = report.summary.limit_comparison {
                    lines.push(Line::from(format!(
                        "Chat usage vs. current limits · {comparison}"
                    )));
                }
                if let Some(as_of) = report
                    .as_of
                    .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
                {
                    let local = as_of.with_timezone(&chrono::Local);
                    lines.push(Line::from(format!(
                        "Chat usage reported through {}",
                        local.format(self.clock_format.date_time_format())
                    )));
                }
            } else if let Some(notice) = thread_credit_notice {
                lines.push(Line::from(notice));
            } else if status.credit_history == Some(interface::CreditHistory::PersonalChats) {
                lines.push(Line::from(interface::UNREPORTED_CHAT_SPENDING));
            }
            if let Some(history) = status.credit_history {
                lines.push(Line::from(match history {
                    interface::CreditHistory::PersonalChats => {
                        "Chat spending in /usage → View analytics → Chats"
                    }
                    interface::CreditHistory::WorkspaceCredits => {
                        "Workspace spending in /usage → View analytics → Credits"
                    }
                }));
            }
            lines.push(Line::default());
        }
        if self.show_account_pool {
            lines.push(Line::from(
                "Use /accounts to view aterm's subscription accounts.",
            ));
        }
        lines.extend([
            Line::default(),
            Line::from(quota_line(
                "Weekly",
                weekly.as_deref(),
                weekly_reset.as_deref(),
                usage_stale,
            )),
            Line::from(quota_line(
                "5h",
                five_hour.as_deref(),
                five_hour_reset.as_deref(),
                five_hour_stale,
            )),
            Line::default(),
            Line::from(format!(
                "Usage checked: {}",
                usage_checked_at.as_deref().unwrap_or("Unavailable")
            )),
        ]);
        if let Some(quota) = quota.filter(|_| !usage_stale) {
            lines.push(Line::default());
            lines.push(Line::from(format!(
                "Weekly usage history · 0–100% used · {} observation{}",
                quota.sample_count,
                if quota.sample_count == 1 { "" } else { "s" }
            )));
            lines.push(Line::from(quota.sparkline(40)));
            let date_time_format = format!("{} %Z", self.clock_format.date_time_format());
            let local_time = |seconds| {
                chrono::DateTime::from_timestamp(seconds, 0)
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format(&date_time_format)
                            .to_string()
                    })
                    .unwrap_or_else(|| "Unavailable".to_string())
            };
            lines.push(Line::from(format!(
                "{} → {}",
                local_time(quota.resets_at.saturating_sub(quota.window_minutes * 60)),
                local_time(quota.resets_at)
            )));
            lines.push(Line::from(
                "· means no observation; bar height shows reported usage.",
            ));
            if let Some(projection) = quota.projection {
                lines.push(Line::from(format!(
                    "Estimated at reset: {:.0}% used · recent pace {:.1}% per day",
                    projection.projected_percent_at_reset, projection.percent_per_day
                )));
                if let Some(exhausted_at) = projection.exhausted_at {
                    lines.push(Line::from(format!(
                        "Estimated limit reached: {}",
                        local_time(exhausted_at)
                    )));
                }
            } else {
                lines.push(Line::from("Forecast: collecting usage history"));
            }
        }
        if let Some(resets) = reset_credits {
            lines.push(Line::default());
            if resets.stale {
                lines.push(Line::from("Usage limit resets: stale · refresh needed"));
            } else {
                lines.push(Line::from(format!(
                    "Usage limit resets: {} available",
                    resets.available_count
                )));
                if let Some(expiry) = resets.next_expiry.as_deref() {
                    let label = if resets.details_known {
                        "Next reset credit expires"
                    } else {
                        "Next known reset credit expires"
                    };
                    lines.push(Line::from(format!("{label}: {expiry}")));
                }
                if resets.available_count > 0 && !resets.details_known {
                    lines.push(Line::from(if resets.next_expiry.is_some() {
                        "Other reset credit expiry: unavailable"
                    } else {
                        "Reset credit expiry: unavailable"
                    }));
                }
                let mut expirations = Vec::new();
                if resets.expiring_count > 0 {
                    let minimum = if resets.details_known {
                        ""
                    } else {
                        "At least "
                    };
                    expirations.push(format!(
                        "{minimum}{} expiring within 24 hours",
                        resets.expiring_count
                    ));
                }
                if resets.expired_count > 0 {
                    expirations.push(format!("{} expired", resets.expired_count));
                }
                if !expirations.is_empty() {
                    lines.push(Line::from(format!(
                        "{} · Review resets in /usage",
                        expirations.join(" · ")
                    )));
                }
            }
        }
        Paragraph::new(lines).wrap(Wrap { trim: false })
    }
}

impl Renderable for AccountDetailsHeader {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Renderable::render(&self.paragraph(), area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.paragraph().desired_height(width)
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        Renderable::render(&self.paragraph().scroll((scroll_offset, 0)), area, buf);
        true
    }
}

fn account_view(
    keymap: ListKeymap,
    clock_format: ClockFormat,
    show_account_pool: bool,
) -> DetailsView {
    let header = AccountDetailsHeader {
        clock_format,
        show_account_pool,
    };
    DetailsView::new(
        VIEW_ID,
        "Account details",
        move || header.paragraph(),
        "Close",
        None,
        keymap,
    )
    .with_dynamic_title(|| match interface::connection_state() {
        interface::ConnectionState::Connected => "Account details",
        interface::ConnectionState::Reconnecting => "Reconnecting — Account details",
        interface::ConnectionState::Offline => "Offline — Account details",
    })
}

impl ChatWidget {
    #[cfg(unix)]
    fn alab_show_account_pool_hint(&self, aterm_session: bool) -> bool {
        aterm_session && self.alab_local_account_server()
    }

    pub(in crate::chatwidget) fn alab_account_details_is_active(&self) -> bool {
        self.bottom_pane.active_view_id() == Some(VIEW_ID)
    }

    pub(in crate::chatwidget) fn alab_open_account_details(&mut self) {
        if !interface::enabled() {
            return;
        }
        self.alab_refresh_interface();
        if self
            .bottom_pane
            .selected_index_for_present_view(VIEW_ID)
            .is_some()
        {
            return;
        }
        #[cfg(unix)]
        let show_account_pool =
            self.alab_show_account_pool_hint(codex_alab_tui::frontend_control::requested());
        #[cfg(not(unix))]
        let show_account_pool = false;
        self.show_bottom_pane_view(Box::new(account_view(
            self.bottom_pane.list_keymap(),
            self.clock_format,
            show_account_pool,
        )));
        if self.should_prefetch_rate_limits() {
            self.alab_request_reset_details();
            self.app_event_tx.send(AppEvent::RefreshRateLimits {
                origin: RateLimitRefreshOrigin::Periodic,
            });
        }
    }

    pub(in crate::chatwidget) fn alab_refresh_account_details(&mut self) {
        if self
            .bottom_pane
            .selected_index_for_present_view(VIEW_ID)
            .is_some()
        {
            self.request_redraw();
        }
    }

    pub(in crate::chatwidget) fn alab_close_account_details(&mut self) {
        self.bottom_pane.dismiss_view_by_id(VIEW_ID);
    }
}

#[cfg(test)]
#[path = "account_details_tests.rs"]
mod tests;
