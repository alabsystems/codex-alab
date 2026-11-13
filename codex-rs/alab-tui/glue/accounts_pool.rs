// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native subscription metadata stays in a view-owned buffer, outside chat history.

use super::ChatWidget;
use super::DetailsView;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
#[cfg(unix)]
use crate::clock_format::ClockFormat;
use codex_alab_tui::accounts_ui::Update;
use codex_alab_tui::accounts_ui::View;
use codex_alab_tui::aterm as codex_alab_aterm;
use codex_protocol::ThreadId;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

const VIEW_ID: &str = "alab-accounts";

/// One sentence for the pane and for a refused `/accounts use|auto`.
#[cfg(unix)]
pub(super) const CHANGES_UNAVAILABLE: &str = "Account changes are unavailable on this host.";

#[cfg(test)]
#[path = "accounts_pool_order_tests.rs"]
mod order_tests;

fn spawn_snapshot(
    view: Weak<Mutex<View>>,
    thread_id: Option<ThreadId>,
    tx: AppEventSender,
    fetch: impl Future<Output = Result<String, String>> + Send + 'static,
) -> Option<tokio::task::JoinHandle<()>> {
    let token = view.upgrade()?.lock().ok()?.begin_snapshot();
    Some(tokio::spawn(async move {
        let result = fetch.await;
        tx.send(AppEvent::AlabAccountsReady {
            view,
            thread_id,
            text: Update::Snapshot { token, result },
        });
    }))
}

#[cfg(unix)]
fn describe(snapshot: &codex_alab_aterm::accounts::Snapshot, clock_format: ClockFormat) -> String {
    let date_time_format = format!("{} %Z", clock_format.date_time_format());
    let mut lines = vec!["Subscription accounts · cached native observations".to_owned()];
    lines.push(if snapshot.assignment.account_id.is_some() {
        "Current account is marked below.".into()
    } else {
        "Current session account: unavailable".into()
    });
    lines.push("Session selection mode: unavailable".into());
    if snapshot.accounts.is_empty() {
        lines.push("\nNo accounts are configured in this aterm pool.".into());
    }
    for row in &snapshot.accounts {
        let selected = snapshot.assignment.account_id.as_ref() == Some(&row.id);
        lines.push(format!(
            "\n{}{}{}",
            row.label,
            if selected { " · current session" } else { "" },
            row.masked_email
                .as_ref()
                .map(|email| format!(" · {email}"))
                .unwrap_or_default()
        ));
        lines.push(format!(
            "{} · {}{}{}",
            row.condition.label(),
            if row.enrolled {
                "enrolled"
            } else {
                "not enrolled"
            },
            if row.held { " · held" } else { "" },
            if row.quota.stale {
                " · stale usage"
            } else {
                ""
            }
        ));
        let quota = &row.quota;
        for (name, used, reset) in [
            ("5h", quota.five_hour_used_percent, quota.five_hour_reset_at),
            ("Weekly", quota.weekly_used_percent, quota.weekly_reset_at),
        ] {
            let usage = used
                .map(|n| format!("{n:.0}% used"))
                .unwrap_or_else(|| "usage unavailable".into());
            let reset = reset
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|time| {
                    time.with_timezone(&chrono::Local)
                        .format(&date_time_format)
                        .to_string()
                })
                .unwrap_or_else(|| "unavailable".into());
            lines.push(format!("{name}: {usage} · resets {reset}"));
        }
        lines.push(format!(
            "Observed: {}",
            quota
                .observed_at
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|time| time
                    .with_timezone(&chrono::Local)
                    .format(&date_time_format)
                    .to_string())
                .unwrap_or_else(|| "unavailable".into())
        ));
    }
    lines.push(format!("\n{CHANGES_UNAVAILABLE}"));
    lines.join("\n")
}

#[cfg(unix)]
pub(super) fn failure(error: &codex_alab_aterm::Error) -> &'static str {
    use codex_alab_aterm::Error;
    match error {
        Error::Refused(line)
            if line.starts_with("ERR unsupported")
                || line.starts_with("ERR unknown")
                || line == "ERR codex-accounts unsupported" =>
        {
            "This aterm version does not support the accounts view."
        }
        Error::Refused(line)
            if line.starts_with("ERR denied")
                || line.starts_with("ERR auth")
                || line == "ERR codex-accounts denied" =>
        {
            "aterm could not authorize account inspection for this session."
        }
        Error::Refused(_) => "aterm refused account inspection for this session.",
        Error::Malformed(_) | Error::BadRequest(_) => {
            "aterm returned an incompatible accounts snapshot."
        }
        _ => "aterm account information is unavailable. Refresh to try again.",
    }
}

impl ChatWidget {
    #[cfg(not(unix))]
    pub(crate) fn alab_accounts_command(&mut self, _args: &str) {
        self.alab_open_accounts();
    }

