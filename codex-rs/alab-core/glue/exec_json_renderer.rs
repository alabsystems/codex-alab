// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde::Deserialize;
use serde::Serialize;

#[cfg(test)]
#[path = "exec_test_process.rs"]
mod test_process;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    kind: String,
    version: u8,
    next_item_id: u64,
    raw_to_exec_item_id: HashMap<String, String>,
    running_todo_list: Option<Todo>,
    last_total_token_usage: Option<ThreadTokenUsage>,
    last_critical_error: Option<ThreadErrorEvent>,
    final_message: Option<String>,
    emit_final_message_on_shutdown: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Todo {
    item_id: String,
    items: Vec<TodoItem>,
}

pub(super) fn snapshot(
    renderer: &EventProcessorWithJsonOutput,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(Snapshot {
        kind: "json".into(),
        version: 1,
        next_item_id: renderer.next_item_id.load(Ordering::SeqCst),
        raw_to_exec_item_id: renderer.raw_to_exec_item_id.clone(),
        running_todo_list: renderer.running_todo_list.as_ref().map(|todo| Todo {
            item_id: todo.item_id.clone(),
            items: todo.items.clone(),
        }),
        last_total_token_usage: renderer.last_total_token_usage.clone(),
        last_critical_error: renderer.last_critical_error.clone(),
        final_message: renderer.final_message.clone(),
        emit_final_message_on_shutdown: renderer.emit_final_message_on_shutdown,
    })
    .map_err(|_| "JSON renderer checkpoint could not be encoded".into())
}

pub(super) fn restore(
    renderer: &mut EventProcessorWithJsonOutput,
    value: serde_json::Value,
) -> Result<(), String> {
    let state: Snapshot =
        serde_json::from_value(value).map_err(|_| "invalid JSON renderer checkpoint".to_owned())?;
    let valid_id = |id: &str| {
        id.strip_prefix("item_")
            .and_then(|n| n.parse::<u64>().ok())
            .is_some_and(|n| n < state.next_item_id && format!("item_{n}") == id)
    };
    let mut ids = std::collections::HashSet::new();
    if state.kind != "json"
        || state.version != 1
        || state.next_item_id == u64::MAX
        || state
            .raw_to_exec_item_id
            .values()
            .any(|id| !valid_id(id) || !ids.insert(id.as_str()))
        || state
            .running_todo_list
            .as_ref()
            .is_some_and(|todo| !valid_id(&todo.item_id) || ids.contains(todo.item_id.as_str()))
    {
        return Err("inconsistent JSON renderer checkpoint".into());
    }
    drop(ids);
    renderer
        .next_item_id
        .store(state.next_item_id, Ordering::SeqCst);
    renderer.raw_to_exec_item_id = state.raw_to_exec_item_id;
    renderer.running_todo_list = state.running_todo_list.map(|todo| RunningTodoList {
        item_id: todo.item_id,
        items: todo.items,
    });
    renderer.last_total_token_usage = state.last_total_token_usage;
    renderer.last_critical_error = state.last_critical_error;
    renderer.final_message = state.final_message;
    renderer.emit_final_message_on_shutdown = state.emit_final_message_on_shutdown;
    Ok(())
}

pub(super) fn interrupted_status() -> Option<CollabToolCallStatus> {
    codex_alab_core::active().then_some(CollabToolCallStatus::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_wait_completes_the_original_visible_item() {
        if !test_process::child(concat!(
            module_path!(),
            "::interrupted_wait_completes_the_original_visible_item"
        )) {
            return;
        }
        codex_alab_core::activate();
        let mut renderer = EventProcessorWithJsonOutput::new(None);
        let item = |status| ThreadItemPayload::CollabAgentToolCall {
            id: "wait".into(),
            tool: CollabAgentTool::Wait,
            status,
            sender_thread_id: "root".into(),
            receiver_thread_ids: Vec::new(),
            prompt: None,
            model: None,
            reasoning_effort: None,
            agents_states: Default::default(),
        };
        let started = renderer
            .map_started_item(item(CollabAgentToolCallStatus::InProgress))
            .unwrap();
        let completed = renderer
            .map_completed_item_mut(item(CollabAgentToolCallStatus::Interrupted))
            .unwrap();
        assert_eq!(started.id, completed.id);
        assert!(matches!(
            completed.details,
            ThreadItemDetails::CollabToolCall(CollabToolCallItem {
                status: CollabToolCallStatus::Failed,
                ..
            })
        ));
        assert!(renderer.raw_to_exec_item_id.is_empty());
    }

    #[test]
    fn json_renderer_handoff_retains_open_items_and_monotonic_ids() {
        let mut old = EventProcessorWithJsonOutput::new(None);
        old.collect_warning("earlier warning".into());
        old.running_todo_list = Some(RunningTodoList {
            item_id: old.next_item_id(),
            items: vec![TodoItem {
                text: "original plan".into(),
                completed: true,
            }],
        });
        old.started_item_id("running-command");
        let state = snapshot(&old).unwrap();
        let mut next = EventProcessorWithJsonOutput::new(None);
        restore(&mut next, state.clone()).unwrap();
        assert_eq!(snapshot(&next).unwrap(), state);
        assert_eq!(next.completed_item_id("running-command"), "item_2");
        assert_eq!(next.next_item_id(), "item_3");
        assert_eq!(next.running_todo_list.as_ref().unwrap().item_id, "item_1");
        let mut invalid = state;
        invalid["next_item_id"] = 1.into();
        assert!(restore(&mut next, invalid).is_err());
        assert_eq!(next.next_item_id(), "item_4");
        let completed = next.collect_thread_events(ServerNotification::TurnCompleted(
            codex_app_server_protocol::TurnCompletedNotification {
                thread_id: "thread".into(),
                turn: codex_app_server_protocol::Turn {
                    id: "turn".into(),
                    root_turn_id: None,
                    items_view: codex_app_server_protocol::TurnItemsView::Full,
                    items: Vec::new(),
                    status: TurnStatus::Completed,
                    error: None,
                    started_at: None,
                    completed_at: Some(0),
                    duration_ms: None,
                },
            },
        ));
        let events = serde_json::to_value(completed.events).unwrap();
        assert!(
            events
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["type"] == "item.completed"
                    && event["item"]["id"] == "item_1"
                    && event["item"]["type"] == "todo_list")
        );
        assert!(next.running_todo_list.is_none());
    }
}
