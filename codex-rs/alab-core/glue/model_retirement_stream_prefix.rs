// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ResponsesStreamEvent;
use crate::ApiError;
use serde_json::Value;

pub(crate) struct Prefix {
    eligible: bool,
    created: Option<String>,
}

impl Prefix {
    pub(crate) fn new() -> Self {
        Self {
            eligible: codex_alab_core::active(),
            created: None,
        }
    }

    /// Only a bare creation envelope may precede the refusal. Unknown protocol events,
    /// malformed messages and embedded response output all make this a future-only update.
    pub(crate) fn observe(&mut self, event: Option<&ResponsesStreamEvent>) -> bool {
        if !self.eligible {
            return false;
        }
        self.eligible = false;
        let Some(event) = event else { return false };
        if event.item.is_some()
            || event.item_id.is_some()
            || event.call_id.is_some()
            || event.summary_index.is_some()
            || event.content_index.is_some()
            || event.delta.is_some()
            || event.text.is_some()
            || event.metadata.is_some()
            || event.safety_buffering.is_some()
        {
            return false;
        }
        if let Some(response) = &event.response {
            let Some(response) = response.as_object() else {
                return false;
            };
            if response
                .get("output")
                .is_some_and(|items| !matches!(items, Value::Array(items) if items.is_empty()))
                || response.get("usage").is_some_and(|usage| !usage.is_null())
            {
                return false;
            }
        }
        let nested_id = event
            .response
            .as_ref()
            .and_then(|response| response.get("id"));
        let id = match (nested_id, event.response_id.as_ref()) {
            (Some(nested), Some(top)) if nested != top => return false,
            (Some(value), _) | (None, Some(value)) => {
                let Some(id) = value.as_str().filter(|id| !id.is_empty()) else {
                    return false;
                };
                Some(id)
            }
            (None, None) => None,
        };
        match event.kind() {
            "response.created" if self.created.is_none() => {
                if let Some(id) = id {
                    self.created = Some(id.to_owned());
                    self.eligible = true;
                }
                false
            }
            "response.failed" | "error" => self
                .created
                .as_deref()
                .is_none_or(|created| id == Some(created)),
            _ => false,
        }
    }
}

pub(crate) fn attach(mut error: ApiError, before_output: bool) -> ApiError {
    if let ApiError::ModelNotFound {
        alab_empty_stream, ..
    } = &mut error
    {
        *alab_empty_stream = before_output;
    }
    error
}

impl ApiError {
    /// The native WebSocket error envelope may keep its original HTTP status and headers.
    /// That representation carries no pre-output attestation, but the real model rejection
    /// can still update the owning session's next model.
    pub fn alab_streamed_model_retirement(&self) -> Option<bool> {
        match self {
            Self::ModelNotFound {
                alab_empty_stream, ..
            } => Some(*alab_empty_stream),
            Self::Transport(codex_client::TransportError::Http {
                status,
                body: Some(body),
                ..
            }) => {
                let envelope = serde_json::from_str::<Value>(body).ok()?;
                matches!(
                    crate::error::parse_terminal_provider_error(
                        envelope.get("error"),
                        Some(*status)
                    ),
                    Some(Self::ModelNotFound { .. })
                )
                .then_some(false)
            }
            _ => None,
        }
    }
}
