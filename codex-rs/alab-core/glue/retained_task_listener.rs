// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Use the existing listener worker and serialized installation for an actual
//! backend start. No client subscription or alternate event reader is created.
use super::*;

pub(in crate::request_processors) async fn ensure_backend_start_listener(
    context: ListenerTaskContext,
    thread_id: ThreadId,
    thread: Arc<CodexThread>,
) -> Result<(), JSONRPCErrorError> {
    let candidate = context
        .thread_state_manager
        .created_listener_state(thread_id)
        .await;
    match start_listener(
        context,
        thread_id,
        thread,
        candidate,
        ListenerStartup::Backend,
    )
    .await
    {
        ListenerStartOutcome::Started { .. } => Ok(()),
        ListenerStartOutcome::Stale | ListenerStartOutcome::Rejected => {
            Err(thread_closing_for_resume(thread_id))
        }
        #[cfg(unix)]
        ListenerStartOutcome::PrimaryAcquired => Err(thread_closing_for_resume(thread_id)),
    }
}

/// Subscribe the actual initialized replaying connection to the exact retained
/// Arc under the SAME goal/pending/Current installation gates. No ID lookup can
/// substitute a replacement runtime, and no second event reader is created.
pub(in crate::request_processors) async fn subscribe_initial_runtime(
    context: ListenerTaskContext,
    thread_id: ThreadId,
    thread: Arc<CodexThread>,
    connection_id: ConnectionId,
    raw_events_enabled: bool,
) -> Result<EnsureConversationListenerResult, JSONRPCErrorError> {
    let candidate = context
        .thread_state_manager
        .created_listener_state(thread_id)
        .await;
    start_listener(
        context,
        thread_id,
        thread,
        candidate,
        ListenerStartup::BackendSubscribe {
            connection_id,
            raw_events_enabled,
        },
    )
    .await
    .into_subscription(thread_id)
}
