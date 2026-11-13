// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lossless envelope for private memory handoff, distinct from provider wire
//! encoding. Never write this representation into a model request or rollout.

use codex_history::CodexHarnessMetadata;
use codex_history::ResponseItemEnvelope;
use codex_protocol::ResponseItemId;
use codex_protocol::ResponseItemIdAllocation;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryItem {
    item: ResponseItem,
    id_allocation: Option<ResponseItemIdAllocation>,
    metadata: Option<CodexHarnessMetadata>,
    hidden: Hidden,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Hidden {
    Plain,
    Reasoning(Option<Vec<ReasoningItemContent>>),
    ToolOutput(Option<bool>),
}

impl MemoryItem {
    pub(crate) fn capture(envelope: &ResponseItemEnvelope) -> Result<Self, String> {
        let hidden = match &envelope.item {
            ResponseItem::Reasoning { content, .. } => Hidden::Reasoning(content.clone()),
            ResponseItem::FunctionCallOutput { output, .. }
            | ResponseItem::CustomToolCallOutput { output, .. } => {
                Hidden::ToolOutput(output.success)
            }
            _ => Hidden::Plain,
        };
        let captured = Self {
            item: envelope.item.clone(),
            id_allocation: envelope
                .item
                .id()
                .and_then(ResponseItemId::allocation)
                .cloned(),
            metadata: envelope.metadata.clone(),
            hidden,
        };
        // Refuse future provider-encoding omissions instead of silently losing
        // new host state when upstream adds another internal field.
        let encoded = serde_json::to_vec(&captured)
            .map_err(|_| "memory history item could not be encoded")?;
        let decoded: Self = serde_json::from_slice(&encoded)
            .map_err(|_| "memory history item could not be decoded")?;
        let restored = decoded.restore()?;
        // Upstream deliberately compares IDs by their wire text, not allocation.
        if restored != *envelope
            || restored.item.id().and_then(ResponseItemId::allocation)
                != envelope.item.id().and_then(ResponseItemId::allocation)
        {
            return Err("memory history encoding would discard item state".into());
        }
        Ok(captured)
    }

    pub(crate) fn restore(mut self) -> Result<ResponseItemEnvelope, String> {
        let id = restore_id(self.item.id().cloned(), self.id_allocation)?;
        self.item.set_id(id);
        match (&mut self.item, self.hidden) {
            (ResponseItem::Reasoning { content, .. }, Hidden::Reasoning(saved)) => *content = saved,
            (
                ResponseItem::FunctionCallOutput { output, .. }
                | ResponseItem::CustomToolCallOutput { output, .. },
                Hidden::ToolOutput(saved),
            ) => output.success = saved,
            (
                ResponseItem::Reasoning { .. }
                | ResponseItem::FunctionCallOutput { .. }
                | ResponseItem::CustomToolCallOutput { .. },
                Hidden::Plain,
            ) => return Err("memory history is missing internal item metadata".into()),
            (_, Hidden::Plain) => {}
            _ => return Err("memory history internal metadata has a different item kind".into()),
        }
        Ok(ResponseItemEnvelope {
            item: self.item,
            metadata: self.metadata,
        })
    }
}

/// Provider wire encoding omits allocation metadata even when IDs compare equal.
pub(super) fn restore_id(
    id: Option<ResponseItemId>,
    allocation: Option<ResponseItemIdAllocation>,
) -> Result<Option<ResponseItemId>, String> {
    match (id, allocation) {
        (Some(id), Some(allocation)) => Ok(Some(id.with_allocation(allocation))),
        (None, Some(_)) => Err("memory allocation has no corresponding item ID".into()),
        (id, None) => Ok(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::FunctionCallOutputPayload;

    fn round_trip(item: ResponseItem) -> ResponseItemEnvelope {
        let envelope = ResponseItemEnvelope {
            item,
            metadata: Some(CodexHarnessMetadata {
                compaction_model_hash: Some("original-producer".into()),
                user_input_order: Some(7),
                ..Default::default()
            }),
        };
        let encoded = serde_json::to_vec(&MemoryItem::capture(&envelope).unwrap()).unwrap();
        let decoded: MemoryItem = serde_json::from_slice(&encoded).unwrap();
        let restored = decoded.restore().unwrap();
        assert_eq!(restored, envelope);
        restored
    }

    #[test]
    fn generated_item_allocations_survive_private_handoff_and_stay_off_provider_wire() {
        let model = serde_json::from_value(serde_json::json!({
            "salt": "item", "ordinal": 1000, "value": 42
        }))
        .unwrap();
        let id = ResponseItemId::generated("msg", model);
        let original: ResponseItem = serde_json::from_value(serde_json::json!({
            "type": "message", "id": id.as_str(), "role": "assistant", "content": []
        }))
        .unwrap();
        let mut original = original;
        original.set_id(Some(id.clone()));
        let wire = serde_json::to_value(&original).unwrap();
        assert_eq!(wire["id"], id.as_str());
        assert!(wire.get("id_allocation").is_none());
        let wire_item: ResponseItem = serde_json::from_value(wire).unwrap();
        assert_eq!(
            wire_item, original,
            "ID equality alone hides lost allocations"
        );
        assert!(wire_item.id().unwrap().allocation().is_none());
        let restored = round_trip(original);
        let allocation = restored.item.id().unwrap().allocation().unwrap();
        assert_eq!(Some(allocation), id.allocation());
        assert_eq!(allocation.model_id.ordinal(), 1000);
        let wire = serde_json::to_value(&restored.item).unwrap();
        assert_eq!(wire["id"], id.as_str());
        assert!(wire.get("id_allocation").is_none());
    }

    #[test]
    fn orphaned_allocation_is_refused_instead_of_discarded() {
        let envelope = ResponseItemEnvelope {
            item: serde_json::from_value(serde_json::json!({
                "type": "message", "role": "assistant", "content": []
            }))
            .unwrap(),
            metadata: None,
        };
        let mut stored = serde_json::to_value(MemoryItem::capture(&envelope).unwrap()).unwrap();
        stored["id_allocation"] = serde_json::json!({
            "prefix": "msg", "salt": "item", "ordinal": 7, "value": 42
        });
        let decoded: MemoryItem = serde_json::from_value(stored).unwrap();
        assert!(decoded.restore().is_err());
    }

    #[test]
    fn private_reasoning_survives_provider_wire_omission() {
        for (content, omitted) in [
            (
                Some(vec![ReasoningItemContent::Text {
                    text: "private fixture".into(),
                }]),
                true,
            ),
            (
                Some(vec![ReasoningItemContent::ReasoningText {
                    text: "retained fixture".into(),
                }]),
                false,
            ),
            (Some(Vec::new()), true),
            (None, false),
        ] {
            let original = ResponseItem::Reasoning {
                id: None,
                summary: Vec::new(),
                content,
                encrypted_content: Some("opaque fixture".into()),
                internal_chat_message_metadata_passthrough: None,
            };
            let provider: ResponseItem =
                serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
            assert_eq!(provider != original, omitted);
            round_trip(original);
        }
    }

    #[test]
    fn tool_failure_survives_provider_wire_omission() {
        let mut output = FunctionCallOutputPayload::from_text("completed failure fixture".into());
        output.success = Some(false);
        let original = ResponseItem::FunctionCallOutput {
            id: None,
            call_id: Some("original-call".into()),
            name: Some("original-tool".into()),
            namespace: None,
            output: output.clone(),
            internal_chat_message_metadata_passthrough: None,
        };
        let provider: ResponseItem =
            serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
        assert_ne!(
            provider, original,
            "this test must exercise the lossy provider encoding"
        );
        round_trip(original);
        round_trip(ResponseItem::CustomToolCallOutput {
            id: None,
            call_id: "custom-call".into(),
            name: None,
            output,
            internal_chat_message_metadata_passthrough: None,
        });
    }

    #[test]
    fn mismatched_internal_metadata_is_not_reinterpreted() {
        let mut captured = MemoryItem::capture(&ResponseItemEnvelope {
            item: ResponseItem::Compaction {
                id: None,
                encrypted_content: "opaque compaction".into(),
                internal_chat_message_metadata_passthrough: None,
            },
            metadata: None,
        })
        .unwrap();
        captured.hidden = Hidden::ToolOutput(Some(true));
        assert!(captured.restore().is_err());
    }
}
