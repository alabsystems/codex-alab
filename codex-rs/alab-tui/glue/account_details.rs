// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Account inspection uses the native bottom-pane lifecycle and list keymap. Its
//! live overview scrolls inside short viewports, so every reset remains reachable.
//! A quota update never replaces the view or resets an in-progress key chord.

use super::ChatWidget;
use super::interface;
use crate::app_event::AppEvent;
use crate::app_event::RateLimitRefreshOrigin;
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

const VIEW_ID: &str = "alab-account";

struct AccountDetailsHeader;

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
            let local_time = |seconds| {
                chrono::DateTime::from_timestamp(seconds, 0)
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format("%d %b %H:%M %Z")
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
                if !resets.details_known {
                    lines.push(Line::from(if resets.next_expiry.is_some() {
                        "Other reset credit expiry: unavailable"
                    } else {
                        "Reset credit expiry: unavailable"
                    }));
                }
                if resets.expiring_count > 0 {
                    let qualifier = if resets.details_known { "" } else { "known " };
                    lines.push(Line::from(format!(
                        "{} {qualifier}reset credits expire within 24 hours. Open /usage to review them.",
                        resets.expiring_count
                    )));
                }
                if resets.expired_count > 0 {
                    lines.push(Line::from(format!(
                        "{} reset credits have expired. Open /usage for current reset options.",
                        resets.expired_count
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

fn account_view(keymap: ListKeymap) -> DetailsView {
    DetailsView::new(
        VIEW_ID,
        "Account details",
        || AccountDetailsHeader.paragraph(),
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
    #[cfg(test)]
    pub(crate) fn alab_account_details_is_open(&self) -> bool {
        interface::enabled()
            && self
                .bottom_pane
                .selected_index_for_present_view(VIEW_ID)
                .is_some()
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
        self.show_bottom_pane_view(Box::new(account_view(self.bottom_pane.list_keymap())));
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
