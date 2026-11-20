// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual queued Cloud events and the same outer conditional-admission entry.
//! UpgradeRequest::new proves original decision ownership, never Native acceptance.
use super::*;
use crate::app::sidebar_loading::Openings;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use crate::backend_id::BackendId;
use codex_alab_core::session::UpgradeRequest;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudCommand;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::FileChangeRequestApprovalParams;
use codex_app_server_protocol::McpServerStartupState;
use codex_app_server_protocol::McpServerStatusUpdatedNotification;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_protocol::ThreadId;
use futures::FutureExt;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[path = "upgrade_parked_input_tests.rs"]
mod parked_input_tests;

const WAIT: Duration = Duration::from_secs(60);
fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn check(value: bool, message: &'static str) -> io::Result<()> {
    if value { Ok(()) } else { Err(failure(message)) }
}
struct Fixture {
    backends: BackendSessions,
    commands: Vec<mpsc::Receiver<CloudCommand>>,
    events: Vec<mpsc::UnboundedSender<AppServerEvent>>,
}
impl Fixture {
    fn new() -> Self {
        let values: Vec<_> = (0..2)
            .map(|_| {
                let (client, commands, events) = CloudClient::channel(4);
                (
                    AppServerSession::new(
                        BackendId::new(),
                        AppServerClient::Cloud(client),
                        ThreadParamsMode::Cloud,
                    ),
                    commands,
                    events,
                )
            })
            .collect();
        let mut sessions = Vec::new();
        let mut commands = Vec::new();
        let mut events = Vec::new();
        for (session, command, event) in values {
            sessions.push(session);
            commands.push(command);
            events.push(event);
        }
        let first = sessions.remove(0);
        Self {
            backends: BackendSessions::from_initial(first, sessions),
            commands,
            events,
        }
    }
    async fn close(self) -> io::Result<()> {
        let Self {
            backends,
            commands,
            events,
        } = self;
        let responders =
            futures::future::join_all(commands.into_iter().map(|mut commands| async move {
                while let Some(command) = commands.recv().await {
                    if matches!(&command.operation, CloudOperation::Shutdown) {
                        command
                            .response
                            .send(Ok(Ok(serde_json::Value::Null)))
                            .map_err(|_| failure("original shutdown reply lost"))?;
                        break;
                    }
                    let _ = command
                        .response
                        .send(Err(failure("owned queue fixture cleanup")));
                }
                Ok::<_, io::Error>(())
            }));
        let (closed, served) = tokio::time::timeout(WAIT, async {
            tokio::join!(backends.shutdown(), responders)
        })
        .await
        .map_err(failure)?;
        drop(events);
        closed?;
        for result in served {
            result?;
        }
        Ok(())
    }
}
fn policy(backends: &BackendSessions, refused: bool) -> io::Result<()> {
    let mut actor = Openings::default();
    let (_sender, inbox) = mpsc::unbounded_channel();
    let (answer, mut observed) = oneshot::channel();
    let (request, _cancel) = UpgradeRequest::new(move |ready| {
        let _ = answer.send(ready);
        ready
    });
    let admitted = actor.alab_admit_event(AppEvent::AlabUpgrade(request), &inbox, false, backends);
    if refused {
        check(
            admitted.is_none() && observed.try_recv() == Ok(false),
            "queued or unavailable observation did not refuse its exact original request",
        )
    } else {
        let Some(AppEvent::AlabUpgrade(original)) = admitted else {
            return Err(failure("fresh empty-owner conditional event was refused"));
        };
        check(
            matches!(
                observed.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ),
            "admission resolved a Native decision",
        )?;
        drop(original);
        check(
            observed.try_recv() == Ok(false),
            "test teardown did not retire its original unaccepted request",
        )
    }
}
#[tokio::test]
async fn actual_hidden_request_stays_on_original_transport_and_parked_fifo_across_refusal()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let body = async {
        policy(&fixture.backends, false)?;
        let thread = ThreadId::new();
        fixture.events[1].send(AppServerEvent::ServerRequest(Box::new(ServerRequest::FileChangeRequestApproval {
            request_id: RequestId::Integer(71),
            params: FileChangeRequestApprovalParams { thread_id: thread.to_string(), turn_id: "turn".into(), item_id: "item".into(), started_at_ms: 0, reason: None, grant_root: None },
        }))).map_err(failure)?;
        policy(&fixture.backends, true)?;
        check(fixture.backends.alab_upgrade_events_pending()?, "second observation consumed original raw event")?;
        // This is the ORIGINAL normal event pump, never the admission observer.
        check(fixture.backends.next_event(None).now_or_never().is_none(), "hidden request was delivered to the displayed backend")?;
        check(fixture.backends.entries[1].pending_events.len() == 1, "normal pump did not retain exactly one original request")?;
        policy(&fixture.backends, true)?;
        check(fixture.backends.entries[1].pending_events.len() == 1, "refusal changed the original parked FIFO")?;
        fixture.backends.displayed = super::super::SessionSlot(1);
        let (owner, actual) = tokio::time::timeout(WAIT, fixture.backends.next_event(None)).await.map_err(failure)?;
        check(owner == fixture.backends.entries[1].session.backend_id(), "request moved to another physical backend")?;
        check(matches!(actual, Some(crate::app_server_session::AppServerSessionEvent { event: AppServerEvent::ServerRequest(request), .. }) if request.id() == &RequestId::Integer(71)), "original request identity was not delivered once")?;
        policy(&fixture.backends, false)
    }.await;
    let cleanup = fixture.close().await;
    body.and(cleanup)
}
#[tokio::test]
async fn actual_side_deferral_preserves_queues_and_empty_eof_stays_admissible() -> io::Result<()> {
    let mut fixture = Fixture::new();
    let body = async {
        let (mut app, _app_events, _operations) = crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
        let owner = fixture.backends.entries[1].session.backend_id();
        let displayed = fixture.backends.displayed_owner();
        let thread = ThreadId::new();
        app.sidebar_mut().creation = Some(crate::app::sidebar::SideCreation {
            owner,
            state: crate::app::sidebar::CreationState::Waiting { request: uuid::Uuid::new_v4(), started: Instant::now() },
            message: None,
        });
        fixture.events[1].send(AppServerEvent::ServerNotification(Box::new(ServerNotification::McpServerStatusUpdated(McpServerStatusUpdatedNotification {
            thread_id: Some(thread.to_string()), name: "owned-tools".into(), status: McpServerStartupState::Failed,
            error: Some("original startup diagnostic".into()), failure_reason: None,
        })))).map_err(failure)?;
        let (_, event) = tokio::time::timeout(WAIT, fixture.backends.next_event(None)).await.map_err(failure)?;
        let event = event.ok_or_else(|| failure("actual side startup event missing"))?;
        check(app.defer_side_startup_notification(&event, displayed), "original App policy did not defer the actual event")?;
        fixture.backends.defer_side_startup(event);
        policy(&fixture.backends, true)?;
        check(fixture.backends.entries[1].side_startup.len() == 1, "refusal drained original side startup event")?;
        fixture.backends.displayed = super::super::SessionSlot(1);
        app.ensure_thread_channel(thread);
        fixture.backends.replay_ready_side_startup(&app);
        policy(&fixture.backends, true)?;
        let (_, event) = tokio::time::timeout(WAIT, fixture.backends.next_event(None)).await.map_err(failure)?;
        check(matches!(event, Some(crate::app_server_session::AppServerSessionEvent { event: AppServerEvent::ServerNotification(notification), .. }) if matches!(notification.as_ref(), ServerNotification::McpServerStatusUpdated(status) if status.error.as_deref() == Some("original startup diagnostic"))), "normal replay changed or lost the original event")?;
        policy(&fixture.backends, false)?;
        drop(fixture.events.remove(1));
        // Queue-only policy: closure alone is not observable backlog.
        policy(&fixture.backends, false)?;
        let (closed_owner, event) = tokio::time::timeout(WAIT, fixture.backends.next_event(None)).await.map_err(failure)?;
        check(closed_owner == owner && event.is_none(), "ordinary actor did not observe original EOF")?;
        check(!fixture.backends.entries[1].listening, "ordinary EOF did not retire event listening")?;
        policy(&fixture.backends, false)?;
        let mut actor = Openings::default(); let (_sender, inbox) = mpsc::unbounded_channel();
        check(matches!(actor.alab_admit_event(AppEvent::StartQueuedSavedChat, &inbox, false, &fixture.backends), Some(AppEvent::StartQueuedSavedChat)), "queue observation stopped unrelated ordinary actor work")
    }.await;
    let cleanup = fixture.close().await;
    body.and(cleanup)
}
