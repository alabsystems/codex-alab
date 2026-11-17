// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Installed view/close DATA on the same target. This owns client operations,
//! never a reconstructed Native receipt or a backend Core lease.
use super::*;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::SideView;
use codex_app_server_client::SideViewCloseCheckpoint;
use codex_app_server_client::SideViewCloseOperation;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    views: BTreeMap<String, View>,
}
struct View {
    data: SideView,
    close: Option<SideViewCloseOperation>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SideViewCheckpoint {
    view: SideView,
    close: Option<SideViewCloseCheckpoint>,
}
impl OwnedLocalTarget {
    /// Called immediately after actual Source activation. The DATA identifies
    /// its existing Source owner; a later close still requires current A.
    pub(crate) fn retain_side_view_data(&self, data: SideView) -> io::Result<()> {
        if !data.valid() {
            return Err(unavailable());
        }
        let mut state = self
            .0
            .side_views
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(old) = state.views.get(&data.thread_id) {
            if old.data == data {
                return Ok(());
            }
            // A pending original close must not be replaced by a fresh pane.
            if old.close.is_some() {
                return Err(unavailable());
            }
        } else if state.views.len() >= 4096 {
            return Err(unavailable());
        }
        state
            .views
            .insert(data.thread_id.clone(), View { data, close: None });
        Ok(())
    }
    /// Synchronous capture before the ORIGINAL unsubscribe future is polled.
    pub(crate) fn side_close_operation(
        &self,
        thread: ThreadId,
    ) -> io::Result<Option<SideViewCloseOperation>> {
        let mut state = self
            .0
            .side_views
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(view) = state.views.get_mut(&thread.to_string()) else {
            return Ok(None);
        };
        if let Some(actual) = &view.close {
            return Ok(Some(actual.clone()));
        }
        let actual = SideViewCloseOperation::new(view.data.clone())?;
        view.close = Some(actual.clone());
        Ok(Some(actual))
    }
    pub(crate) fn side_view_checkpoints(&self) -> Vec<SideViewCheckpoint> {
        self.0
            .side_views
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .views
            .values()
            .map(|view| SideViewCheckpoint {
                view: view.data.clone(),
                close: view.close.as_ref().map(SideViewCloseOperation::checkpoint),
            })
            .collect()
    }
    /// E1 caller restores this required leaf BEFORE replaying the original UI
    /// close. Bytes select an operation; they do not recreate server custody.
    pub(crate) fn restore_side_views(
        &self,
        checkpoints: Vec<SideViewCheckpoint>,
    ) -> io::Result<()> {
        if checkpoints.len() > 4096 {
            return Err(unavailable());
        }
        let mut restored = BTreeMap::new();
        for checkpoint in checkpoints {
            if !checkpoint.view.valid() {
                return Err(unavailable());
            }
            let close = checkpoint
                .close
                .map(SideViewCloseOperation::restore)
                .transpose()?;
            if close
                .as_ref()
                .is_some_and(|actual| actual.view_data() != checkpoint.view)
                || restored.contains_key(&checkpoint.view.thread_id)
            {
                return Err(unavailable());
            }
            restored.insert(
                checkpoint.view.thread_id.clone(),
                View {
                    data: checkpoint.view,
                    close,
                },
            );
        }
        let mut state = self
            .0
            .side_views
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !state.views.is_empty() {
            return Err(unavailable());
        }
        state.views = restored;
        Ok(())
    }
    /// UI application happened before this call. A lost ACK leaves the SAME
    /// cached operation for C2; no ordinary close or new view is retried here.
    pub(crate) async fn observe_side_close(
        &self,
        thread: ThreadId,
        handle: &AppServerRequestHandle,
    ) -> io::Result<()> {
        let actual = {
            let state = self
                .0
                .side_views
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state
                .views
                .get(&thread.to_string())
                .and_then(|view| view.close.clone())
        };
        let Some(actual) = actual else {
            return Ok(());
        };
        actual.observe_applied(handle).await?;
        let mut state = self
            .0
            .side_views
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let key = thread.to_string();
        if let Some(view) = state.views.get_mut(&key)
            && view
                .close
                .as_ref()
                .is_some_and(|current| current.same_operation(&actual))
        {
            if actual.original_succeeded() {
                state.views.remove(&key);
            } else {
                view.close = None;
            }
        }
        Ok(())
    }
}
