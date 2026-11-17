// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Weak-only original command identity. Holds no client/catalog/auth resource,
//! keeps no worker alive, and proves no Native/currentness/reconnect fact.
use super::*;
#[derive(Clone)]
pub struct OriginalRequestIdentity {
    same: std::sync::Arc<dyn Fn(&AppServerRequestHandle) -> bool + Send + Sync>,
}
impl OriginalRequestIdentity {
    pub(crate) fn from_weak(
        check: impl Fn(&AppServerRequestHandle) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            same: std::sync::Arc::new(check),
        }
    }
    pub fn matches(&self, actual: &AppServerRequestHandle) -> bool {
        (self.same)(actual)
    }
}
impl AppServerRequestHandle {
    pub fn retained_original_resource(&self) -> OriginalRequestIdentity {
        match self {
            Self::InProcess(actual) => {
                let weak = actual.command_tx.downgrade();
                OriginalRequestIdentity::from_weak(
                    move |current| matches!(current,AppServerRequestHandle::InProcess(current) if weak.upgrade().is_some_and(|original|original.same_channel(&current.command_tx))),
                )
            }
            Self::Remote(actual) => actual.alab_original_backend_identity(),
            Self::Cloud(actual) => actual.alab_original_backend_identity(),
        }
    }
}