    #[cfg(not(unix))]
    pub(crate) fn alab_enrollment_event(
        &mut self,
        _view: Weak<Mutex<codex_alab_tui::enrollment_ui::View>>,
        _thread_id: Option<ThreadId>,
        event: codex_alab_tui::enrollment_ui::Event,
    ) {
        match event {}
    }

    pub(crate) fn alab_open_accounts(&mut self) {
        if !codex_alab_core::is_alab_binary() {
            return;
        }
        let weak = self.alab_accounts_view();
        let thread_id = self.thread_id();
        let tx = self.app_event_tx.clone();
        self.alab_fetch_accounts(weak, thread_id, tx);
    }

    pub(crate) fn alab_accounts_view(&mut self) -> Weak<Mutex<View>> {
        self.bottom_pane.dismiss_view_by_id(VIEW_ID);
        let text = Arc::new(Mutex::new(View::new(
            "Loading subscription accounts from aterm…".to_owned(),
        )));
        let weak = Arc::downgrade(&text);
        let thread_id = self.thread_id();
        let refresh = self.app_event_tx.clone();
        let refresh_view = weak.clone();
        self.show_bottom_pane_view(Box::new(
            DetailsView::new(
                VIEW_ID,
                "Accounts",
                move || {
                    let text = text
                        .lock()
                        .map(|view| view.text().to_owned())
                        .unwrap_or_else(|_| "Account view unavailable".into());
                    Paragraph::new(text).wrap(Wrap { trim: false })
                },
                "refresh",
                None,
                self.bottom_pane.list_keymap(),
            )
            .with_refresh(move || {
                refresh.send(AppEvent::AlabAccountsRefresh {
                    view: refresh_view.clone(),
                    thread_id,
                });
            }),
        ));
        weak
    }

    pub(crate) fn alab_refresh_accounts(
        &self,
        view: Weak<Mutex<View>>,
        thread_id: Option<ThreadId>,
    ) {
        if self.thread_id() == thread_id
            && self
                .bottom_pane
                .selected_index_for_active_view(VIEW_ID)
                .is_some()
            && view.upgrade().is_some()
        {
            self.alab_fetch_accounts(view, thread_id, self.app_event_tx.clone());
        }
    }

    fn alab_fetch_accounts(
        &self,
        weak: Weak<Mutex<View>>,
        thread_id: Option<ThreadId>,
        tx: AppEventSender,
    ) {
        #[cfg(unix)]
        let clock_format = self.clock_format;
        #[cfg(unix)]
        let fetch = async move {
            let result = match codex_alab_aterm::Client::connect_accounts_self().await {
                Ok(Some(client)) => {
                    let birth = codex_alab_tui::identity::current()
                        .ok()
                        .and_then(|identity| codex_alab_tui::identity::proc_start_utc(&identity));
                    match birth {
                        Some(birth) => client
                            .accounts_snapshot(&birth)
                            .await
                            .map(|snapshot| describe(&snapshot, clock_format)),
                        None => Err(codex_alab_aterm::Error::BadRequest(
                            "process identity unavailable".into(),
                        )),
                    }
                }
                Ok(None) => Err(codex_alab_aterm::Error::Closed),
                Err(error) => Err(error),
            };
            result.map_err(|error| failure(&error).to_owned())
        };
        #[cfg(not(unix))]
        let fetch =
            async { Err("Native account inspection is unavailable on this platform.".into()) };
        let _ = spawn_snapshot(weak, thread_id, tx, fetch);
    }

