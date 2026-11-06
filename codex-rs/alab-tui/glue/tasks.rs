// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One local view over the native checklist, agent navigation and running terminals.
//! The checklist source is the newest native plan cell, including an explicit empty
//! plan. Refreshes read current state, do no network polling, and never start a daemon.

use super::App;
use super::alab_tasks_view;
use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionDescriptionLayout;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::picker_hint_line_for_keymap;
use crate::history_cell::PlanUpdateCell;
use crate::multi_agents::format_agent_picker_item_name;
use codex_alab_tui::interface;
use codex_protocol::ThreadId;
use codex_protocol::plan_tool::StepStatus;

struct Task {
    key: String,
    name: String,
    category: &'static str,
    summary: String,
    detail: String,
    agent: Option<ThreadId>,
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn detail_text(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_control() && ch != '\n' {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

impl App {
    fn alab_task_plan(&self) -> Option<&PlanUpdateCell> {
        self.transcript_cells
            .iter()
            .rev()
            .find_map(|cell| cell.as_any().downcast_ref::<PlanUpdateCell>())
    }

    fn alab_tasks(&self) -> Vec<Task> {
        let mut tasks = Vec::new();
        if let Some(plan) = self.alab_task_plan() {
            let mut occurrences = std::collections::HashMap::new();
            for item in plan.alab_task_plan() {
                let occurrence = occurrences.entry(item.step.as_str()).or_insert(0_usize);
                let key = format!("plan:{occurrence}:{}", item.step);
                *occurrence += 1;
                let (marker, status) = match item.status {
                    StepStatus::Pending => ("[ ]", "Pending"),
                    StepStatus::InProgress => ("[~]", "In progress"),
                    StepStatus::Completed => ("[x]", "Completed"),
                };
                tasks.push(Task {
                    key,
                    name: format!("{marker} {}", plain(&item.step)),
                    category: "PLAN",
                    summary: status.to_string(),
                    detail: format!("{status}\n\n{}", detail_text(&item.step)),
                    agent: None,
                });
            }
        }
        for (id, entry) in self.agent_navigation.visible_threads() {
            if Some(id) == self.primary_thread_id {
                continue;
            }
            let name = format_agent_picker_item_name(
                entry.agent_nickname.as_deref(),
                entry.agent_role.as_deref(),
                false,
            );
            let status = if entry.is_closed {
                "Closed"
            } else if entry.is_running {
                "Working"
            } else {
                "Idle"
            };
            tasks.push(Task {
                key: format!("agent:{id}"),
                name: plain(&name),
                category: "AGENT",
                summary: status.to_string(),
                detail: String::new(),
                agent: Some(id),
            });
        }
        for terminal in self.chat_widget.alab_terminal_tasks() {
            tasks.push(Task {
                key: format!("terminal:{}", terminal.key),
                name: plain(&terminal.command),
                category: "TERMINAL",
                summary: "Running · inspect recent output".into(),
                detail: if terminal.output.is_empty() {
                    "No output yet. Use /ps to print running terminals or /stop to stop them."
                        .into()
                } else {
                    detail_text(&terminal.output.join("\n"))
                },
                agent: None,
            });
        }
        tasks
    }

    fn alab_tasks_view(
        &self,
        selected_key: Option<&str>,
        query: Option<String>,
    ) -> Box<dyn crate::bottom_pane::BottomPaneView> {
        let thread_id = self.chat_widget.thread_id();
        let tasks = self.alab_tasks();
        let mut keys = Vec::new();
        let mut items = Vec::new();
        for task in tasks {
            let disabled_reason = (self.reconnect.offline && task.agent.is_some())
                .then(|| "Reconnect to open".to_string());
            let description = disabled_reason.is_none().then_some(task.summary);
            keys.push(task.key.clone());
            let key = task.key;
            let action: crate::bottom_pane::SelectionAction = if let Some(agent) = task.agent {
                Box::new(move |tx| tx.send(AppEvent::SelectAgentThread(agent)))
            } else {
                Box::new(move |tx| {
                    tx.send(AppEvent::AlabTaskDetails {
                        thread_id,
                        key: key.clone(),
                    })
                })
            };
            items.push(SelectionItem {
                name: task.name,
                description,
                category_tag: Some(task.category.into()),
                disabled_reason,
                actions: vec![action],
                dismiss_on_select: true,
                ..Default::default()
            });
        }
        if items.is_empty() {
            keys.push("empty".into());
            items.push(SelectionItem {
                name: "No tasks yet".into(),
                description: Some(
                    "Checklists, local agents and running terminals appear here.".into(),
                ),
                is_disabled: true,
                ..Default::default()
            });
        }
        let keymap = self.chat_widget.alab_task_list_keymap();
        // Connected servers own their tool exposure; this client config only
        // proves the checklist setting for the embedded server it starts.
        let subtitle = if matches!(&self.app_server_target, crate::AppServerTarget::Embedded)
            && !self.config.update_plan_enabled
            && self.alab_task_plan().is_none()
        {
            "Checklist tool is off · /alab help"
        } else {
            "Checklist · Agents · Background terminals"
        };
        let params = SelectionViewParams {
            title: Some("Tasks".into()),
            subtitle: Some(subtitle.into()),
            initial_selected_idx: selected_key
                .and_then(|key| keys.iter().position(|candidate| candidate == key)),
            initial_search_query: query,
            footer_hint: Some(picker_hint_line_for_keymap(&keymap)),
            description_layout: SelectionDescriptionLayout::HideWhenNarrow {
                min_description_width: 20,
            },
            items,
            ..SelectionViewParams::picker()
        };
        alab_tasks_view::view(params, keys, thread_id, self.app_event_tx.clone(), keymap)
    }

    pub(super) fn alab_open_tasks(&mut self) {
        if interface::enabled() {
            let view = self.alab_tasks_view(None, None);
            self.chat_widget.show_bottom_pane_view(view);
        } else if interface::active() {
            self.chat_widget.add_info_message(
                "Turn on the ALab interface with /alab on to use Tasks.".into(),
                None,
            );
        }
    }

    pub(super) fn alab_tasks_back(&mut self, thread_id: Option<ThreadId>) {
        if interface::enabled() && self.chat_widget.thread_id() == thread_id {
            self.alab_open_tasks();
        }
    }

    /// A disconnect keeps the native local picker open. Disable agent actions
    /// immediately rather than waiting for its next refresh frame.
    pub(super) fn alab_tasks_connection_changed(&mut self) {
        if !interface::enabled() {
            return;
        }
        let Some((index, query)) = self.chat_widget.alab_task_view_state() else {
            return;
        };
        let tasks = self.alab_tasks();
        let selected_key = index
            .and_then(|index| tasks.get(index))
            .map(|task| task.key.as_str());
        let view = self.alab_tasks_view(selected_key, query);
        self.chat_widget
            .replace_bottom_pane_view_if_present(alab_tasks_view::VIEW_ID, view);
    }

    pub(super) fn alab_refresh_tasks(
        &mut self,
        view: std::sync::Weak<()>,
        thread_id: Option<ThreadId>,
        selected_key: Option<String>,
        selected_index: Option<usize>,
        query: Option<String>,
    ) {
        // Closing or replacing a picker invalidates even a queued refresh with the
        // same thread, query and index as a newly opened picker.
        if view.upgrade().is_none()
            || !interface::enabled()
            || self.chat_widget.thread_id() != thread_id
        {
            return;
        }
        // A queued timer must not replace a newer search query or keyboard selection.
        if self.chat_widget.alab_task_view_state() != Some((selected_index, query.clone())) {
            return;
        }
        let view = self.alab_tasks_view(selected_key.as_deref(), query);
        self.chat_widget
            .replace_bottom_pane_view_if_present(alab_tasks_view::VIEW_ID, view);
    }

    fn alab_task_details_content(&self, key: &str) -> (&'static str, String) {
        let task = self.alab_tasks().into_iter().find(|task| task.key == key);
        task.map_or_else(
            || {
                (
                    "Task changed",
                    "This task is no longer in the current task list.".into(),
                )
            },
            |task| match task.category {
                "PLAN" => ("Checklist step", task.detail),
                _ => (
                    "Terminal output",
                    format!("Command: {}\n\n{}", task.name, task.detail),
                ),
            },
        )
    }

    pub(super) fn alab_task_details(&mut self, thread_id: Option<ThreadId>, key: &str) {
        if !interface::enabled() || self.chat_widget.thread_id() != thread_id {
            return;
        }
        let (title, detail) = self.alab_task_details_content(key);
        self.chat_widget.alab_show_task_details(title, detail);
    }
}

#[cfg(test)]
#[path = "tasks_tests.rs"]
mod tests;
