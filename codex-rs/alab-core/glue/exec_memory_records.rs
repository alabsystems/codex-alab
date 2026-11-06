// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private memory records preserve host state that the durable rollout wire
//! format deliberately transforms or omits. Never use them as provider input.

use super::codec::MemoryItem;
use codex_history::CompactedItem;
use codex_history::CompactionResumeMetadata;
use codex_history::GuardianHistoryCheckpoint;
use codex_history::RetainedContext;
use codex_history::RetainedContextEvent;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::mcp::McpResourceOriginCheckpoint;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::PrivateSessionEvent;
use codex_protocol::protocol::PrivateSessionEventPayload;
use codex_protocol::protocol::RawResponseItemEvent;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::TokenUsageRecord;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnContextItem;
use codex_protocol::protocol::WorldStateItem;
use codex_protocol::realtime::RealtimeItem;
use codex_protocol::security_risk::SecurityRiskScore;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(super) enum MemoryRecord {
    SessionMeta(Box<SessionMetaLine>),
    DynamicToolsUpdated {
        thread_id: ThreadId,
        tools: Vec<DynamicToolSpec>,
    },
    Response(MemoryItem),
    InterAgentCommunication(InterAgentCommunication),
    InterAgentCommunicationMetadata {
        trigger_turn: bool,
    },
    Compacted(Box<MemoryCompacted>),
    TurnContext(TurnContextItem),
    TokenUsageRecord(TokenUsageRecord),
    WorldState(WorldStateItem),
    SecurityRiskScore(SecurityRiskScore),
    RetainedContext(RetainedContextEvent),
    Event(MemoryEvent),
    RealtimeItem(RealtimeItem),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(super) enum MemoryEvent {
    Ordinary(EventMsg),
    Raw(MemoryItem),
    PrivateRecorded {
        thread: ThreadId,
        item: MemoryItem,
    },
    PrivateAborted {
        thread: ThreadId,
        event: TurnAbortedEvent,
    },
}

impl MemoryEvent {
    fn capture(event: &EventMsg) -> Result<Self, String> {
        let item = |item: &codex_protocol::models::ResponseItem| {
            MemoryItem::capture(&codex_history::ResponseItemEnvelope {
                item: item.clone(),
                metadata: None,
            })
        };
        Ok(match event {
            EventMsg::RawResponseItem(event) => Self::Raw(item(&event.item)?),
            EventMsg::PrivateSessionEvent(event) => match &event.event {
                PrivateSessionEventPayload::RecordedItem(recorded) => Self::PrivateRecorded {
                    thread: event.thread_id,
                    item: item(&recorded.item)?,
                },
                PrivateSessionEventPayload::TurnAborted(aborted) => Self::PrivateAborted {
                    thread: event.thread_id,
                    event: aborted.clone(),
                },
            },
            event => Self::Ordinary(event.clone()),
        })
    }

    fn restore(self) -> Result<EventMsg, String> {
        Ok(match self {
            Self::Ordinary(EventMsg::RawResponseItem(_) | EventMsg::PrivateSessionEvent(_)) => {
                return Err("memory event is missing internal state".into());
            }
            Self::Ordinary(event) => event,
            Self::Raw(item) => EventMsg::RawResponseItem(RawResponseItemEvent {
                item: item.restore()?.item,
            }),
            Self::PrivateRecorded { thread, item } => {
                EventMsg::PrivateSessionEvent(PrivateSessionEvent {
                    thread_id: thread,
                    event: PrivateSessionEventPayload::RecordedItem(RawResponseItemEvent {
                        item: item.restore()?.item,
                    }),
                })
            }
            Self::PrivateAborted { thread, event } => {
                EventMsg::PrivateSessionEvent(PrivateSessionEvent {
                    thread_id: thread,
                    event: PrivateSessionEventPayload::TurnAborted(event),
                })
            }
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MemoryCompacted {
    message: String,
    replacement_history: Option<Vec<MemoryItem>>,
    guardian_history: Option<Vec<MemoryItem>>,
    retained_context: Option<RetainedContext>,
    mcp_resource_origins: Option<McpResourceOriginCheckpoint>,
    window_number: Option<u64>,
    first_window_id: Option<String>,
    previous_window_id: Option<String>,
    window_id: Option<String>,
    compaction_response_id: Option<String>,
    latest_token_usage_record: Option<TokenUsageRecord>,
    resume_metadata: Option<CompactionResumeMetadata>,
}

impl MemoryCompacted {
    fn capture(value: &CompactedItem) -> Result<Self, String> {
        let CompactedItem {
            message,
            replacement_history,
            guardian_history,
            retained_context,
            mcp_resource_origins,
            window_number,
            first_window_id,
            previous_window_id,
            window_id,
            compaction_response_id,
            latest_token_usage_record,
            resume_metadata,
        } = value;
        let capture = |items: &[codex_history::ResponseItemEnvelope]| {
            items
                .iter()
                .map(MemoryItem::capture)
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(Self {
            message: message.clone(),
            replacement_history: replacement_history
                .as_ref()
                .map(|items| capture(items))
                .transpose()?,
            guardian_history: guardian_history
                .as_ref()
                .map(|items| capture(&items.0))
                .transpose()?,
            retained_context: retained_context.clone(),
            mcp_resource_origins: mcp_resource_origins.clone(),
            window_number: *window_number,
            first_window_id: first_window_id.clone(),
            previous_window_id: previous_window_id.clone(),
            window_id: window_id.clone(),
            compaction_response_id: compaction_response_id.clone(),
            latest_token_usage_record: latest_token_usage_record.clone(),
            resume_metadata: resume_metadata.clone(),
        })
    }

    fn restore(self) -> Result<CompactedItem, String> {
        let restore = |items: Vec<MemoryItem>| {
            items
                .into_iter()
                .map(MemoryItem::restore)
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(CompactedItem {
            message: self.message,
            replacement_history: self.replacement_history.map(restore).transpose()?,
            guardian_history: self
                .guardian_history
                .map(restore)
                .transpose()?
                .map(GuardianHistoryCheckpoint),
            retained_context: self.retained_context,
            mcp_resource_origins: self.mcp_resource_origins,
            window_number: self.window_number,
            first_window_id: self.first_window_id,
            previous_window_id: self.previous_window_id,
            window_id: self.window_id,
            compaction_response_id: self.compaction_response_id,
            latest_token_usage_record: self.latest_token_usage_record,
            resume_metadata: self.resume_metadata,
        })
    }
}

impl MemoryRecord {
    pub(super) fn capture(item: &RolloutItem) -> Result<Self, String> {
        Ok(match item {
            RolloutItem::SessionMeta(value) => Self::SessionMeta(value.clone()),
            RolloutItem::DynamicToolsUpdated { thread_id, tools } => Self::DynamicToolsUpdated {
                thread_id: *thread_id,
                tools: tools.clone(),
            },
            RolloutItem::ResponseItem(value) => Self::Response(MemoryItem::capture(value)?),
            RolloutItem::InterAgentCommunication(value) => {
                Self::InterAgentCommunication(value.clone())
            }
            RolloutItem::InterAgentCommunicationMetadata { trigger_turn } => {
                Self::InterAgentCommunicationMetadata {
                    trigger_turn: *trigger_turn,
                }
            }
            RolloutItem::Compacted(value) => {
                Self::Compacted(Box::new(MemoryCompacted::capture(value)?))
            }
            RolloutItem::TurnContext(value) => Self::TurnContext(value.clone()),
            RolloutItem::TokenUsageRecord(value) => Self::TokenUsageRecord(value.clone()),
            RolloutItem::WorldState(value) => Self::WorldState(value.clone()),
            RolloutItem::SecurityRiskScore(value) => Self::SecurityRiskScore(value.clone()),
            RolloutItem::RetainedContext(value) => Self::RetainedContext(value.clone()),
            RolloutItem::EventMsg(value) => Self::Event(MemoryEvent::capture(value)?),
            RolloutItem::RealtimeItem(value) => Self::RealtimeItem(value.clone()),
        })
    }

    pub(super) fn restore(self) -> Result<RolloutItem, String> {
        Ok(match self {
            Self::SessionMeta(value) => RolloutItem::SessionMeta(value),
            Self::DynamicToolsUpdated { thread_id, tools } => {
                RolloutItem::DynamicToolsUpdated { thread_id, tools }
            }
            Self::Response(value) => RolloutItem::ResponseItem(value.restore()?),
            Self::InterAgentCommunication(value) => RolloutItem::InterAgentCommunication(value),
            Self::InterAgentCommunicationMetadata { trigger_turn } => {
                RolloutItem::InterAgentCommunicationMetadata { trigger_turn }
            }
            Self::Compacted(value) => RolloutItem::Compacted(Box::new(value.restore()?)),
            Self::TurnContext(value) => RolloutItem::TurnContext(value),
            Self::TokenUsageRecord(value) => RolloutItem::TokenUsageRecord(value),
            Self::WorldState(value) => RolloutItem::WorldState(value),
            Self::SecurityRiskScore(value) => RolloutItem::SecurityRiskScore(value),
            Self::RetainedContext(value) => RolloutItem::RetainedContext(value),
            Self::Event(value) => RolloutItem::EventMsg(value.restore()?),
            Self::RealtimeItem(value) => RolloutItem::RealtimeItem(value),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_history::CodexHarnessMetadata;
    use codex_history::ResponseItemEnvelope;
    use codex_history::RetainedUserMessage;
    use codex_protocol::models::FunctionCallOutputPayload;
    use codex_protocol::models::ResponseItem;

    fn round_trip(item: &RolloutItem) -> RolloutItem {
        let encoded = serde_json::to_vec(&MemoryRecord::capture(item).unwrap()).unwrap();
        serde_json::from_slice::<MemoryRecord>(&encoded)
            .unwrap()
            .restore()
            .unwrap()
    }

    #[test]
    fn compaction_keeps_nested_tool_outcomes_and_review_provenance() {
        let mut output = FunctionCallOutputPayload::from_text("completed tool".into());
        output.success = Some(false);
        let item = ResponseItemEnvelope {
            item: ResponseItem::CustomToolCallOutput {
                id: None,
                call_id: "original-call".into(),
                name: None,
                output,
                internal_chat_message_metadata_passthrough: None,
            },
            metadata: Some(CodexHarnessMetadata {
                compaction_model_hash: Some("producer".into()),
                user_input_order: Some(3),
                ..Default::default()
            }),
        };
        let original = CompactedItem {
            message: "checkpoint".into(),
            replacement_history: Some(vec![item.clone()]),
            guardian_history: Some(GuardianHistoryCheckpoint(vec![item])),
            retained_context: Some(RetainedContext::default()),
            mcp_resource_origins: None,
            window_number: Some(2),
            first_window_id: Some("first".into()),
            previous_window_id: Some("previous".into()),
            window_id: Some("current".into()),
            compaction_response_id: Some("response".into()),
            latest_token_usage_record: None,
            resume_metadata: None,
        };
        let RolloutItem::Compacted(restored) =
            round_trip(&RolloutItem::Compacted(Box::new(original.clone())))
        else {
            panic!("compaction kind changed")
        };
        assert_eq!(*restored, original);
    }

    #[test]
    fn retained_delivery_remains_a_host_fact() {
        let original = RetainedContextEvent::DeliveredAssistantMessage {
            message: RetainedUserMessage {
                turn_id: "original-turn".into(),
                message_id: Some("message".into()),
                text: "retained fixture".into(),
                complete: true,
                origin: Default::default(),
                phase: None,
            },
            acceptance_order: 9,
        };
        let RolloutItem::RetainedContext(restored) =
            round_trip(&RolloutItem::RetainedContext(original.clone()))
        else {
            panic!("host fact became model input")
        };
        assert_eq!(restored, original);
    }
}
