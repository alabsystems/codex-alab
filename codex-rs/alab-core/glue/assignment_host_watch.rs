// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Control-plane notifications follow actual host publication, not counter equality.
use super::*;

struct HostWatch(tokio::task::AbortHandle);
impl Drop for HostWatch {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) fn register(session: &Arc<Session>) {
    let host = Arc::clone(&session.services.auth_manager);
    let Ok(mut current) = capture_factory_manager(&host) else {
        return;
    };
    if Arc::ptr_eq(&host, &current) {
        return;
    }
    let weak = Arc::downgrade(session);
    let shutdown = session.mcp_prewarm_shutdown.clone();
    let worker = session.services.runtime_handle.spawn(async move {
        let mut refresh = current.auth_change_receiver();
        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => break,
                changed = host.alab_current_manager_changed(&current) => {
                    if changed.is_err() { break; }
                    let Ok(next) = capture_factory_manager(&host) else { break; };
                    if Arc::ptr_eq(&next, &current) { continue; }
                    // B's real counters may equal A's. Publication selects the
                    // real manager first, then subscribes to its actual channel.
                    current = next;
                    refresh = current.auth_change_receiver();
                },
                changed = refresh.changed() => {
                    if changed.is_err() { break; }
                },
            }
            let Some(session) = weak.upgrade() else {
                break;
            };
            session.request_mcp_runtime_refresh(
                crate::thread_manager::McpRefreshReason::RuntimeAuthChanged,
            );
        }
    });
    session
        .services
        .session_extension_data
        .insert(Arc::new(HostWatch(worker.abort_handle())));
}
