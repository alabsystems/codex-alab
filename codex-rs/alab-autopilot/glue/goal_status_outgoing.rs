// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Reserve normal native output before acknowledging selected passive metadata.
//! Cancellation before reservation retains custody. After reservation, finish and
//! permit.send are synchronous in one poll. Client generation fences handle
//! ordering across concurrent publishers on different runtime threads.

impl super::OutgoingMessageSender {
    pub(crate) async fn alab_queue_goal_runtime(
        &self,
        snapshot: codex_protocol::protocol::AlabGoalRuntimeSnapshot,
    ) -> Result<(), ()> {
        match self.sender.reserve().await {
            Ok(permit) => {
                let message = super::timestamped_server_notification(
                    codex_app_server_protocol::ServerNotification::AlabGoalRuntimeUpdated(
                        snapshot.update().clone(),
                    ),
                );
                snapshot.finish(codex_protocol::protocol::AlabGoalRuntimeDelivery::Sent);
                permit.send(super::OutgoingEnvelope::Broadcast { message });
                Ok(())
            }
            Err(_) => {
                snapshot.finish(codex_protocol::protocol::AlabGoalRuntimeDelivery::Closed);
                Err(())
            }
        }
    }
}
