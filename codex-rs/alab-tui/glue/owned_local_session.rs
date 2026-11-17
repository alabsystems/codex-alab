// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Session semantics follow the selected target, independently of wire client.
use super::*;

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
    ) -> impl std::future::Future<Output = Result<()>> + use<> {
        self.thread_unsubscribe(thread)
    }
    pub(crate) async fn alab_observe_side_close(&self, _thread: ThreadId) -> Result<()> {
        Ok(())
    }
}
