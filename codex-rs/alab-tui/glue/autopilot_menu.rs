// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Codex owns its Autopilot picker. aterm supplies the shared policy, never the popup.

use super::ChatWidget;
use super::interface;
use super::send_autopilot_request;
use crate::activation_key_guard::ActivationKeySuppression;
use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;
use crate::chatwidget::ExternalEditorState;
use crate::key_hint::KeyBinding;
use crate::key_hint::KeyBindingListExt;
use crate::keymap::ListAction;
use crate::keymap::ListKeymap;
use codex_alab_tui::interface::AUTOPILOT_PICKER_ID as VIEW_ID;
use codex_alab_tui::session::HarnessWord;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Instant;

const DEADLINE_EXAMPLES: &str = "18:00 or +2h";

fn timed_choices() -> Vec<SelectionItem> {
    let mut items = Vec::new();
    for (name, when) in [
        ("Cruise for 1 hour", "+1h"),
        ("Cruise for 2 hours", "+2h"),
        ("Cruise for 4 hours", "+4h"),
    ] {
        items.push(SelectionItem {
            name: name.into(),
            actions: vec![Box::new(move |tx| {
                send_autopilot_request(tx, interface::set_cruise_until(when.into()))
            })],
            dismiss_on_select: true,
            ..Default::default()
        });
    }
    items.push(SelectionItem {
        name: "Cruise until…".into(),
        description: Some(DEADLINE_EXAMPLES.into()),
        actions: vec![Box::new(|tx| tx.send(AppEvent::OpenAlabAutopilotTimer))],
        dismiss_parent_on_child_accept: true,
        ..Default::default()
    });
    items
}

pub(in crate::chatwidget) struct AutopilotMenu {
    status: interface::AutopilotStatus,
    navigated: Arc<AtomicBool>,
}

impl AutopilotMenu {
    fn mode_index(&self) -> Option<usize> {
        match self.status.mode {
            HarnessWord::Cruise => Some(0),
            HarnessWord::Land => Some(1),
            HarnessWord::Off => Some(2),
            _ => None,
        }
    }

