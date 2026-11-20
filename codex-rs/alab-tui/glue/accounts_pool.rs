// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native subscription metadata stays in a view-owned buffer, outside chat history. Only an
//! account move's last word, a fixed sentence that names no account, is said in history, and
//! only where its pane is gone ([`ChatWidget::alab_accounts_ready`]).

use super::ChatWidget;
use super::DetailsView;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionDescriptionLayout;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
#[cfg(unix)]
use crate::clock_format::ClockFormat;
use crate::keymap::ListAction;
use codex_alab_tui::accounts_ui::AddressResult;
#[cfg(unix)]
use codex_alab_tui::accounts_ui::Destination;
use codex_alab_tui::accounts_ui::PickerAction;
use codex_alab_tui::accounts_ui::Snapshot as PaneSnapshot;
use codex_alab_tui::accounts_ui::Update;
use codex_alab_tui::accounts_ui::View;
use codex_alab_tui::aterm as codex_alab_aterm;
#[cfg(unix)]
use codex_alab_tui::aterm::accounts::Condition;
use codex_protocol::ThreadId;
#[cfg(unix)]
use itertools::Itertools;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

pub(super) const VIEW_ID: &str = "alab-accounts";
const PICKER_ID: &str = "alab-account-picker";

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
    fetch: impl Future<Output = Result<PaneSnapshot, String>> + Send + 'static,
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
fn describe(
    snapshot: &codex_alab_aterm::accounts::Snapshot,
    clock_format: ClockFormat,
    autopilot_off: bool,
) -> PaneSnapshot {
    let switch_refusal =
        super::account_move_pool::switch_refusal(snapshot, /*auto*/ false, autopilot_off);
    let date_time_format = format!("{} %Z", clock_format.date_time_format());
    let mut lines = vec!["Subscription accounts · cached native observations".to_owned()];
    let label_of = |id: &String| {
        snapshot
            .accounts
            .iter()
            .find(|row| &row.id == id)
            .map(|row| row.label.as_str())
    };
    match &snapshot.pool {
        // Schema 1: aterm names no row as this session's and offers no move.
        None => {
            lines.push(if snapshot.assignment.account_id.is_some() {
                "Current account is marked below.".into()
            } else {
                "Current session account: unavailable".into()
            });
            lines.push("Session selection mode: unavailable".into());
        }
        // aterm names the row from the login this session was launched with.
        Some(pool) => {
            lines.push(
                match snapshot.assignment.account_id.as_ref().and_then(label_of) {
                    Some(label) => format!("Session account (by its launch login): {label}"),
                    None => "Session account: unavailable".into(),
                },
            );
            lines.push(match pool.choice_id.as_ref().and_then(label_of) {
                Some(label) if pool.pinned => format!("Pool's choice: {label} · pinned"),
                Some(label) => format!("Pool's choice: {label}"),
                None => "Pool's choice: none".into(),
            });
        }
    }
    if snapshot.accounts.is_empty() {
        lines.push("\nNo accounts are configured in this aterm pool.".into());
    }
    let shared_mask = !snapshot
        .accounts
        .iter()
        .filter_map(|row| row.masked_email.as_deref())
        .all_unique();
    if shared_mask {
        lines.push(
            "\nSome accounts have the same masked email. Check their slot labels. \
             From a codex-alab checkout, run bash alab/account-labels.sh \
             to match provider and sign-in addresses to their switch labels."
                .into(),
        );
    }
    let label_counts = snapshot
        .accounts
        .iter()
        .map(|row| row.label.as_str())
        .counts();
    let mut destinations = Vec::new();
    let mut named_alternative = false;
    let mut switch_annotations = Vec::new();
    for row in &snapshot.accounts {
        let selected = snapshot.assignment.account_id.as_ref() == Some(&row.id);
        let switch = if selected || switch_refusal.is_some() {
            String::new()
        } else if row.eligible
            && codex_alab_aterm::account_move::admissible_label(&row.label)
            && label_counts.get(row.label.as_str()) == Some(&1)
        {
            named_alternative = true;
            if snapshot.capabilities.select_bound
                && let Some(selection_ref) = &row.selection_ref
            {
                destinations.push(Destination {
                    id: row.id.clone(),
                    label: row.label.clone(),
                    masked_email: row.masked_email.clone(),
                    revision: snapshot.revision.clone(),
                    selection_ref: selection_ref.clone(),
                });
            }
            format!(" · /accounts use {}", row.label)
        } else if !row.eligible && row.condition == Condition::Limited {
            " · switch unavailable (usage limit)".into()
        } else {
            " · switch unavailable".into()
        };
        if !switch.is_empty() {
            switch_annotations.push((lines.len(), switch));
        }
        lines.push(format!(
            "\n{}{}{}",
            row.label,
            if selected { " · current session" } else { "" },
            row.masked_email
                .as_ref()
                .map(|email| format!(" · {email}"))
                .unwrap_or_default()
        ));
        let reason = match (&row.condition, row.signed_out_reason) {
            (Condition::SignedOut, Some(reason)) => format!(" ({})", reason.words()),
            _ => String::new(),
        };
        lines.push(format!(
            "{} · cached pool status {}{reason}{}{}",
            if row.enrolled {
                "Login file present"
            } else {
                "No login file"
            },
            row.condition.label(),
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
    lines.push("\n/accounts add [label] opens aterm enrollment for a configured account.".into());
    // Cached observations stay readable during a refresh. Actions belong to the revision
    // that offered them, including automatic choice when no named destination is eligible.
    let cached_text = lines.join("\n");
    for (row, annotation) in switch_annotations {
        lines[row].push_str(&annotation);
    }
    if let Some(reason) = switch_refusal {
        lines.push(format!("\n{reason}"));
    } else {
        lines.push(if !named_alternative {
            "\nNo other account is available to select by label.".into()
        } else {
            "\n/accounts use <label> requests a switch to that account.".into()
        });
        if snapshot.capabilities.auto {
            lines.push("/accounts auto requests the pool's choice.".into());
        }
        if named_alternative || snapshot.capabilities.auto {
            lines.push(
                "At relaunch, aterm checks whether the requested account can serve. After resume, /status shows the active login."
                    .into(),
            );
        }
    }
    PaneSnapshot {
        text: lines.join("\n"),
        cached_text: Some(cached_text),
        shared_mask,
        address_revision: shared_mask.then(|| Arc::from(snapshot.revision.as_str())),
        destinations,
    }
}

#[cfg(unix)]
fn address_failure(error: &codex_alab_aterm::Error) -> AddressResult {
    match error {
        codex_alab_aterm::Error::Refused(line) if line == "ERR unknown verb" => {
            AddressResult::Unsupported
        }
        codex_alab_aterm::Error::Refused(line) if line == "ERR codex-accounts-display stale" => {
            AddressResult::Stale
        }
        _ => AddressResult::Unavailable,
    }
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
        self.alab_refresh_interface();
        self.bottom_pane.dismiss_view_by_id(PICKER_ID);
        self.bottom_pane.dismiss_view_by_id(VIEW_ID);
        let text = Arc::new(Mutex::new(View::new(
            "Loading subscription accounts from aterm…".to_owned(),
        )));
        let weak = Arc::downgrade(&text);
        let thread_id = self.thread_id();
        let refresh = self.app_event_tx.clone();
        let refresh_view = weak.clone();
        let picker = self.app_event_tx.clone();
        let picker_view = weak.clone();
        let choices = Arc::clone(&text);
        let addresses = Arc::clone(&text);
        let address_view = weak.clone();
        let address_tx = self.app_event_tx.clone();
        self.show_bottom_pane_view(Box::new(
            DetailsView::new(
                VIEW_ID,
                "Accounts",
                move || {
                    let email = super::interface::connected_local_email();
                    let text = text
                        .lock()
                        .map(|view| view.display_text(email.as_deref()))
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
            })
            .with_navigation_action(
                ListAction::MoveLeft,
                "show addresses",
                move || {
                    addresses
                        .lock()
                        .is_ok_and(|view| view.address_action().is_some())
                },
                move || {
                    if let Some(revision) = address_view
                        .upgrade()
                        .and_then(|owner| owner.lock().ok()?.address_action())
                    {
                        address_tx.send(AppEvent::AlabAccountsPicker {
                            view: address_view.clone(),
                            thread_id,
                            action: PickerAction::ShowAddresses(revision),
                        });
                    }
                },
            )
            .with_navigation_action(
                ListAction::MoveRight,
                "switch account",
                move || {
                    choices
                        .lock()
                        .is_ok_and(|view| !view.destinations().is_empty())
                },
                move || {
                    picker.send(AppEvent::AlabAccountsPicker {
                        view: picker_view.clone(),
                        thread_id,
                        action: PickerAction::Open,
                    });
                },
            ),
        ));
        weak
    }

    pub(crate) fn alab_accounts_picker(
        &mut self,
        view: Weak<Mutex<View>>,
        thread_id: Option<ThreadId>,
        action: PickerAction,
    ) {
        if self.thread_id() != thread_id {
            return;
        }
        let Some(owner) = view.upgrade() else { return };
        let Ok(current) = owner.lock() else { return };
        match action {
            #[cfg(unix)]
            PickerAction::ShowAddresses(requested)
                if self.bottom_pane.active_view_id() == Some(VIEW_ID) =>
            {
                let mut current = current;
                let Some(revision) = current.begin_addresses(&requested) else {
                    return;
                };
                drop(current);
                self.request_redraw();
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let result = match codex_alab_aterm::Client::connect_accounts_self().await {
                        Ok(Some(client)) => {
                            if !view.upgrade().is_some_and(|owner| {
                                owner
                                    .lock()
                                    .is_ok_and(|state| state.awaiting_addresses(&requested))
                            }) {
                                return;
                            }
                            client.show_account_addresses(&revision).await
                        }
                        Ok(None) => Err(codex_alab_aterm::Error::Closed),
                        Err(error) => Err(error),
                    };
                    let result = match result {
                        Ok(()) => AddressResult::Opened,
                        Err(error) => address_failure(&error),
                    };
                    tx.send(AppEvent::AlabAccountsReady {
                        view,
                        thread_id,
                        text: Update::Addresses {
                            revision: requested,
                            result,
                        },
                    });
                });
            }
            PickerAction::Open
                if self.bottom_pane.active_view_id() == Some(VIEW_ID)
                    && !current.destinations().is_empty() =>
            {
                let items = current
                    .destinations()
                    .iter()
                    .cloned()
                    .map(|destination| {
                        let row = Arc::new(destination);
                        let view = view.clone();
                        SelectionItem {
                            name: row.label.clone(),
                            description: row.masked_email.clone(),
                            actions: vec![Box::new(move |tx| {
                                tx.send(AppEvent::AlabAccountsPicker {
                                    view: view.clone(),
                                    thread_id,
                                    action: PickerAction::Select(Arc::downgrade(&row)),
                                });
                            })],
                            dismiss_on_select: false,
                            require_explicit_confirmation: true,
                            ..Default::default()
                        }
                    })
                    .collect();
                drop(current);
                self.show_selection_view(SelectionViewParams {
                    title: Some("Switch account".into()),
                    subtitle: Some(
                        "Selecting an account restarts Codex and resumes this conversation.".into(),
                    ),
                    view_id: Some(PICKER_ID),
                    items,
                    description_layout: SelectionDescriptionLayout::SelectedBelowWhenNarrow {
                        min_description_width: 24,
                    },
                    allow_input_when_disconnected: true,
                    ..SelectionViewParams::picker()
                });
            }
            #[cfg(unix)]
            PickerAction::Select(row) if self.bottom_pane.active_view_id() == Some(PICKER_ID) => {
                let Some(row) = row.upgrade() else { return };
                if current.destinations().contains(row.as_ref()) {
                    drop(current);
                    self.alab_accounts_move(
                        "use",
                        &row.label,
                        codex_alab_core::session::shared(),
                        Some((*row).clone()),
                    );
                }
            }
            _ => {}
        }
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
            let writer =
                codex_alab_tui::session::selection_writer(codex_alab_core::session::shared());
            let result = match codex_alab_aterm::Client::connect_accounts_self().await {
                Ok(Some(client)) => {
                    if let Err(message) =
                        super::account_move_pool::reconcile_selection(&client, writer).await
                    {
                        return Err(message.to_owned());
                    }
                    let birth = codex_alab_tui::identity::current()
                        .ok()
                        .and_then(|identity| codex_alab_tui::identity::proc_start_utc(&identity));
                    match birth {
                        Some(birth) => {
                            client
                                .accounts_snapshot_for_display(&birth)
                                .await
                                .map(|snapshot| {
                                    let off = super::account_move_pool::autopilot_is_off(
                                        codex_alab_core::session::shared(),
                                    );
                                    describe(&snapshot, clock_format, off)
                                })
                        }
                        None => Err(codex_alab_aterm::Error::BadRequest(
                            "process identity unavailable".into(),
                        )),
                    }
                }
                Ok(None) => Err(codex_alab_aterm::Error::Closed),
                Err(error) => Err(error),
            };
            result.map_err(|error| {
                if writer.is_some_and(|writer| {
                    writer.manual_selection_state()
                        != codex_alab_tui::session::ManualSelectionState::None
                }) {
                    super::account_move_pool::SELECTION_PENDING.to_owned()
                } else {
                    failure(&error).to_owned()
                }
            })
        };
        #[cfg(not(unix))]
        let fetch =
            async { Err("Native account inspection is unavailable on this platform.".into()) };
        if spawn_snapshot(weak, thread_id, tx, fetch).is_some() {
            self.request_redraw();
        }
    }

    /// `text` for the pane `view` of `thread_id`, shown only while that very pane is open on
    /// that thread. An account move's last word ([`Update::Outcome`]) is said in history
    /// instead where its pane is gone: the person may close it while aterm weighs the move,
    /// and the key that closes it can itself refuse the move.
    pub(crate) fn alab_accounts_ready(
        &mut self,
        view: Weak<Mutex<View>>,
        thread_id: Option<ThreadId>,
        text: Update,
    ) {
        let shown = (self.thread_id() == thread_id
            && self
                .bottom_pane
                .selected_index_for_present_view(VIEW_ID)
                .is_some())
        .then(|| view.upgrade())
        .flatten();
        match (shown, text) {
            (Some(view), text) => {
                let dismiss_picker = match &text {
                    Update::Addresses { result, .. } => *result == AddressResult::Stale,
                    _ => true,
                };
                if let Ok(mut current) = view.lock()
                    && current.apply(text)
                {
                    drop(current);
                    if dismiss_picker {
                        self.bottom_pane.dismiss_view_by_id(PICKER_ID);
                    }
                    self.request_redraw();
                }
            }
            (None, Update::Outcome(text)) => self.add_info_message(text, /*hint*/ None),
            (None, Update::Snapshot { .. } | Update::Observation(_) | Update::Addresses { .. }) => {
            }
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
            assignment: Assignment { account_id: None },
            pool: None,
            accounts: vec![Account {
                id: "opaque".into(),
                label: "Spare subscription".into(),
                masked_email: None,
                condition: Condition::NotEnrolled,
                signed_out_reason: None,
                selection_ref: None,
                enrolled: false,
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
                select_bound: false,
                add: false,
                select: false,
                auto: false,
            },
        };
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert!(description.destinations.is_empty());
        let text = description.text;
        assert!(text.contains("Spare subscription"));
        assert!(text.contains("No login file · cached pool status not enrolled"));
        assert!(text.contains("usage unavailable"));
        assert!(text.contains("stale usage"));
        assert!(!text.contains(" · switch "));
        assert!(!text.contains("0% used"));
        assert!(!text.contains("bash alab/account-labels.sh"));
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        assert!(
            crate::render::renderable::Renderable::desired_height(&paragraph, 20)
                > crate::render::renderable::Renderable::desired_height(&paragraph, 80)
        );
    }

    #[cfg(unix)]
    fn schema_two_snapshot() -> codex_alab_aterm::accounts::Snapshot {
        use codex_alab_aterm::accounts::*;
        let row = |id: &str, label: &str| Account {
            id: id.into(),
            label: label.into(),
            masked_email: None,
            condition: Condition::Usable,
            signed_out_reason: None,
            selection_ref: None,
            enrolled: true,
            held: false,
            eligible: id == "work-id",
            quota: Quota {
                stale: true,
                observed_at: None,
                five_hour_used_percent: None,
                five_hour_reset_at: None,
                weekly_used_percent: None,
                weekly_reset_at: None,
            },
        };
        Snapshot {
            schema: 2,
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
                account_id: Some("main-id".into()),
            },
            pool: Some(Pool {
                choice_id: Some("work-id".into()),
                pinned: true,
                rotation: Rotation::Wall,
            }),
            accounts: vec![
                row("main-id", "main"),
                row("work-id", "work"),
                row("spare-id", "spare"),
            ],
            capabilities: Capabilities {
                select_bound: false,
                add: false,
                select: true,
                auto: true,
            },
        }
    }

    #[cfg(unix)]
    fn bound_snapshot() -> codex_alab_aterm::accounts::Snapshot {
        let mut snapshot = schema_two_snapshot();
        snapshot.schema = 4;
        snapshot.capabilities.select_bound = true;
        snapshot.accounts[1].selection_ref = Some("d".repeat(64));
        snapshot
    }

    #[cfg(unix)]
    #[test]
    fn typed_switches_remain_available_without_a_bound_picker() {
        let mut snapshot = schema_two_snapshot();
        for schema in [2, 3, 4] {
            snapshot.schema = schema;
            let pane = describe(&snapshot, ClockFormat::TwentyFourHour, false);
            assert!(pane.destinations.is_empty());
            assert!(pane.text.contains("/accounts use work"));
            assert!(pane.text.contains("/accounts auto"));
        }
        let mut snapshot = bound_snapshot();
        snapshot.accounts[2].eligible = true;
        let pane = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert_eq!(pane.destinations.len(), 1);
        assert_eq!(pane.destinations[0].label, "work");
        assert_eq!(pane.destinations[0].selection_ref, "d".repeat(64));
        assert!(pane.text.contains("/accounts use spare"));
        assert!(!pane.text.contains(&"d".repeat(64)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn address_replies_only_dismiss_the_switch_picker_for_a_current_stale_roster() {
        use crossterm::event::KeyCode;
        let (mut chat, _tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.apply_external_edit("unfinished draft".into());
        while events.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let view = chat.alab_accounts_view();
        let token = view.upgrade().unwrap().lock().unwrap().begin_snapshot();
        let mut roster = bound_snapshot();
        for row in &mut roster.accounts {
            row.masked_email = Some("m***@example.test".into());
        }
        roster.accounts[2].eligible = true;
        roster.accounts[2].selection_ref = Some("e".repeat(64));
        chat.alab_accounts_ready(
            view.clone(),
            chat.thread_id(),
            Update::Snapshot {
                token,
                result: Ok(describe(&roster, ClockFormat::TwentyFourHour, false)),
            },
        );
        assert!(render_bottom_popup(&chat, 120).contains("show addresses"));
        chat.handle_key_event(KeyCode::Left.into());
        let AppEvent::AlabAccountsPicker {
            action: PickerAction::ShowAddresses(revision),
            ..
        } = events.try_recv().unwrap()
        else {
            panic!("expected the displayed address revision");
        };
        let owner = view.upgrade().unwrap();
        assert_eq!(
            owner.lock().unwrap().begin_addresses(&revision).as_deref(),
            Some(roster.revision.as_str())
        );
        chat.handle_key_event(KeyCode::Right.into());
        let AppEvent::AlabAccountsPicker {
            action, thread_id, ..
        } = events.try_recv().unwrap()
        else {
            panic!("expected switch picker");
        };
        chat.alab_accounts_picker(view.clone(), thread_id, action);
        chat.handle_key_event(KeyCode::Down.into());
        assert_eq!(chat.selected_index_for_active_view(PICKER_ID), Some(1));
        let result = address_failure(&codex_alab_aterm::Error::Refused("ERR unknown verb".into()));
        chat.alab_accounts_ready(
            view.clone(),
            thread_id,
            Update::Addresses {
                revision: revision.clone(),
                result,
            },
        );
        assert_eq!(chat.selected_index_for_active_view(PICKER_ID), Some(1));
        chat.handle_key_event(KeyCode::Esc.into());
        let shown = render_bottom_popup(&chat, 160);
        assert!(shown.contains("slot-label lookup below"));
        assert!(
            owner
                .lock()
                .unwrap()
                .display_text(None)
                .contains("bash alab/account-labels.sh")
        );
        let token = owner.lock().unwrap().begin_snapshot();
        chat.alab_accounts_ready(
            view.clone(),
            thread_id,
            Update::Snapshot {
                token,
                result: Ok(describe(&roster, ClockFormat::TwentyFourHour, false)),
            },
        );
        let current_revision = owner.lock().unwrap().address_action().unwrap();
        assert!(
            owner
                .lock()
                .unwrap()
                .begin_addresses(&current_revision)
                .is_some()
        );
        chat.alab_accounts_picker(view.clone(), thread_id, PickerAction::Open);
        chat.handle_key_event(KeyCode::Down.into());
        chat.alab_accounts_ready(
            view.clone(),
            thread_id,
            Update::Addresses {
                revision,
                result: AddressResult::Stale,
            },
        );
        assert_eq!(chat.selected_index_for_active_view(PICKER_ID), Some(1));
        chat.alab_accounts_ready(
            view,
            thread_id,
            Update::Addresses {
                revision: current_revision,
                result: AddressResult::Stale,
            },
        );
        assert_eq!(chat.bottom_pane.active_view_id(), Some(VIEW_ID));
        let shown = render_bottom_popup(&chat, 160);
        assert!(shown.contains("The account list changed."));
        assert!(!shown.contains("/accounts use") && !shown.contains("/accounts auto"));
        chat.handle_key_event(KeyCode::Left.into());
        chat.handle_key_event(KeyCode::Right.into());
        assert!(events.try_recv().is_err());
        assert!(ops.try_recv().is_err());
        assert_eq!(chat.composer_text_with_pending(), "unfinished draft");
    }

    #[cfg(unix)]
    #[test]
    fn cached_roster_withdraws_switch_commands_until_a_new_snapshot_arrives() {
        let mut roster = bound_snapshot();
        roster.accounts.truncate(2);
        for row in &mut roster.accounts {
            row.masked_email = Some("m***@example.test".into());
        }
        roster.accounts[1].quota.weekly_used_percent = Some(21.0);
        let mut view = View::new(String::new());
        let initial = view.begin_snapshot();
        assert!(view.apply(Update::Snapshot {
            token: initial,
            result: Ok(describe(&roster, ClockFormat::TwentyFourHour, false)),
        }));
        let active = view.display_text(None);
        assert!(active.contains("/accounts use work") && active.contains("/accounts auto"));
        assert_eq!(view.destinations().len(), 1);

        let assert_cached = |view: &View| {
            let shown = view.display_text(None);
            assert!(shown.contains("work · m***@example.test"), "{shown}");
            assert!(shown.contains("Weekly: 21% used"), "{shown}");
            assert!(!shown.contains("/accounts use"), "{shown}");
            assert!(!shown.contains("/accounts auto"), "{shown}");
            assert!(view.destinations().is_empty());
        };
        let pending = view.begin_snapshot();
        assert_cached(&view);
        assert!(view.apply(Update::Snapshot {
            token: pending,
            result: Err("Refresh failed".into()),
        }));
        assert_cached(&view);
        let recovered = view.begin_snapshot();
        assert_cached(&view);
        roster.accounts[1].label = "renamed".into();
        assert!(view.apply(Update::Snapshot {
            token: recovered,
            result: Ok(describe(&roster, ClockFormat::TwentyFourHour, false)),
        }));
        let shown = view.display_text(None);
        assert!(shown.contains("/accounts use renamed") && shown.contains("/accounts auto"));
        assert!(!shown.contains("/accounts use work"));
        assert_eq!(view.destinations()[0].label, "renamed");

        roster.accounts[1].eligible = false;
        let auto_only = view.begin_snapshot();
        assert!(view.apply(Update::Snapshot {
            token: auto_only,
            result: Ok(describe(&roster, ClockFormat::TwentyFourHour, false)),
        }));
        assert!(view.destinations().is_empty());
        assert!(view.display_text(None).contains("/accounts auto"));
        view.begin_snapshot();
        assert!(!view.display_text(None).contains("/accounts auto"));
    }

    #[cfg(unix)]
    #[test]
    fn bound_accounts_name_the_sessions_row_the_pools_choice_and_the_commands() {
        let mut snapshot = bound_snapshot();
        snapshot.accounts[1].masked_email = Some("w***@example.com".into());
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert_eq!(description.destinations.len(), 1);
        let destination = &description.destinations[0];
        assert_eq!(destination.label, "work");
        assert_eq!(
            destination.masked_email.as_deref(),
            Some("w***@example.com")
        );
        assert_eq!(destination.id, "work-id");
        assert_eq!(destination.revision, snapshot.revision);
        let text = description.text;
        assert!(
            text.contains("Session account (by its launch login): main"),
            "{text}"
        );
        assert!(text.contains("Pool's choice: work · pinned"), "{text}");
        assert!(text.contains("\nmain · current session\n"), "{text}");
        assert!(
            text.contains("\nwork · w***@example.com · /accounts use work\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "\nspare · switch unavailable\nLogin file present · cached pool status usable"
            ),
            "{text}"
        );
        assert!(
            text.contains("/accounts use <label> requests a switch to that account."),
            "{text}"
        );
        assert!(
            text.contains("/accounts auto requests the pool's choice."),
            "{text}"
        );
        assert!(
            text.contains("After resume, /status shows the active login."),
            "{text}"
        );
        assert!(!text.contains(CHANGES_UNAVAILABLE));
        assert!(!text.contains("bash alab/account-labels.sh"), "{text}");

        snapshot.capabilities.auto = false;
        let text = describe(&snapshot, ClockFormat::TwentyFourHour, false).text;
        assert!(text.contains("/accounts use <label>") && !text.contains("/accounts auto"));
        snapshot.accounts[1].masked_email = None;
        snapshot.accounts[2].label = "work".into();
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert!(description.destinations.is_empty());
        let text = description.text;
        assert!(!text.contains("/accounts use work"), "{text}");
        assert_eq!(text.matches("\nwork · switch unavailable\n").count(), 2);
        snapshot.accounts[1].label = "help".into();
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert!(description.destinations.is_empty());
        let text = description.text;
        assert!(text.contains("\nhelp · switch unavailable\n"), "{text}");
        assert!(!text.contains("/accounts use help"), "{text}");
        snapshot.capabilities.auto = true;
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert!(description.destinations.is_empty());
        assert!(description.text.contains("/accounts auto"));
        assert!(!description.text.contains("/accounts use"));
        snapshot.capabilities.auto = false;
        snapshot.capabilities.select = false;
        snapshot.pool.as_mut().unwrap().pinned = false;
        snapshot.pool.as_mut().unwrap().choice_id = None;
        snapshot.assignment.account_id = None;
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert!(description.destinations.is_empty());
        let text = description.text;
        assert!(text.ends_with(CHANGES_UNAVAILABLE), "{text}");
        assert!(text.contains("Session account: unavailable"));
        assert!(text.contains("Pool's choice: none"));
        assert!(!text.contains(" · switch "));
        assert!(!text.contains("/accounts use"));
    }

    #[cfg(unix)]
    #[test]
    fn account_pane_explains_switch_refusal_only_for_its_current_snapshot() {
        use codex_alab_aterm::accounts::Rotation;
        let mut roster = schema_two_snapshot();
        roster.capabilities.select = false;
        roster.capabilities.auto = false;
        for (has_pool, rotation, autopilot_off, expected) in [
            (true, Rotation::Wall, true, "Turn Autopilot on"),
            (true, Rotation::Off, true, "Account rotation is off"),
            (true, Rotation::Off, false, "Account rotation is off"),
            (true, Rotation::Wall, false, CHANGES_UNAVAILABLE),
            (false, Rotation::Wall, true, CHANGES_UNAVAILABLE),
        ] {
            let mut current = roster.clone();
            if has_pool {
                current.pool.as_mut().unwrap().rotation = rotation;
            } else {
                current.pool = None;
            }
            let description = describe(&current, ClockFormat::TwentyFourHour, autopilot_off);
            assert!(description.text.contains(expected), "{}", description.text);
            assert!(
                !description.text.contains("/accounts use") && description.destinations.is_empty()
            );
            let cached = description.cached_text.unwrap();
            assert!(!cached.contains(expected) && cached.contains("cached pool status"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn accounts_with_the_same_mask_offer_only_a_serving_alternative() {
        use codex_alab_aterm::accounts::Condition;
        let mut snapshot = bound_snapshot();
        snapshot.accounts.truncate(2);
        for row in &mut snapshot.accounts {
            row.masked_email = Some("m***@example.test".into());
            row.quota.stale = false;
        }
        snapshot.accounts[1].condition = Condition::Limited;
        snapshot.accounts[1].eligible = false;
        snapshot.accounts[1].quota.weekly_used_percent = Some(100.0);
        snapshot.accounts[1].quota.weekly_reset_at = Some(1_800_006_000);
        snapshot.pool.as_mut().unwrap().choice_id = Some("main-id".into());
        snapshot.capabilities.auto = false;

        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert!(description.destinations.is_empty());
        assert!(description.shared_mask);
        assert_eq!(
            description.address_revision.as_deref(),
            Some(snapshot.revision.as_str())
        );
        let text = description.text;
        assert!(
            text.contains("main · current session · m***@example.test"),
            "{text}"
        );
        assert!(text.contains("Check their slot labels."), "{text}");
        assert_eq!(
            text.matches("bash alab/account-labels.sh").count(),
            1,
            "{text}"
        );
        assert!(
            text.contains("work · m***@example.test · switch unavailable (usage limit)"),
            "{text}"
        );
        let weekly = text
            .lines()
            .find(|line| line.starts_with("Weekly: 100% used"))
            .unwrap();
        assert!(
            weekly.contains("resets ") && !weekly.contains("unavailable"),
            "{weekly}"
        );
        assert!(
            text.contains("No other account is available to select by label."),
            "{text}"
        );
        assert!(
            !text.contains("/accounts use") && !text.contains("/accounts auto"),
            "{text}"
        );

        snapshot.assignment.account_id = Some("work-id".into());
        snapshot.accounts[0].eligible = true;
        snapshot.accounts[0].selection_ref = Some("e".repeat(64));
        snapshot.capabilities.auto = true;
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert_eq!(description.destinations.len(), 1);
        assert_eq!(description.destinations[0].id, "main-id");
        let text = description.text;
        assert!(
            text.contains("main · m***@example.test · /accounts use main"),
            "{text}"
        );
        assert!(
            text.contains("work · current session · m***@example.test"),
            "{text}"
        );
        assert!(!text.contains("No other account is available"), "{text}");

        snapshot.accounts[1].masked_email = Some("w***@example.test".into());
        let description = describe(&snapshot, ClockFormat::TwentyFourHour, false);
        assert!(!description.shared_mask);
        assert!(description.address_revision.is_none());
        let text = description.text;
        assert!(!text.contains("bash alab/account-labels.sh"), "{text}");
    }

    #[cfg(unix)]
    #[test]
    fn signed_out_rows_show_their_recorded_reasons_without_changing_the_quota_row() {
        use codex_alab_aterm::accounts::SignedOutReason;
        let mut snapshot = schema_two_snapshot();
        snapshot.schema = 3;
        snapshot.accounts[0].condition = Condition::SignedOut;
        snapshot.accounts[1].condition = Condition::Limited;
        snapshot.accounts[1].eligible = false;
        snapshot.accounts[1].quota.weekly_used_percent = Some(100.0);
        snapshot.accounts[2].condition = Condition::SignedOut;

        for (reason, words) in [
            (SignedOutReason::RefreshTokenExpired, "token expired"),
            (SignedOutReason::RefreshTokenReused, "token reused"),
            (
                SignedOutReason::RefreshTokenInvalidated,
                "token invalidated",
            ),
            (SignedOutReason::InvalidGrant, "invalid grant"),
            (SignedOutReason::Unknown, "reason unknown"),
        ] {
            snapshot.accounts[0].signed_out_reason = Some(reason);
            let text = describe(&snapshot, ClockFormat::TwentyFourHour, false).text;
            assert!(text.contains(&format!(
                "main · current session\nLogin file present · cached pool status signed out ({words})"
            )), "{text}");
            assert!(text.contains(
                "work · switch unavailable (usage limit)\nLogin file present · cached pool status limited · stale usage"
            ), "{text}");
            assert!(text.contains("Weekly: 100% used"), "{text}");
            assert!(text.contains(
                "spare · switch unavailable\nLogin file present · cached pool status signed out · stale usage"
            ), "{text}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn accounts_errors_never_echo_host_data() {
        let error = codex_alab_aterm::Error::Refused("ERR codex-accounts denied".into());
        assert!(failure(&error).contains("authorize"));
        let error = codex_alab_aterm::Error::Refused("ERR private token or path".into());
        assert!(!failure(&error).contains("private token"));
        assert_eq!(address_failure(&error), AddressResult::Unavailable);
        let unknown = codex_alab_aterm::Error::Refused("ERR unknown verb".into());
        assert_eq!(address_failure(&unknown), AddressResult::Unsupported);
        let stale = codex_alab_aterm::Error::Refused("ERR codex-accounts-display stale".into());
        assert_eq!(address_failure(&stale), AddressResult::Stale);
    }
}
