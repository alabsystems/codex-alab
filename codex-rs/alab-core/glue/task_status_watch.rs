// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A read wait may watch only an already tracked runtime. Existing subscribe
//! inserts a persistent sender even for an arbitrary nonexistent UUID. Keep
//! the existence check and subscription under the same original state lock,
//! atomic with removal; this hint never creates a runtime or grants authority.
use codex_app_server_protocol::ThreadStatus;
use codex_protocol::ThreadId;
use tokio::sync::watch;

impl super::ThreadWatchManager {
    pub(crate) async fn alab_subscribe_tracked_task(
        &self,
        thread_id: ThreadId,
    ) -> Option<watch::Receiver<ThreadStatus>> {
        let thread_id = thread_id.to_string();
        let mut state = self.state.lock().await;
        if !state.runtime_by_thread_id.contains_key(&thread_id) {
            return None;
        }
        Some(state.subscribe(thread_id))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ThreadWatchManager;
    use super::*;

    #[tokio::test]
    async fn arbitrary_untracked_wait_targets_leave_no_persistent_sender() {
        let manager = ThreadWatchManager::new();
        for _ in 0..256 {
            assert!(
                manager
                    .alab_subscribe_tracked_task(ThreadId::new())
                    .await
                    .is_none()
            );
        }
        let state = manager.state.lock().await;
        assert!(state.runtime_by_thread_id.is_empty());
        assert!(state.status_watcher_by_thread_id.is_empty());
    }

    #[tokio::test]
    async fn tracked_hint_observes_between_snapshot_and_wait_then_recreates_after_remove() {
        let manager = ThreadWatchManager::new();
        let id = ThreadId::new();
        manager.upsert_thread_silently(&id.to_string()).await;
        let mut first = manager
            .alab_subscribe_tracked_task(id)
            .await
            .expect("tracked task");
        drop(first.borrow_and_update());
        manager.note_turn_started(&id.to_string()).await;
        first
            .changed()
            .await
            .expect("update after snapshot must stay observable");
        assert_eq!(
            *first.borrow_and_update(),
            ThreadStatus::Active {
                active_flags: vec![]
            }
        );
        manager.remove_thread(&id.to_string()).await;
        assert!(first.has_changed().is_err());
        assert!(manager.alab_subscribe_tracked_task(id).await.is_none());
        manager.upsert_thread_silently(&id.to_string()).await;
        let fresh = manager
            .alab_subscribe_tracked_task(id)
            .await
            .expect("recreated tracked task");
        assert_eq!(*fresh.borrow(), ThreadStatus::Idle);
    }
}