    fn params(&self, focus: Option<usize>, keymap: &ListKeymap) -> SelectionViewParams {
        let selected = self.status.mode;
        let initial_selected_idx = focus.or_else(|| self.mode_index());
        let disabled = self.mode_index().is_none();
        let mut items = Vec::new();
        for (name, description, mode) in [
            (
                "Cruise",
                "Continue after each task. Clear any landing timer.",
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
                description: Some(description.into()),
                actions: vec![Box::new(move |tx| {
                    send_autopilot_request(tx, interface::set_harness_mode(mode))
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
        }
        items.extend(timed_choices());
        let footer_hint = if disabled {
            for item in &mut items {
                item.is_disabled = true;
                item.disabled_gutter_marker = Some("–");
            }
            let mut back = Line::default();
            if let Some(key) = keymap.primary_hint(ListAction::Cancel) {
                back = Line::from(key.spans());
                back.push_span(" back".dim());
            }
            // An empty custom hint also suppresses the standard select hint if cancel is unbound.
            Some(back)
        } else {
            None
        };
        let navigated = Arc::clone(&self.navigated);
        SelectionViewParams {
            title: Some("Autopilot".into()),
            subtitle: Some(self.status.picker_subtitle()),
            // The native scroll indicator already separates the header from the mode choices.
            header_gap: 0,
            footer_hint,
            view_id: Some(VIEW_ID),
            allow_input_when_disconnected: true,
            initial_selected_idx,
            on_selection_changed: Some(Box::new(move |_, _| {
                // An all-disabled list can highlight its first row during construction.
                if !disabled {
                    navigated.store(true, Ordering::Relaxed);
                }
            })),
            items,
            ..SelectionViewParams::picker()
        }
    }
}

impl ChatWidget {
    pub(in crate::chatwidget) fn alab_autopilot_shortcut_available(&self) -> bool {
        !self.chat_keymap.open_autopilot.is_empty() && self.alab_autopilot_input_available()
    }

    pub(crate) fn alab_autopilot_input_available(&self) -> bool {
        interface::enabled()
            && !self.blocks_direct_input
            && self.bottom_pane.composer_input_enabled()
            && self.can_launch_external_editor()
            && !self.shortcut_overlay_visible()
            && !self.quick_dial_menu
            && !self.fork_in_progress
            && self.input_queue.startup_submission.is_none()
            && self.safety_recourse.is_none()
            && !self.has_provider_error()
            && !self.has_pending_protected_request()
            && matches!(self.external_editor_state, ExternalEditorState::Closed)
    }

    pub(in crate::chatwidget) fn alab_handle_autopilot_key(
        &mut self,
        key: KeyEvent,
        activation_key: Option<KeyBinding>,
    ) -> bool {
        self.alab_handle_autopilot_input(key, activation_key, /*allow_draft_command*/ false)
    }

    pub(crate) fn alab_handle_autopilot_input(
        &mut self,
        key: KeyEvent,
        activation_key: Option<KeyBinding>,
        allow_draft_command: bool,
    ) -> bool {
        let shortcut = self.chat_keymap.open_autopilot.is_pressed(key);
        if key.kind != KeyEventKind::Press
            || !(shortcut || allow_draft_command)
            || !self.alab_autopilot_input_available()
        {
            return false;
        }

        if shortcut {
            self.alab_open_autopilot_menu();
        } else if let Some(command) = self.bottom_pane.alab_submit_autopilot_menu(key) {
            self.handle_composer_input_result(command, /*had_modal_or_popup*/ false);
            if !self.active_view_is(VIEW_ID) {
                return true;
            }
        } else {
            return false;
        }
        // A held submit key or chord terminator can also activate a picker choice.
        self.suppress_current_view_key_until_release(
            activation_key.unwrap_or_else(|| KeyBinding::from_event(key)),
            Instant::now(),
            ActivationKeySuppression::ViewTransition,
        );
        true
    }

    pub(super) fn alab_open_autopilot_menu(&mut self) {
        let menu = AutopilotMenu {
            status: interface::AutopilotStatus::read(),
            navigated: Arc::default(),
        };
        self.show_selection_view(menu.params(None, &self.bottom_pane.list_keymap()));
        self.alab_controls.menu = Some(menu);
    }

    pub(in crate::chatwidget) fn alab_refresh_autopilot_menu(&mut self) {
        let Some(menu) = &mut self.alab_controls.menu else {
            return;
        };
        let Some(selected) = self.bottom_pane.selected_index_for_present_view(VIEW_ID) else {
            self.alab_controls.menu = None;
            return;
        };
        let status = interface::AutopilotStatus::read();
        if status == menu.status {
            return;
        }
        // Numeric shortcuts select and accept a row directly without a navigation callback.
        let keep_focus = menu
            .mode_index()
            .is_some_and(|old_mode| selected != old_mode || menu.navigated.load(Ordering::Relaxed));
        menu.status = status;
        let keep_focus = keep_focus && menu.mode_index().is_some();
        menu.navigated.store(keep_focus, Ordering::Relaxed);
        let focus = keep_focus.then_some(selected);
        let params = menu.params(focus, &self.bottom_pane.list_keymap());
        self.bottom_pane
            .replace_selection_view_if_present(VIEW_ID, params);
    }

    pub(crate) fn alab_open_autopilot_timer(&mut self) {
        if self.bottom_pane.active_view_id() != Some(VIEW_ID)
            || !interface::autopilot_selection_available()
        {
            return;
        }
        let tx = self.app_event_tx.clone();
        let mut view = CustomPromptView::new(
            "Cruise until…".into(),
            DEADLINE_EXAMPLES.into(),
            String::new(),
            None,
            Box::new(move |when: String| {
                send_autopilot_request(&tx, interface::set_cruise_until(when))
            }),
        );
        view.local_submit_available = Some(interface::autopilot_selection_available);
        view.validate_submission = Some(interface::validate_cruise_until);
        self.bottom_pane.show_text_prompt(view);
    }
}
