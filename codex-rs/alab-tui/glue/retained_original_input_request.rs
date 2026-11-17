// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! The original typed request before its native presentation conversion.
//! It is correlation DATA; only the actual reserved reply and Source cell
//! authorize publication. Auth/verification owners remain a distinct route.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Request {
    UserInput { params: ToolRequestUserInputParams },
    Approval { original: Box<ServerRequest> },
}
impl Request {
    pub(super) fn from_original(original: &ServerRequest) -> Option<Self> {
        match original {
            ServerRequest::ToolRequestUserInput { params, .. } => Some(Self::UserInput {
                params: params.clone(),
            }),
            ServerRequest::CommandExecutionRequestApproval { .. }
            | ServerRequest::FileChangeRequestApproval { .. }
            | ServerRequest::PermissionsRequestApproval { .. } => Some(Self::Approval {
                original: Box::new(original.clone()),
            }),
            // Elicitation/user-verification must retain their actual separate
            // auth revision/response owner; this never claims their raw reply.
            _ => None,
        }
    }
    pub(super) fn thread(&self) -> &str {
        match self {
            Self::UserInput { params } => &params.thread_id,
            Self::Approval { original } => match original.as_ref() {
                ServerRequest::CommandExecutionRequestApproval { params, .. } => &params.thread_id,
                ServerRequest::FileChangeRequestApproval { params, .. } => &params.thread_id,
                ServerRequest::PermissionsRequestApproval { params, .. } => &params.thread_id,
                _ => "",
            },
        }
    }
    pub(super) fn user_input(&self) -> Option<&ToolRequestUserInputParams> {
        match self {
            Self::UserInput { params } => Some(params),
            Self::Approval { .. } => None,
        }
    }
    pub(super) fn valid_for(&self, id: &RequestId) -> bool {
        if codex_protocol::ThreadId::from_string(self.thread()).is_err() {
            return false;
        }
        match self {
            Self::UserInput { .. } => true,
            Self::Approval { original } => {
                Self::from_original(original).is_some()
                    && !matches!(
                        original.as_ref(),
                        ServerRequest::ToolRequestUserInput { .. }
                    )
                    && original.id() == id
            }
        }
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        matches!((serde_json::to_value(self), serde_json::to_value(other)), (Ok(left), Ok(right)) if left == right)
    }
    pub(super) fn allows_native(
        &self,
        native: &crate::bottom_pane::alab_retained_approval::RequestData,
    ) -> bool {
        use crate::bottom_pane::alab_retained_approval::RequestData;
        native.thread_id_data().to_string() == self.thread()
            && matches!((self, native),
            (Self::Approval { original }, RequestData::Exec { .. }) if matches!(original.as_ref(), ServerRequest::CommandExecutionRequestApproval { .. }))
            || native.thread_id_data().to_string() == self.thread()
                && matches!((self, native),
            (Self::Approval { original }, RequestData::Permissions { .. }) if matches!(original.as_ref(), ServerRequest::PermissionsRequestApproval { .. }))
            || native.thread_id_data().to_string() == self.thread()
                && matches!((self, native),
            (Self::Approval { original }, RequestData::ApplyPatch { .. }) if matches!(original.as_ref(), ServerRequest::FileChangeRequestApproval { .. }))
    }
}
