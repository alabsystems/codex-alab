// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::thread_attachment::Incoming;
use crate::app_server_session::ThreadParamsMode;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudCommand;
use pretty_assertions::assert_eq;

pub(super) struct Parked {
    server: AppServerSession,
    child: ThreadId,
    incoming: Incoming,
    commands: mpsc::Receiver<CloudCommand>,
    notifications: mpsc::UnboundedSender<AppServerEvent>,
}

impl Parked {
    pub(super) async fn prepare(
        app: &mut App,
        parent: ThreadId,
        displayed: &AppServerSession,
    ) -> Self {
        let (client, commands, notifications) = CloudClient::channel(1);
        let mut server = AppServerSession::new(
            crate::backend_id::BackendId::new(),
            AppServerClient::Cloud(client),
            ThreadParamsMode::Cloud,
        );
        let backend_id = server.backend_id();
        assert_ne!(backend_id, displayed.backend_id());
        let child = ThreadId::new();
        let target = sidebar_loading::Target {
            backend_id,
            parent,
            child,
        };
        let incoming = Incoming::begin(app, &mut server, target, uuid::Uuid::new_v4()).await;
        let channel = &app.thread_event_channels[&child];
        let update = ThreadBufferedEvent::Notification(Box::new(agent_message_delta_notification(
            child,
            "parked-turn",
            "parked-item",
            "parked output",
        )));
        channel
            .store
            .lock()
            .await
            .push_buffered_event(update.clone());
        channel
            .sender
            .try_send(update)
            .expect("parked receiver is open");
        Self {
            server,
            child,
            incoming,
            commands,
            notifications,
        }
    }

    pub(super) async fn check(mut self, app: &mut App) {
        let channel = app
            .thread_event_channels
            .get_mut(&self.child)
            .expect("parked channel");
        assert!(
            self.incoming
                .owns_channel(Some(channel), self.server.backend_id())
        );
        assert_eq!(channel.attachment(), ThreadEventAttachment::Live);
        let store = channel.store.lock().await;
        assert_eq!(store.attachment_prefix, Some(0));
        assert_eq!(store.buffer.len(), 1);
        assert_parked_update(store.buffer.front().unwrap());
        drop(store);
        let receiver = channel.receiver.as_mut().expect("parked original receiver");
        assert_parked_update(&receiver.try_recv().expect("queued parked update"));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert!(!self.notifications.is_closed());
        assert!(matches!(
            self.commands.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }
}

fn assert_parked_update(event: &ThreadBufferedEvent) {
    assert!(
        matches!(event, ThreadBufferedEvent::Notification(notification)
        if matches!(&**notification, ServerNotification::AgentMessageDelta(delta)
            if delta.delta == "parked output"))
    );
}
