// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Resource identity only. This creates no reply/Native authority and never
//! casts a retained WebSocket to InProcess. Compiled inside server_request.
use super::*;
impl AppServerReplyHandle {
    pub fn retained_pending_for_client(&self, actual: &AppServerRequestHandle) -> bool {
        !self.claimed.load(Ordering::Acquire)
            && matches!((&self.client, actual),
                (AppServerRequestHandle::Remote(left), AppServerRequestHandle::Remote(right))
                    if left.alab_same_channel(right))
    }
}