    pub(crate) fn alab_accounts_ready(
        &mut self,
        view: Weak<Mutex<View>>,
        thread_id: Option<ThreadId>,
        text: Update,
    ) {
        if self.thread_id() != thread_id
            || self
                .bottom_pane
                .selected_index_for_present_view(VIEW_ID)
                .is_none()
        {
            return;
        }
        if let Some(view) = view.upgrade()
            && let Ok(mut current) = view.lock()
            && current.apply(text)
        {
            self.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
    use crate::chatwidget::tests::render_bottom_popup;

    #[tokio::test]
    async fn accounts_refresh_stays_open_and_cannot_reopen_over_a_replacement() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let original = chat.alab_accounts_view();
        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        let AppEvent::AlabAccountsRefresh { view, thread_id } = rx.try_recv().unwrap() else {
            panic!("Enter must queue an account refresh");
        };
        assert!(
            original.upgrade().is_some(),
            "refresh must keep its view alive"
        );
        assert_eq!(chat.bottom_pane.active_view_id(), Some(VIEW_ID));
        assert!(view.ptr_eq(&original));
        chat.alab_refresh_accounts(view, thread_id);
        tokio::task::yield_now().await;
        while rx.try_recv().is_ok() {}

        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        let AppEvent::AlabAccountsRefresh { view, thread_id } = rx.try_recv().unwrap() else {
            panic!("Enter must queue another account refresh");
        };
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        chat.show_bottom_pane_view(Box::new(DetailsView::new(
            "accounts-replacement-test",
            "Other view",
            || Paragraph::new("replacement"),
            "close",
            None,
            chat.bottom_pane.list_keymap(),
        )));
        assert!(original.upgrade().is_none());
        chat.alab_refresh_accounts(view, thread_id);
        tokio::task::yield_now().await;
        assert_eq!(
            chat.bottom_pane.active_view_id(),
            Some("accounts-replacement-test")
        );
        assert!(render_bottom_popup(&chat, 80).contains("replacement"));
        assert!(
            rx.try_recv().is_err(),
            "stale refresh must start no async work"
        );
        assert!(ops.try_recv().is_err());
    }

    #[tokio::test]
    async fn accounts_reply_updates_only_its_open_view_and_original_thread() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.apply_external_edit("unfinished work".into());
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let original_thread = chat.thread_id();
        let old = chat.alab_accounts_view();
        chat.alab_accounts_ready(
            old.clone(),
            original_thread,
            Update::Observation("two cached accounts".into()),
        );
        assert!(render_bottom_popup(&chat, 80).contains("two cached accounts"));
        let current = chat.alab_accounts_view();
        assert!(old.upgrade().is_none());
        chat.alab_accounts_ready(
            old,
            original_thread,
            Update::Observation("stale secretless metadata".into()),
        );
        assert!(!render_bottom_popup(&chat, 80).contains("stale secretless metadata"));
        chat.thread_id = Some(ThreadId::new());
        chat.alab_accounts_ready(
            current.clone(),
            original_thread,
            Update::Observation("wrong thread".into()),
        );
        assert!(!render_bottom_popup(&chat, 80).contains("wrong thread"));
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        assert!(current.upgrade().is_none());
        assert_eq!(chat.composer_text_with_pending(), "unfinished work");
        assert!(
            rx.try_recv().is_err(),
            "inspection must write no transcript event"
        );
        assert!(
            ops.try_recv().is_err(),
            "inspection must send no model operation"
        );
    }

    #[tokio::test]
    async fn accounts_view_remains_available_with_presentation_off() {
        const CHILD: &str = "ALAB_ACCOUNTS_OFF_FIXTURE";
        if std::env::var_os(CHILD).is_none() {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    concat!(
                        module_path!(),
                        "::accounts_view_remains_available_with_presentation_off"
                    )
                    .split_once("::")
                    .unwrap()
                    .1,
                ])
                .env(CHILD, "1")
                .env_remove(codex_alab_core::session::ATERM_SESSION_ENV)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
            return;
        }
        codex_alab_core::activate();
        codex_alab_core::set_enhanced(false);
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        assert!(!codex_alab_core::active());
        assert!(
            crate::slash_command::built_in_slash_commands()
                .iter()
                .any(|(_, command)| *command == crate::slash_command::SlashCommand::Accounts)
        );
        chat.alab_open_accounts();
        assert!(
            chat.bottom_pane
                .selected_index_for_present_view(VIEW_ID)
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn accounts_labels_and_unknown_usage_render_in_a_narrow_view() {
        use codex_alab_aterm::accounts::*;
        let snapshot = Snapshot {
            schema: 1,
            observed_at: 1_800_000_000,
            request: "a".repeat(32),
            session: Session {
                sid: "s-a".into(),
                process_pid: 1,
                process_birth: "birth".into(),
                epoch: "epoch".into(),
            },
            revision: "b".repeat(64),
            assignment: Assignment {
                account_id: None,
                mode: Mode::Unknown,
            },
            accounts: vec![Account {
                id: "opaque".into(),
                label: "Spare subscription".into(),
                masked_email: None,
                condition: Condition::Unseen,
                enrolled: true,
                held: false,
                eligible: false,
                quota: Quota {
                    stale: true,
                    observed_at: None,
                    five_hour_used_percent: None,
                    five_hour_reset_at: None,
                    weekly_used_percent: None,
                    weekly_reset_at: None,
                },
            }],
            capabilities: Capabilities {
                add: false,
                select: false,
                auto: false,
            },
        };
        let text = describe(&snapshot, ClockFormat::TwentyFourHour);
        assert!(text.contains("Spare subscription"));
        assert!(text.contains("usage unavailable"));
        assert!(text.contains("stale usage"));
        assert!(!text.contains("0% used"));
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        assert!(
            crate::render::renderable::Renderable::desired_height(&paragraph, 20)
                > crate::render::renderable::Renderable::desired_height(&paragraph, 80)
        );
    }

    #[cfg(unix)]
    #[test]
    fn accounts_errors_never_echo_host_data() {
        let error = codex_alab_aterm::Error::Refused("ERR codex-accounts denied".into());
        assert!(failure(&error).contains("authorize"));
        let error = codex_alab_aterm::Error::Refused("ERR private token or path".into());
        assert!(!failure(&error).contains("private token"));
    }
}
