// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Test-only access to the maintained App/backend routing fixture.

use super::App;
use crate::app_command::AppCommand;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerSessionEvent;
use crate::app_server_session::ThreadParamsMode;
use crate::backend_id::BackendId;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudCommand;
use codex_app_server_protocol::ServerNotification;
use codex_protocol::ThreadId;
use tokio::sync::mpsc;

pub(crate) async fn fixture() -> (
    Box<App>,
    mpsc::UnboundedReceiver<AppEvent>,
    mpsc::UnboundedReceiver<AppCommand>,
) {
    super::tests::make_test_app_with_channels().await
}

pub(crate) fn backend() -> (
    AppServerSession,
    mpsc::Receiver<CloudCommand>,
    mpsc::UnboundedSender<AppServerEvent>,
) {
    let (client, commands, events) = CloudClient::channel(8);
    (
        AppServerSession::new(
            BackendId::new(),
            AppServerClient::Cloud(client),
            ThreadParamsMode::Cloud,
        ),
        commands,
        events,
    )
}

pub(crate) fn select(app: &mut App, owner: BackendId, thread: ThreadId) {
    assert!(app.backend_registry.observe(owner, thread));
    app.alab_backend_bookkeeping.displayed(owner);
    app.active_thread_id = Some(thread);
    app.primary_thread_id = Some(thread);
    app.chat_widget
        .handle_thread_session_quiet(super::tests::test_thread_session(
            thread,
            app.config.cwd.to_path_buf(),
        ));
    app.chat_widget.alab_bind_goal_runtime_backend(owner);
}

pub(crate) async fn feed(
    app: &mut App,
    session: &AppServerSession,
    displayed: BackendId,
    claimed: BackendId,
    notification: ServerNotification,
) {
    event(
        app,
        session,
        displayed,
        claimed,
        AppServerEvent::ServerNotification(Box::new(notification)),
    )
    .await;
}

pub(crate) async fn event(
    app: &mut App,
    session: &AppServerSession,
    displayed: BackendId,
    claimed: BackendId,
    event: AppServerEvent,
) {
    let mut parked = Default::default();
    app.handle_polled_backend_event(
        session,
        displayed,
        &mut parked,
        AppServerSessionEvent {
            backend_id: claimed,
            event,
        },
    )
    .await;
}

pub(crate) fn observe(app: &App) {
    super::alab_backend_bookkeeping::observe_runtime(app);
}
pub(crate) fn refresh_pending(app: &App) -> bool {
    app.alab_backend_bookkeeping.runtime_refresh_pending()
}
