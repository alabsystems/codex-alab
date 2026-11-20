// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::AppEvent;
use super::AppServerSession;
use super::Openings;
use super::Target;
use crate::app_server_session::alab_session_work::Ready;
use std::io;
use std::time::Duration;
use uuid::Uuid;

pub(in crate::app) async fn ready(server: &AppServerSession) -> io::Result<Ready> {
    let cut = server.alab_capture_original_session_jobs()?;
    let ready = tokio::time::timeout(Duration::from_secs(60), cut.observe()).await??;
    ready.current(server)?;
    Ok(ready)
}

pub(in crate::app) struct PreparedWork {
    before: Ready,
    target: Target,
    request: Uuid,
}

impl PreparedWork {
    pub(in crate::app) async fn capture(
        original: &AppServerSession,
        openings: &Openings,
        event: &AppEvent,
    ) -> io::Result<Self> {
        let AppEvent::SavedChatPrepared {
            target, request, ..
        } = event
        else {
            panic!("expected the original preparation before checking its follow-up");
        };
        assert_eq!(target.backend_id, original.backend_id());
        assert!(openings.completions.is_empty());
        Ok(Self {
            before: ready(original).await?,
            target: *target,
            request: *request,
        })
    }

    pub(in crate::app) async fn join(
        &self,
        original: &AppServerSession,
        openings: &Openings,
    ) -> io::Result<()> {
        assert_eq!(
            self.before
                .current(original)
                .expect_err("the follow-up must enter the original session's work book")
                .kind(),
            io::ErrorKind::WouldBlock,
        );
        ready(original).await?;
        // The actor has not consumed any event between this follow-up starting
        // and its worker joining. Its one result must already be in this queue.
        assert_eq!(openings.completions.len(), 1);
        Ok(())
    }

    pub(in crate::app) fn check_event(&self, event: &AppEvent) {
        match event {
            AppEvent::SavedChatPrepared {
                target, request, ..
            }
            | AppEvent::SavedChatReleased {
                target, request, ..
            } => {
                assert_eq!((*target, *request), (self.target, self.request));
            }
            _ => {}
        }
    }
}
