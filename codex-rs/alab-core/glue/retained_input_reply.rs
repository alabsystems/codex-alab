// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Claims the actual original reply capability, never a request-ID constructor.
use super::*;
pub(crate) struct ReplyOwner {
    actual: AppServerReplyHandle,
}
impl ReplyOwner {
    pub(crate) fn request_id(&self) -> &RequestId {
        &self.actual.request_id
    }
    pub(crate) fn is_for_client(&self, client: &AppServerRequestHandle) -> bool {
        matches!((&self.actual.client, client), (AppServerRequestHandle::Remote(left), AppServerRequestHandle::Remote(right)) if left.alab_same_channel(right))
    }
    pub(crate) fn is_for_reply(&self, reply: &AppServerReplyHandle) -> bool {
        Arc::ptr_eq(&self.actual.claimed, &reply.claimed)
            && self.is_for_client(&reply.client)
            && self.actual.request_id == reply.request_id
    }
}
impl AppServerReplyHandle {
    /// Attach only the restored SAME operation to its actual replayed reply.
    /// This claims the real local reply but grants no Source cell authority.
    pub fn adopt_original_input(
        &self,
        restored: &Arc<crate::retained_callbacks::OriginalInputReservation>,
    ) -> io::Result<Arc<crate::retained_callbacks::OriginalInputReservation>> {
        if !matches!(&self.client, AppServerRequestHandle::Remote(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "original retained input requires actual Remote reply",
            ));
        }
        let attached = restored.attach_reply(ReplyOwner {
            actual: self.clone(),
        })?;
        self.claim()?;
        Ok(attached)
    }
    /// Call at actual queue-item ingress. This consumes SAME shared single-use
    /// reply, so a later raw resolve/reject cannot publish a second answer.
    pub fn reserve_original_input(
        &self,
        service: &str,
    ) -> io::Result<Arc<crate::retained_callbacks::OriginalInputReservation>> {
        if !matches!(&self.client, AppServerRequestHandle::Remote(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "original retained input requires actual Remote reply",
            ));
        }
        let reservation = crate::retained_callbacks::OriginalInputReservation::from_reply(
            ReplyOwner {
                actual: self.clone(),
            },
            service,
        )?;
        self.claim()?;
        Ok(reservation)
    }
}
