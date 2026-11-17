// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Borrow the actual callback/reply records created by original listener replay.
//! Neither request IDs nor saved view JSON can construct these borrowed owners.
use super::*;
use codex_app_server_protocol::ToolRequestUserInputParams;
use std::io;

pub(crate) struct UserInputOwner<'a> {
    request_id: std::borrow::Cow<'a, AppServerRequestId>,
    thread: String,
    turn: &'a str,
    item: &'a str,
    _reply: Option<&'a AppServerReplyHandle>,
    original_input: Option<std::sync::Arc<super::alab_retained_replies::original_input::Item>>,
}
impl<'a> UserInputOwner<'a> {
    pub(crate) fn from_actual_item(
        item: &'a std::sync::Arc<crate::app::retained_original_input::Item>,
        client: &AppServerRequestHandle,
    ) -> io::Result<Self> {
        let params = item.user_params().ok_or_else(unavailable)?;
        if !item.is_for_client(client) {
            return Err(unavailable());
        }
        Ok(Self {
            request_id: std::borrow::Cow::Owned(item.id()),
            thread: params.thread_id.clone(),
            turn: &params.turn_id,
            item: &params.item_id,
            _reply: None,
            original_input: Some(item.clone()),
        })
    }
    pub(crate) fn original_input(
        &self,
    ) -> Option<std::sync::Arc<super::alab_retained_replies::original_input::Item>> {
        self.original_input.clone()
    }
    pub(crate) fn request_id_data(&self) -> &AppServerRequestId {
        self.request_id.as_ref()
    }
    pub(crate) fn matches(&self, request: &ToolRequestUserInputParams) -> bool {
        self.thread == PendingAppServerRequests::canonical_thread_id(&request.thread_id)
            && self.turn == request.turn_id
            && self.item == request.item_id
    }
}
impl PendingAppServerRequests {
    /// Match every queued request in its original per-turn order while borrowing
    /// SAME real registry. Rust's borrow prevents callback removal/replacement
    /// until synchronous view installation releases these actual reply owners.
    pub(in crate::app) fn alab_replayed_user_input_owners<'a>(
        &'a self,
        requests: &[&ToolRequestUserInputParams],
        actual_client: &AppServerRequestHandle,
    ) -> io::Result<Vec<UserInputOwner<'a>>> {
        let mut offsets = HashMap::<(String, String), usize>::new();
        let mut owners = Vec::with_capacity(requests.len());
        for request in requests {
            let key = (
                Self::canonical_thread_id(&request.thread_id),
                request.turn_id.clone(),
            );
            let offset = offsets.entry(key.clone()).or_default();
            let queue = self.user_inputs.get(&key).ok_or_else(unavailable)?;
            let record = queue.get(*offset).ok_or_else(unavailable)?;
            if record.item_id != request.item_id {
                return Err(unavailable());
            }
            let reply = replayed_reply(&record.request)?;
            let original_input = self.alab_retained_replies.input_for_reply(reply);
            if !reply.retained_pending_for_client(actual_client)
                && original_input
                    .as_ref()
                    .is_none_or(|item| !item.is_for_client(actual_client))
            {
                return Err(unavailable());
            }
            // Borrow the actual map's key as well as its exact reply record.
            let ((_, turn), _) = self
                .user_inputs
                .get_key_value(&key)
                .ok_or_else(unavailable)?;
            owners.push(UserInputOwner {
                request_id: std::borrow::Cow::Borrowed(&record.request.request_id),
                thread: key.0.clone(),
                turn,
                item: &record.item_id,
                _reply: Some(reply),
                original_input,
            });
            *offset += 1;
        }
        Ok(owners)
    }
}
fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "original replayed user-input callback owner unavailable",
    )
}
fn replayed_reply(record: &PendingServerRequest) -> io::Result<&AppServerReplyHandle> {
    record.reply.as_ref().ok_or_else(unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logical_request_data_without_original_reply_owner_cannot_bind() {
        let params = ToolRequestUserInputParams {
            thread_id: "00000000-0000-0000-0000-000000000001".into(),
            item_id: "actual-item".into(),
            turn_id: "actual-turn".into(),
            questions: Vec::new(),
            is_blocking: true,
            auto_resolution_ms: None,
        };
        let mut pending = PendingAppServerRequests::default();
        let request = ServerRequest::ToolRequestUserInput {
            request_id: AppServerRequestId::Integer(7),
            params,
        };
        assert!(pending.note_server_request(&request).is_none());
        // Existing lower test helper records request DATA with reply=None.
        // It is deliberately insufficient for the real replay binding.
        let record = pending
            .user_inputs
            .values()
            .next()
            .unwrap()
            .front()
            .unwrap();
        assert!(replayed_reply(&record.request).is_err());
        assert_eq!(
            pending
                .user_inputs
                .values()
                .map(VecDeque::len)
                .sum::<usize>(),
            1
        );
    }
}

#[path = "retained_approval_owners.rs"]
mod approvals;
pub(crate) use approvals::ApprovalOwner;
