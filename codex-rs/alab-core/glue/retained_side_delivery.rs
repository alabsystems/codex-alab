// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ordered local observer correlation, deliberately outside history/wire.
//! A public Arc cannot satisfy a private expected SAME Core occurrence.
use super::*;
pub(crate) async fn deliver(session: &Session, turn: &str, identity: std::sync::Arc<()>) {
    session
        .deliver_event_raw(Event {
            id: turn.into(),
            msg: EventMsg::PrivateSessionEvent(PrivateSessionEvent {
                thread_id: session.thread_id,
                event: PrivateSessionEventPayload::AlabSideBoundary(identity),
            }),
        })
        .await;
}
