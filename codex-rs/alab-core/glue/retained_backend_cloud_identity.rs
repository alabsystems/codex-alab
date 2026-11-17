// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Only the original weak command identity; catalog/auth/worker is not cloned.
use super::*;
impl CloudRequestHandle {
    pub(crate) fn alab_original_backend_identity(&self) -> crate::OriginalRequestIdentity {
        let weak = self.commands.downgrade();
        crate::OriginalRequestIdentity::from_weak(
            move |actual| matches!(actual,crate::AppServerRequestHandle::Cloud(actual) if weak.upgrade().is_some_and(|original|original.same_channel(&actual.commands))),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_identity_is_weak_and_cannot_keep_actual_cloud_command_resource_alive() {
        let (client, mut commands, events) = CloudClient::channel(1);
        let handle = client.request_handle();
        let original = handle.alab_original_backend_identity();
        assert!(original.matches(&crate::AppServerRequestHandle::Cloud(handle.clone())));
        drop(handle);
        drop(client);
        drop(events);
        assert!(matches!(
            commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
        drop(original);
    }
}
