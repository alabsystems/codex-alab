use std::sync::Arc;

use codex_extension_api::ExtensionEventSink;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadGoal;
use codex_protocol::protocol::ThreadGoalUpdatedEvent;

#[derive(Clone)]
pub(crate) struct GoalEventEmitter {
    sink: Arc<dyn ExtensionEventSink>,
}

impl GoalEventEmitter {
    pub(crate) fn alab_runtime_inline_snapshot(
        &self,
        update: codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification,
    ) -> Option<codex_protocol::protocol::AlabGoalRuntimeSnapshot> {
        self.sink.alab_goal_runtime_inline_snapshot(update)
    } // ALAB(storage-status): native listener reattach uses the same host sequence.
    pub(crate) fn alab_runtime_updated(
        &self,
        update: codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification,
    ) {
        self.sink.alab_goal_runtime_updated(update);
    } // ALAB(storage-status): passive metadata never enters rollouts.

    pub(crate) fn new(sink: Arc<dyn ExtensionEventSink>) -> Self {
        Self { sink }
    }

    pub(crate) fn thread_goal_updated(
        &self,
        event_id: impl Into<String>,
        turn_id: Option<String>,
        goal: ThreadGoal,
    ) {
        self.sink.emit(Event {
            id: event_id.into(),
            msg: EventMsg::ThreadGoalUpdated(ThreadGoalUpdatedEvent {
                thread_id: goal.thread_id,
                turn_id,
                goal,
            }),
        });
    }
}
