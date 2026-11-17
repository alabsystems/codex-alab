// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Same actual command transport only. This is not Native/currentness evidence.
use super::RemoteAppServerRequestHandle;
impl RemoteAppServerRequestHandle {
    pub(crate) fn alab_same_channel(&self, other: &Self) -> bool {
        self.command_tx.same_channel(&other.command_tx)
    }
}

#[cfg(test)]
mod original_input_tests {
    use super::*;
    use crate::AppServerRequestHandle;
    use crate::retained_callbacks::OriginalInputReservation;
    use codex_app_server_protocol::RequestId;

    #[tokio::test]
    async fn actual_remote_reply_claim_is_shared_before_any_original_send()
    -> Result<(), Box<dyn std::error::Error>> {
        let (command_tx, mut commands) = tokio::sync::mpsc::channel(2);
        let client = AppServerRequestHandle::Remote(RemoteAppServerRequestHandle { command_tx });
        let reply = client.server_request_reply_handle(RequestId::Integer(17));
        let clone = reply.clone();
        let reservation = reply.reserve_original_input("00000000000000000000000000000001")?;
        assert!(reservation.is_for_reply(&clone));
        assert!(reservation.is_for_client(&client));
        assert!(!clone.retained_pending_for_client(&client));
        assert!(
            clone
                .reserve_original_input("00000000000000000000000000000001")
                .is_err()
        );
        assert!(
            clone
                .resolve(serde_json::json!({"answers":{}}))
                .await
                .is_err()
        );
        assert!(commands.try_recv().is_err());
        let restored = OriginalInputReservation::restore_checkpoint(reservation.checkpoint()?)?;
        assert!(!restored.is_for_reply(&reply));
        let replayed = client.server_request_reply_handle(RequestId::Integer(17));
        let adopted = replayed.adopt_original_input(&restored)?;
        assert!(adopted.is_for_reply(&replayed));
        assert_eq!(adopted.checkpoint()?, reservation.checkpoint()?);
        assert!(
            replayed
                .resolve(serde_json::json!({"different":true}))
                .await
                .is_err()
        );
        assert!(commands.try_recv().is_err());
        Ok(())
    }
    #[test]
    fn replay_adoption_refuses_replacement_id_and_already_live_owner()
    -> Result<(), Box<dyn std::error::Error>> {
        let (command_tx, _commands) = tokio::sync::mpsc::channel(2);
        let client = AppServerRequestHandle::Remote(RemoteAppServerRequestHandle { command_tx });
        let reply = client.server_request_reply_handle(RequestId::Integer(17));
        let reservation = reply.reserve_original_input("00000000000000000000000000000001")?;
        let another = client.server_request_reply_handle(RequestId::Integer(17));
        assert!(another.adopt_original_input(&reservation).is_err());
        assert!(another.retained_pending_for_client(&client));
        let restored = OriginalInputReservation::restore_checkpoint(reservation.checkpoint()?)?;
        let wrong = client.server_request_reply_handle(RequestId::Integer(18));
        assert!(wrong.adopt_original_input(&restored).is_err());
        assert!(wrong.retained_pending_for_client(&client));
        Ok(())
    }
}

#[cfg(unix)]
impl RemoteAppServerRequestHandle {
    pub(crate) fn alab_original_backend_identity(&self) -> crate::OriginalRequestIdentity {
        let weak = self.command_tx.downgrade();
        crate::OriginalRequestIdentity::from_weak(
            move |actual| matches!(actual,crate::AppServerRequestHandle::Remote(actual) if weak.upgrade().is_some_and(|original|original.same_channel(&actual.command_tx))),
        )
    }
}
