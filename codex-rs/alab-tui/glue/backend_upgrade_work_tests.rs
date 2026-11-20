// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual physical Cloud transports and original model/picker producers.
//! UpgradeRequest::new checks decision ownership only, never Native authority.
use super::*;
use crate::app::sidebar_loading::Openings;
use crate::app_event::AppEvent;
use crate::app_event::ExitMode;
use crate::app_event_sender::AppEventSender;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use codex_alab_core::session::UpgradeCancellation;
use codex_alab_core::session::UpgradeRequest;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudCommand;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::RequestId;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

const WAIT: Duration = Duration::from_secs(60);
fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn check(value: bool, message: &'static str) -> io::Result<()> {
    if value { Ok(()) } else { Err(failure(message)) }
}
fn model_reply() -> serde_json::Value {
    serde_json::json!({"data":[{
        "id":"owned-cut-model", "model":"owned-cut-model", "displayName":"Owned cut model",
        "description":"Original transport reply", "hidden":false,
        "supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Medium"}],
        "defaultReasoningEffort":"medium", "isDefault":true
    }],"nextCursor":null})
}
fn physical(
    owner: BackendId,
) -> (
    AppServerSession,
    mpsc::Receiver<CloudCommand>,
    mpsc::UnboundedSender<AppServerEvent>,
) {
    let (client, commands, events) = CloudClient::channel(8);
    // Remote mode is intentional: Cloud mode skips original model prefetch.
    (
        AppServerSession::new(
            owner,
            AppServerClient::Cloud(client),
            ThreadParamsMode::Remote,
        ),
        commands,
        events,
    )
}
struct Fixture {
    backends: BackendSessions,
    commands: Vec<mpsc::Receiver<CloudCommand>>,
    events: Vec<mpsc::UnboundedSender<AppServerEvent>>,
    retired: Vec<AppServerSession>,
}
impl Fixture {
    fn new() -> Self {
        let (first, first_commands, first_events) = physical(BackendId::new());
        let (second, second_commands, second_events) = physical(BackendId::new());
        Self {
            backends: BackendSessions::from_initial(first, vec![second]),
            commands: vec![first_commands, second_commands],
            events: vec![first_events, second_events],
            retired: Vec::new(),
        }
    }
    async fn inactive_second(&mut self) -> io::Result<()> {
        let owner = self.backends.entries[1].session.backend_id();
        self.events[1]
            .send(AppServerEvent::Disconnected {
                message: "owned inactive backend".into(),
            })
            .map_err(failure)?;
        let (actual, event) = tokio::time::timeout(WAIT, self.backends.next_event(None))
            .await
            .map_err(failure)?;
        check(
            actual == owner
                && matches!(
                    event,
                    Some(crate::app_server_session::AppServerSessionEvent {
                        event: AppServerEvent::Disconnected { .. },
                        ..
                    })
                ),
            "original backend disconnect was not observed",
        )?;
        // A delivered disconnect still occupies the actual event stream. Close
        // its original Cloud producer, then let the real backend pump observe EOF.
        drop(self.events.remove(1));
        let (closed_owner, closed) = tokio::time::timeout(WAIT, self.backends.next_event(None))
            .await
            .map_err(failure)?;
        check(
            closed_owner == owner && closed.is_none(),
            "original backend EOF was not observed",
        )?;
        check(
            !self.backends.entries[1].listening && self.backends.displayed.0 == 0,
            "second physical backend was not inactive/parked",
        )
    }
    async fn close(self) -> io::Result<()> {
        let Self {
            backends,
            commands,
            events,
            retired,
        } = self;
        let responders =
            futures::future::join_all(commands.into_iter().map(|mut receiver| async move {
                while let Some(command) = receiver.recv().await {
                    if matches!(&command.operation, CloudOperation::Shutdown) {
                        command
                            .response
                            .send(Ok(Ok(serde_json::Value::Null)))
                            .map_err(|_| failure("original cleanup shutdown response was lost"))?;
                        break;
                    }
                    let _sent = command
                        .response
                        .send(Err(failure("owned admission fixture cleanup")));
                }
                Ok::<_, io::Error>(())
            }));
        let shutdown = async move {
            let mut first = backends.shutdown().await.err();
            for session in retired {
                if let Err(error) = session.shutdown().await {
                    first.get_or_insert(error);
                }
            }
            first.map_or(Ok(()), Err)
        };
        let (closed, served) =
            tokio::time::timeout(WAIT, async { tokio::join!(shutdown, responders) })
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
fn request() -> (UpgradeRequest, UpgradeCancellation, oneshot::Receiver<bool>) {
    let (answer, receiver) = oneshot::channel();
    let (request, cancellation) = UpgradeRequest::new(move |ready| {
        let _ = answer.send(ready);
        ready
    });
    (request, cancellation, receiver)
}
fn refuse(
    actor: &mut Openings,
    inbox: &mpsc::UnboundedReceiver<AppEvent>,
    backends: &BackendSessions,
) -> io::Result<()> {
    let (original, _cancellation, mut answer) = request();
    let admitted = actor.alab_admit_event(AppEvent::AlabUpgrade(original), inbox, false, backends);
    check(
        admitted.is_none(),
        "conditional actor admitted the original busy request",
    )?;
    check(
        answer.try_recv() == Ok(false),
        "refusal did not resolve the original request exactly once",
    )
}
fn admit(
    actor: &mut Openings,
    inbox: &mpsc::UnboundedReceiver<AppEvent>,
    backends: &BackendSessions,
) -> io::Result<()> {
    let (original, _cancellation, mut answer) = request();
    let Some(AppEvent::AlabUpgrade(original)) =
        actor.alab_admit_event(AppEvent::AlabUpgrade(original), inbox, false, backends)
    else {
        return Err(failure(
            "settled actual backends did not preserve the fresh original request",
        ));
    };
    check(
        matches!(answer.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
        "admission dispatched or replaced the original decision",
    )?;
    check(
        original.decide(true) && answer.try_recv() == Ok(true),
        "returned event did not retain its original decision owner",
    )
}
fn model_request(command: &CloudCommand, expected: &RequestId) -> io::Result<()> {
    check(
        matches!(&command.operation, CloudOperation::Request(request) if matches!(request.as_ref(), ClientRequest::ModelList { request_id, params } if request_id == expected && params.cursor.is_none() && params.limit.is_none() && params.include_hidden == Some(true) && params.model_provider.is_none())),
        "original model producer request or parameters changed",
    )
}

#[tokio::test]
async fn actual_upgrade_admission_refuses_live_work_on_listening_and_inactive_backends()
-> io::Result<()> {
    for inactive in [false, true] {
        let mut fixture = Fixture::new();
        let body = async {
            if inactive { fixture.inactive_second().await?; }
            let mut actor = Openings::default();
            let (sender, mut inbox) = mpsc::unbounded_channel();
            let ready = fixture.backends.alab_try_upgrade_jobs()?.ok_or_else(|| failure("actual empty physical baseline was not ready"))?;
            ready.current(&fixture.backends)?;
            let id = uuid::Uuid::new_v4();
            fixture.backends.entries[1].session.fetch_models(id, AppEventSender::new(sender.clone()));
            let command = tokio::time::timeout(WAIT, fixture.commands[1].recv()).await.map_err(failure)?.ok_or_else(|| failure("actual picker model RPC absent"))?;
            model_request(&command, &RequestId::String(format!("model-list-{id}")))?;
            check(!actor.alab_events_pending(&inbox), "actor queue would mask the held-work guard")?;
            refuse(&mut actor, &inbox, &fixture.backends)?;
            check(fixture.commands[1].try_recv().is_err(), "refusal resubmitted the original picker RPC")?;
            check(matches!(actor.alab_admit_event(AppEvent::StartQueuedSavedChat, &inbox, true, &fixture.backends), Some(AppEvent::StartQueuedSavedChat)), "non-upgrade event was filtered by pending Book work")?;
            command.response.send(Ok(Ok(serde_json::json!({"data":[],"nextCursor":null})))).map_err(|_| failure("refusal lost the original picker response owner"))?;
            let cut = fixture.backends.entries[1].session.alab_capture_original_session_jobs()?;
            let joined = tokio::time::timeout(WAIT, cut.observe()).await.map_err(failure)??;
            joined.current(&fixture.backends.entries[1].session)?;
            // Completion publication stays in the ORIGINAL App FIFO. Refuse,
            // preserve its order, and require a fresh request after processing it.
            sender.send(AppEvent::StartQueuedSavedChat).map_err(failure)?;
            refuse(&mut actor, &inbox, &fixture.backends)?;
            check(matches!(inbox.try_recv(), Ok(AppEvent::ModelsLoaded { request_id, result: Ok(models) }) if request_id == id && models.is_empty()), "original picker UUID/result/FIFO head was consumed or replaced")?;
            check(matches!(inbox.try_recv(), Ok(AppEvent::StartQueuedSavedChat)) && inbox.is_empty(), "original App FIFO order changed")?;
            // The persistent actor inbox is independent of the backend App inbox.
            (actor.alab_shutdown_callback())();
            refuse(&mut actor, &inbox, &fixture.backends)?;
            let actual = tokio::time::timeout(WAIT, actor.next_event(&mut inbox)).await.map_err(failure)?
                .ok_or_else(|| failure("persistent actor inbox closed before its completion"))?;
            check(matches!(actual, AppEvent::Exit(ExitMode::ShutdownFirst)), "persistent actor completion was lost")?;
            admit(&mut actor, &inbox, &fixture.backends)
        }.await;
        let cleanup = fixture.close().await;
        body?;
        cleanup?;
    }
    Ok(())
}

#[tokio::test]
async fn actual_upgrade_admission_refuses_prefetch_and_preserves_ordinary_cache_consumption()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let (app, _events, _ops) =
        crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
    let body = async {
        fixture.inactive_second().await?;
        let mut actor = Openings::default();
        let (_sender, inbox) = mpsc::unbounded_channel();
        fixture.backends.entries[1].session.prefetch_model_list();
        let command = tokio::time::timeout(WAIT, fixture.commands[1].recv())
            .await
            .map_err(failure)?
            .ok_or_else(|| failure("actual prefetched model RPC absent"))?;
        model_request(&command, &RequestId::Integer(1))?;
        check(
            !actor.alab_events_pending(&inbox),
            "actor queue would mask model-Book guard",
        )?;
        refuse(&mut actor, &inbox, &fixture.backends)?;
        check(
            fixture.commands[1].try_recv().is_err(),
            "busy refusal resubmitted actual prefetch",
        )?;
        command
            .response
            .send(Ok(Ok(model_reply())))
            .map_err(|_| failure("refusal lost original model cache response owner"))?;
        let cut = fixture.backends.entries[1]
            .session
            .alab_capture_original_model_jobs();
        let joined = tokio::time::timeout(WAIT, cut.observe())
            .await
            .map_err(failure)??;
        let _cache_data = joined.current(&fixture.backends.entries[1].session)?;
        admit(&mut actor, &inbox, &fixture.backends)?;
        let old_ready = fixture
            .backends
            .alab_try_upgrade_jobs()?
            .ok_or_else(|| failure("joined prefetch was not current"))?;
        let account = GetAccountResponse {
            account: None,
            user_id: None,
            workspace_routing: None,
            requires_openai_auth: false,
        };
        let consume = fixture.backends.entries[1]
            .session
            .bootstrap_with_account(&app.config, account);
        let responses = async {
            let mut requirements = 0;
            let mut collaboration = 0;
            for _ in 0..2 {
                let command = fixture.commands[1]
                    .recv()
                    .await
                    .ok_or_else(|| failure("ordinary cache consumption companion absent"))?;
                let reply = match &command.operation {
                    CloudOperation::Request(request) => match request.as_ref() {
                        ClientRequest::ConfigRequirementsRead { .. } => {
                            requirements += 1;
                            serde_json::json!({"requirements":null})
                        }
                        ClientRequest::CollaborationModeList { .. } => {
                            collaboration += 1;
                            serde_json::json!({"data":[]})
                        }
                        _ => {
                            let _ = command
                                .response
                                .send(Err(failure("cache consumption repeated model discovery")));
                            return Err(failure(
                                "retained prefetch was not consumed by the original bootstrap",
                            ));
                        }
                    },
                    _ => {
                        let _ = command
                            .response
                            .send(Err(failure("unexpected original bootstrap operation")));
                        return Err(failure("ordinary bootstrap operation changed"));
                    }
                };
                command
                    .response
                    .send(Ok(Ok(reply)))
                    .map_err(|_| failure("ordinary bootstrap response owner lost"))?;
            }
            check(
                requirements == 1 && collaboration == 1,
                "ordinary bootstrap companion requests repeated",
            )
        };
        let (bootstrap, served) =
            tokio::time::timeout(WAIT, async { tokio::join!(consume, responses) })
                .await
                .map_err(failure)?;
        served?;
        let bootstrap = bootstrap.map_err(failure)?;
        check(
            bootstrap.available_models.len() == 1
                && bootstrap.available_models[0].model == "owned-cut-model",
            "ordinary bootstrap lost the exact retained model cache",
        )?;
        check(
            fixture.commands[1].try_recv().is_err(),
            "cache consumption emitted another model RPC",
        )?;
        check(
            old_ready.current(&fixture.backends).is_err(),
            "legitimate cache consumption did not invalidate its old model cut",
        )?;
        admit(&mut actor, &inbox, &fixture.backends)
    }
    .await;
    let cleanup = fixture.close().await;
    body?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn actual_upgrade_admission_guard_rejects_new_generations_and_same_id_replacement()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let body = async {
        let ready = fixture.backends.alab_try_upgrade_jobs()?.ok_or_else(|| failure("actual physical baseline absent"))?;
        let id = uuid::Uuid::new_v4();
        let (sender, mut events) = mpsc::unbounded_channel();
        fixture.backends.entries[0].session.fetch_models(id, AppEventSender::new(sender));
        check(ready.current(&fixture.backends).is_err(), "new actual Work job left an old aggregate current")?;
        let command = tokio::time::timeout(WAIT, fixture.commands[0].recv()).await.map_err(failure)?.ok_or_else(|| failure("new generation picker RPC absent"))?;
        model_request(&command, &RequestId::String(format!("model-list-{id}")))?;
        command.response.send(Ok(Ok(serde_json::json!({"data":[],"nextCursor":null})))).map_err(|_| failure("actual generation response owner lost"))?;
        let cut = fixture.backends.entries[0].session.alab_capture_original_session_jobs()?;
        tokio::time::timeout(WAIT, cut.observe()).await.map_err(failure)??;
        check(matches!(events.try_recv(), Ok(AppEvent::ModelsLoaded { request_id, result: Ok(models) }) if request_id == id && models.is_empty()), "actual generation result changed")?;
        let ready = fixture.backends.alab_try_upgrade_jobs()?.ok_or_else(|| failure("settled generation was not current"))?;
        let displayed = fixture.backends.displayed;
        let displayed_owner = fixture.backends.displayed_owner();
        fixture.backends.displayed = super::super::SessionSlot(1);
        check(ready.current(&fixture.backends).is_err(), "displayed owner switch left an old aggregate current")?;
        fixture.backends.displayed = displayed;
        ready.current(&fixture.backends)?;
        // Preserve the same displayed physical owner while changing only its
        // position in the original ordered inventory; restore the SAME cut.
        fixture.backends.entries.swap(0, 1);
        fixture.backends.displayed = super::super::SessionSlot(1);
        check(fixture.backends.displayed_owner() == displayed_owner, "inventory-order control changed its displayed physical owner")?;
        check(ready.current(&fixture.backends).is_err(), "physical inventory reorder left an old aggregate current")?;
        fixture.backends.entries.swap(0, 1);
        fixture.backends.displayed = displayed;
        ready.current(&fixture.backends)?;
        let owner = fixture.backends.entries[1].session.backend_id();
        let (replacement, commands, events) = physical(owner);
        let outgoing = std::mem::replace(&mut fixture.backends.entries[1].session, replacement);
        fixture.retired.push(outgoing);
        fixture.commands.push(commands);
        fixture.events.push(events);
        check(ready.current(&fixture.backends).is_err(), "same logical BackendId hid a replacement physical Book lineage")?;
        let mut actor = Openings::default();
        let (_sender, inbox) = mpsc::unbounded_channel();
        admit(&mut actor, &inbox, &fixture.backends)?;
        // Cancellation remains the SAME original decision even with ready Books.
        let (original, cancellation, mut answer) = request();
        drop(cancellation);
        let Some(AppEvent::AlabUpgrade(original)) = actor.alab_admit_event(AppEvent::AlabUpgrade(original), &inbox, false, &fixture.backends) else {
            return Err(failure("settled admission changed cancelled request custody"));
        };
        check(!original.decide(true) && answer.try_recv() == Ok(false), "admission rebased or resurrected a cancelled original request")
    }.await;
    let cleanup = fixture.close().await;
    body?;
    cleanup?;
    Ok(())
}
