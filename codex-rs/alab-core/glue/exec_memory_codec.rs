// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lossless envelope for private memory handoff, distinct from provider wire
//! encoding. Never write this representation into a model request or rollout.

use codex_history::CodexHarnessMetadata;
use codex_history::ResponseItemEnvelope;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryItem {
    item: ResponseItem,
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
            metadata: envelope.metadata.clone(),
            hidden,
        };
        // Refuse future provider-encoding omissions instead of silently losing
        // new host state when upstream adds another internal field.
        let encoded = serde_json::to_vec(&captured)
            .map_err(|_| "memory history item could not be encoded")?;
        let decoded: Self = serde_json::from_slice(&encoded)
            .map_err(|_| "memory history item could not be decoded")?;
        if decoded.restore()? != *envelope {
            return Err("memory history encoding would discard item state".into());
        }
        Ok(captured)
    }

    pub(crate) fn restore(mut self) -> Result<ResponseItemEnvelope, String> {
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
