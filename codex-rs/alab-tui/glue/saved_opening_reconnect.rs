// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::reconnect::ReconnectPresentation;
use crate::app::reconnect::reconnect;
use pretty_assertions::assert_eq;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub(super) const BUFFERED_TURN: &str = "turn-before-reconnect";
const REPLACEMENT_TURN: &str = "turn-after-replacement-attach";

pub(super) struct OriginalOpening {
    pub(super) store: Arc<Mutex<ThreadEventStore>>,
    pub(super) delayed_reply: Option<oneshot::Sender<()>>,
    pub(super) wrong_thread: bool,
}

pub(super) async fn finish_and_reopen(
    app: &mut App,
    tui: &mut crate::tui::Tui,
    server: &mut AppServerSession,
    events: &mut mpsc::UnboundedReceiver<AppEvent>,
    target: sidebar_loading::Target,
    listener: Arc<TcpListener>,
    original: OriginalOpening,
) -> Result<(JoinHandle<Result<()>>, Uuid)> {
    let parent = target.parent;
    let child = target.child;
    let parent_reply = response(app, parent, false)?;
    let mut child_reply = response(app, child, false)?;
    child_reply.model = "model-from-new-connection".into();
    let proxy = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut resumed = Vec::new();
        disconnect::serve_reconnect_requests(
            tokio_tungstenite::accept_async(stream).await?,
            |request| {
                let reply = match request.method.as_str() {
                    "thread/read" | "thread/resume" => {
                        let thread = &request.params.as_ref().unwrap()["threadId"];
                        let found = if thread == &json!(parent.to_string()) {
                            &parent_reply
                        } else {
                            assert_eq!(thread, &json!(child.to_string()));
                            &child_reply
                        };
                        if request.method == "thread/resume" {
                            resumed.push(found.thread.id.clone());
                            json!({"result": found})
                        } else {
                            json!({"result": {"thread": found.thread}})
                        }
                    }
                    "thread/list" | "thread/loaded/list" => {
                        json!({"result": {"data": [], "nextCursor": null}})
                    }
                    "thread/goal/get" => json!({"result": {"goal": null}}),
                    method => composer_read(method),
                };
                std::future::ready(Some(reply))
            },
        )
        .await?;
        // C2 resumes the parent and establishes exactly one subscription to the child.
        assert_eq!(resumed, [parent.to_string(), child.to_string()]);
        Ok(())
    });

    let original_sender = app.app_event_tx.clone();
    let mut original_view = server.request_view();
    let connected = reconnect(
        target.backend_id,
        app.app_server_target.clone(),
        app.config.clone(),
        app.local_settings.clone(),
        Some(parent),
        None,
        server.thread_tool_transport(),
        ReconnectPresentation::Conversation,
    )
    .await?;
    crate::boxed_future!(app.finish_reconnect(tui, server, events, connected, "2.0.0")).await?;
    assert!(original_sender.app_event_tx.is_closed());
    let replacement = &app.thread_event_channels[&child];
    let parked_store = replacement.store.clone();
    assert!(!Arc::ptr_eq(&original.store, &parked_store));
    assert_eq!(replacement.attachment(), ThreadEventAttachment::ReplayOnly);
    {
        let store = parked_store.lock().await;
        assert!(store.attachment_prefix.is_none());
        assert_eq!(store.active_turn_id(), Some(BUFFERED_TURN));
    }

    let unusable_reply = original.delayed_reply.is_some();
    let (replacement_store, buffered_turn) = if let Some(delayed_reply) = original.delayed_reply {
        // The replacement's own resume has finished before C1 publishes its unusable success.
        assert!(app.attach_live_thread_for_selection(server, child).await?);
        assert_eq!(
            app.thread_event_channels[&child].attachment(),
            ThreadEventAttachment::Live
        );
        app.handle_app_server_event(
            server,
            AppServerEvent::ServerNotification(Box::new(turn_started_notification(
                child,
                REPLACEMENT_TURN,
            ))),
        )
        .await;
        let store = app.thread_event_channels[&child].store.clone();
        assert_eq!(store.lock().await.active_turn_id(), Some(REPLACEMENT_TURN));
        delayed_reply.send(()).unwrap();
        (store, REPLACEMENT_TURN)
    } else {
        (parked_store, BUFFERED_TURN)
    };

    let prepared = next_saved_event(app, tui, server, events).await?;
    let AppEvent::SavedChatPrepared {
        target: actual,
        request,
        result,
    } = &prepared
    else {
        bail!("missing queued reply from the old connection");
    };
    assert_eq!(*actual, target);
    assert_eq!(result.result.is_err(), unusable_reply);
    if original.wrong_thread {
        assert!(result.metadata.subscribed);
        assert!(!result.unreadable_resume);
        assert!(result.missing_resume.is_none());
    }
    let original_operation = *request;
    crate::boxed_future!(app.handle_event(tui, server, prepared)).await?;
    let released = next_saved_event(app, tui, server, events).await?;
    let AppEvent::SavedChatReleased {
        target: actual,
        request,
        receipt,
    } = &released
    else {
        bail!("missing release after reconnect");
    };
    assert_eq!((*actual, *request), (target, original_operation));
    assert_eq!(
        receipt.wire_request,
        RequestId::String(format!("saved-chat-release-{original_operation}"))
    );
    assert_eq!(
        receipt.outcome,
        SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::Unsubscribed)
    );
    crate::boxed_future!(app.handle_event(tui, server, released)).await?;
    // The original peer can still serve this read after its unsubscribe reply.
    assert_eq!(
        original_view
            .thread_read(child, false)
            .await
            .wrap_err("original saved-thread read after unsubscribe completion")?
            .id,
        child.to_string()
    );
    drop(original_view);
    assert_eq!(app.current_displayed_thread_id(), Some(parent));
    assert!(Arc::ptr_eq(
        &app.thread_event_channels[&child].store,
        &replacement_store
    ));
    assert_eq!(
        replacement_store.lock().await.active_turn_id(),
        Some(buffered_turn)
    );

    app.sidebar_mut().show(
        sidebar::SidebarContent::Chooser,
        sidebar::PaneFocus::Sidebar,
    );
    app.sidebar_mut().update_saved_chats(vec![child.into()]);
    app.open_saved_chat(tui, server, target).await?;
    if !unusable_reply {
        let fresh = next_saved_event(app, tui, server, events).await?;
        assert!(
            matches!(&fresh, AppEvent::SavedChatPrepared { target: actual, request, .. }
            if *actual == target && *request != original_operation)
        );
        crate::boxed_future!(app.handle_event(tui, server, fresh)).await?;
    }
    assert_eq!(app.current_displayed_thread_id(), Some(child));
    assert_eq!(
        app.thread_event_channels[&child].attachment(),
        ThreadEventAttachment::Live
    );
    let store = replacement_store.lock().await;
    assert_eq!(
        store.session.as_ref().unwrap().model,
        "model-from-new-connection"
    );
    Ok((proxy, original_operation))
}

pub(super) async fn next_saved_event(
    app: &mut App,
    tui: &mut crate::tui::Tui,
    server: &mut AppServerSession,
    events: &mut mpsc::UnboundedReceiver<AppEvent>,
) -> Result<AppEvent> {
    loop {
        let event = app
            .saved_openings
            .next_event(events)
            .await
            .expect("open actor receiver");
        if matches!(
            &event,
            AppEvent::SavedChatPrepared { .. } | AppEvent::SavedChatReleased { .. }
        ) {
            return Ok(event);
        }
        crate::boxed_future!(app.handle_event(tui, server, event)).await?;
    }
}
