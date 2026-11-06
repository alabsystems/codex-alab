// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Codex owns its Autopilot picker. aterm supplies the shared policy, never the popup.

use super::ChatWidget;
use super::interface;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;
use crate::history_cell;
use codex_alab_tui::session::HarnessWord;
use codex_alab_tui::session::Shared;

fn send_request(
    tx: AppEventSender,
    request: impl std::future::Future<Output = Result<(), String>> + Send + 'static,
    context: &'static str,
) {
    tokio::spawn(async move {
        if let Err(error) = request.await {
            tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(format!("{context}: {error}")),
            )));
        }
    });
}

fn timed_choices(disabled: bool, armed: bool) -> Vec<SelectionItem> {
    let mut items = Vec::new();
    for (name, when) in [
        ("Cruise for 1 hour", "+1h"),
        ("Cruise for 2 hours", "+2h"),
        ("Cruise for 4 hours", "+4h"),
    ] {
        items.push(SelectionItem {
            name: name.into(),
            is_disabled: disabled,
            disabled_reason: disabled.then(interface::autopilot_words),
            description: Some("Switch to Land when the timer ends.".into()),
            actions: vec![Box::new(move |tx| {
                send_request(
                    tx.clone(),
                    interface::set_cruise_until(when.into()),
                    "Could not set Autopilot timer",
                )
            })],
            dismiss_on_select: true,
            ..Default::default()
        });
    }
    items.push(SelectionItem {
        name: "Cruise until…".into(),
        description: Some("Choose a time or duration, then land.".into()),
        is_disabled: disabled,
        disabled_reason: disabled.then(interface::autopilot_words),
        actions: vec![Box::new(|tx| tx.send(AppEvent::OpenAlabAutopilotTimer))],
        dismiss_on_select: true,
        ..Default::default()
    });
    items.push(SelectionItem {
        name: "Cancel scheduled landing".into(),
        description: Some("Keep cruising without a deadline.".into()),
        is_disabled: disabled || !armed,
        actions: vec![Box::new(|tx| {
            send_request(
                tx.clone(),
                interface::set_harness_mode(HarnessWord::Cruise),
                "Could not cancel scheduled landing",
            )
        })],
        dismiss_on_select: true,
        ..Default::default()
    });
    items
}

impl ChatWidget {
    pub(super) fn alab_open_autopilot_menu(&mut self) {
        let shared = codex_alab_tui::boxes::tick_shared();
        let selected = shared.map_or(HarnessWord::Unknown, Shared::harness_word);
        let initial_selected_idx = match selected {
            HarnessWord::Cruise => Some(0),
            HarnessWord::Land => Some(1),
            HarnessWord::Off => Some(2),
            _ => None,
        };
        let disabled = initial_selected_idx.is_none();
        let mut items = Vec::new();
        for (name, description, mode) in [
            (
                "Cruise",
                "Continue working, including after a task is finished.",
                HarnessWord::Cruise,
            ),
            (
                "Land",
                "Finish and land the current work, then stop.",
                HarnessWord::Land,
            ),
            (
                "Off",
                "Wait for your messages between turns.",
                HarnessWord::Off,
            ),
        ] {
            items.push(SelectionItem {
                name: name.into(),
                is_current: selected == mode,
                is_disabled: disabled,
                disabled_reason: disabled.then(interface::autopilot_words),
                description: Some(description.into()),
                actions: vec![Box::new(move |tx| {
                    send_request(
                        tx.clone(),
                        interface::set_harness_mode(mode),
                        "Could not change Autopilot",
                    )
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
        }
        items.extend(timed_choices(
            disabled,
            shared.and_then(Shared::land_at).is_some(),
        ));
        self.show_selection_view(SelectionViewParams {
            title: Some("Autopilot".into()),
            subtitle: Some(
                interface::autopilot_words()
                    .lines()
                    .next()
                    .unwrap_or("Autopilot")
                    .to_owned(),
            ),
            view_id: Some("alab-autopilot"),
            allow_input_when_disconnected: true,
            initial_selected_idx,
            items,
            ..SelectionViewParams::picker()
        });
    }

    pub(crate) fn alab_open_autopilot_timer(&mut self) {
        if !interface::enabled() {
            return;
        }
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            "Cruise until…".into(),
            "Time (18:00) or duration (+2h), then land".into(),
            String::new(),
            None,
            Box::new(move |when: String| {
                send_request(
                    tx.clone(),
                    interface::set_cruise_until(when.trim().to_owned()),
                    "Could not set Autopilot timer",
                )
            }),
        );
        self.bottom_pane.show_text_prompt(view);
    }
}
