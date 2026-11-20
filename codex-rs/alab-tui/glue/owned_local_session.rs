// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Session semantics follow the selected target, independently of wire client.
use super::*;

/// A callback can acknowledge only the original result delivered to that callback.
pub(crate) struct SideCloseAck {
    #[cfg(unix)]
    target: crate::owned_local_frontend::OwnedLocalTarget,
    #[cfg(unix)]
    thread: ThreadId,
    #[cfg(unix)]
    operation: codex_app_server_client::SideViewCloseOperation,
    #[cfg(unix)]
    handle: codex_app_server_client::AppServerRequestHandle,
}

impl std::fmt::Debug for SideCloseAck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SideCloseAck")
    }
}

impl SideCloseAck {
    pub(crate) async fn after_application(ack: Option<Self>) {
        #[cfg(unix)]
        if let Some(ack) = ack
            && let Err(error) = ack
                .target
                .observe_side_close(ack.thread, &ack.operation, &ack.handle)
                .await
        {
            tracing::warn!(%error, "Side close result observation retained");
        }
        #[cfg(not(unix))]
        let _ = ack;
    }
}

impl AppServerSession {
    #[cfg(unix)]
    pub(crate) fn with_selected_target(
        mut self,
        target: &crate::AppServerTarget,
    ) -> std::io::Result<Self> {
        if let crate::AppServerTarget::OwnedLocal { target } = target {
            if self.owned_local.is_some() {
                return Err(std::io::Error::other(
                    "session already owns a frontend delivery",
                ));
            }
            self.owned_local = Some(target.session_tenure()?);
        }
        Ok(self)
    }

    #[cfg(not(unix))]
    pub(crate) fn with_selected_target(
        self,
        _target: &crate::AppServerTarget,
    ) -> std::io::Result<Self> {
        Ok(self)
    }

    #[cfg(unix)]
    pub(crate) fn retained_frontend(&self) -> Option<&crate::owned_local_frontend::FrontendTenure> {
        self.owned_local.as_ref()
    }
}

#[cfg(unix)]
impl AppServerSession {
    /// Owning sessions observe physical-client shutdown and the SAME Native
    /// delivery close. Request-only views retain custody without closing it.
    pub(crate) async fn alab_shutdown_owned(self) -> std::io::Result<()> {
        alab_session_models::shutdown_actual(self).await // ALAB(retained-ui): actual shutdown task retains client/Book/tenure across canceled observation.
    }
}

// ALAB(side-boundary): only explicit pane-close callers use the original unsubscribe scope.
#[cfg(unix)]
#[path = "owned_local_side_close_session.rs"]
mod side_close;

#[cfg(not(unix))]
impl AppServerSession {
    pub(crate) fn alab_unsubscribe_side_view(
        &mut self,
        thread: ThreadId,
    ) -> (
        bool,
        impl std::future::Future<Output = (Result<()>, Option<SideCloseAck>)> + use<>,
    ) {
        let original = self.thread_unsubscribe(thread);
        (true, async move { (original.await, None) })
    }
}
