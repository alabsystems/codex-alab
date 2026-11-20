// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::backend_sessions::BackendSessions;
use crate::app_event::ExitMode;
use codex_alab_core::session::UpgradeRequest;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudCommand;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadStartParams;
use serde_json::json;

fn session(backend: BackendId) -> (AppServerSession, mpsc::Receiver<CloudCommand>) {
    let (client, commands, _events) = CloudClient::channel(2);
    (
        AppServerSession::new(
            backend,
            AppServerClient::Cloud(client),
            crate::ThreadParamsMode::Embedded,
        ),
        commands,
    )
}

fn assert_upgrade_decision(openings: &mut Openings, backends: &BackendSessions, admitted: bool) {
    let (_events, inbox) = mpsc::unbounded_channel();
    let (sender, mut decision) = tokio::sync::oneshot::channel();
    let (request, _cancellation) = UpgradeRequest::new(move |ready| {
        sender.send(ready).expect("original upgrade decision");
        ready
    });
    let event = openings.alab_admit_event(AppEvent::AlabUpgrade(request), &inbox, false, backends);
    if admitted {
        let Some(AppEvent::AlabUpgrade(request)) = event else {
            panic!("fresh upgrade was refused after the original work settled")
        };
        assert!(matches!(
            decision.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(request.decide(true));
    } else {
        assert!(event.is_none());
    }
    assert_eq!(decision.try_recv(), Ok(admitted));
}

fn assert_explicit_exit(openings: &mut Openings, backends: &BackendSessions) {
    let (_events, inbox) = mpsc::unbounded_channel();
    assert!(matches!(
        openings.alab_admit_event(
            AppEvent::Exit(ExitMode::ShutdownFirst),
            &inbox,
            false,
            backends
        ),
        Some(AppEvent::Exit(ExitMode::ShutdownFirst))
    ));
}

fn assert_account_decision(app: &mut App, backends: &BackendSessions, admitted: bool) {
    let (_events, inbox) = mpsc::unbounded_channel();
    let (reply, mut decision) = tokio::sync::oneshot::channel();
    let thread_id = app.chat_widget.thread_id().expect("displayed conversation");
    let event = AppEvent::AlabAccountsCheckInput {
        thread_id: Some(thread_id),
        reply,
    };
    assert!(
        app.alab_admit_upgrade_event(event, &inbox, false, None, backends)
            .is_none()
    );
    assert_eq!(decision.try_recv(), Ok(admitted));
}

#[tokio::test]
async fn recovered_saved_resume_releases_only_retired_physical_connection() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    let target = Target {
        backend_id: BackendId::new(),
        parent: ThreadId::new(),
        child: ThreadId::new(),
    };
    let (mut c1, mut old_commands) = session(target.backend_id);
    let (mut c2, mut new_commands) = session(target.backend_id);
    let opening = Uuid::new_v4();
    let released = || SavedChatRelease::reserve(&c1, target, opening);
    assert!(retired_release(Origin::Flight(Some(released())), &c1, None).is_none());
    assert!(retired_release(Origin::Flight(Some(released())), &c2, None).is_none());
    assert!(retired_release(Origin::Handoff, &c2, None).is_none());

    let mut first = released();
    let mut second = released();
    assert!(first.retire_if_owned_by(&c1));
    assert!(second.retire_if_owned_by(&c1));
    let mut flight = Flight {
        request: Uuid::new_v4(),
        started: Instant::now(),
        retired: false,
        incoming: None,
    };
    flight.incoming = Some(Incoming::begin(&mut app, &mut c1, target, flight.request).await);
    assert!(retired_release(Origin::Flight(Some(first)), &c2, Some(&flight)).is_none());
    app.thread_event_channels.remove(&target.child);
    flight.incoming = Some(Incoming::begin(&mut app, &mut c2, target, flight.request).await);
    let c2_store = app.thread_event_channels[&target.child].store.clone();
    let release = retired_release(Origin::Flight(Some(second)), &c2, Some(&flight))
        .expect("historical C1 release");
    let backends = BackendSessions::new(c2);
    app.app_server_backend_id = Some(target.backend_id);
    let displayed =
        crate::app::tests::test_thread_session(target.parent, app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session_quiet(displayed);
    assert_upgrade_decision(&mut app.saved_openings, &backends, true);
    assert_account_decision(&mut app, &backends, true);
    app.saved_openings.publish_historical_release(release);
    let command = tokio::time::timeout(Duration::from_secs(60), old_commands.recv())
        .await?
        .expect("C1 unsubscribe");
    let CloudOperation::Request(request) = command.operation else {
        panic!("C1 request")
    };
    let ClientRequest::ThreadUnsubscribe { request_id, params } = *request else {
        panic!("C1 unsubscribe")
    };
    assert_eq!(
        request_id,
        RequestId::String(format!("saved-chat-release-{opening}"))
    );
    assert_eq!(params.thread_id, target.child.to_string());
    // C2 is idle while the retired C1's actual unsubscribe is still on the wire.
    assert_upgrade_decision(&mut app.saved_openings, &backends, false);
    assert_explicit_exit(&mut app.saved_openings, &backends);
    assert_account_decision(&mut app, &backends, false);
    let cut = c1.alab_capture_original_session_jobs()?;
    command
        .response
        .send(Ok(Ok(json!({"status":"unsubscribed"}))))
        .expect("C1 response");
    tokio::time::timeout(Duration::from_secs(60), cut.observe()).await??;
    // The worker has joined, but the chooser hasn't consumed its original receipt.
    assert_account_decision(&mut app, &backends, false);
    let actor = &mut app.saved_openings;
    assert_upgrade_decision(actor, &backends, false);
    let completion = actor.completions.try_recv()?;
    let AppEvent::SavedChatReleased {
        target: original,
        request,
        receipt,
    } = completion
    else {
        panic!("original retired release callback")
    };
    assert_eq!(original, target);
    assert_eq!(request, opening);
    assert!(
        !actor
            .alab_observe_historical_release(
                Target {
                    child: ThreadId::new(),
                    ..target
                },
                request,
                &receipt
            )
            .await
    );
    assert!(
        !actor
            .alab_observe_historical_release(original, Uuid::new_v4(), &receipt)
            .await
    );
    assert_upgrade_decision(actor, &backends, false);
    // A different physical publication can carry the very same public correlation data.
    let (other, mut other_wire) = session(target.backend_id);
    let (events, mut other_callbacks) = mpsc::unbounded_channel();
    let mut independent = SavedChatRelease::reserve(&other, target, request)
        .publish_historical(AppEventSender::new(events));
    let other_command = tokio::time::timeout(Duration::from_secs(60), other_wire.recv())
        .await?
        .expect("independent physical request");
    other_command
        .response
        .send(Ok(Ok(json!({"status":"unsubscribed"}))))
        .expect("independent physical reply");
    let other_cut = other.alab_capture_original_session_jobs()?;
    tokio::time::timeout(Duration::from_secs(60), other_cut.observe()).await??;
    let AppEvent::SavedChatReleased {
        target: same_target,
        request: same_request,
        receipt: other_receipt,
    } = other_callbacks.try_recv()?
    else {
        panic!("independent release receipt")
    };
    assert_eq!((same_target, same_request), (original, request));
    assert_eq!(other_receipt.wire_request, receipt.wire_request);
    assert_eq!(other_receipt.outcome, receipt.outcome);
    assert!(
        independent
            .observe_and_join(same_target, same_request, &other_receipt)
            .await
    );
    assert!(
        !actor
            .alab_observe_historical_release(same_target, same_request, &other_receipt)
            .await
    );
    assert_upgrade_decision(actor, &backends, false);
    assert!(
        actor
            .alab_observe_historical_release(original, request, &receipt)
            .await
    );
    assert!(
        !actor
            .alab_observe_historical_release(original, request, &receipt)
            .await
    );
    assert_upgrade_decision(actor, &backends, true);
    assert_account_decision(&mut app, &backends, true);
    assert!(matches!(
        old_commands.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        new_commands.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(flight.incoming.as_ref().unwrap().owns_channel(
        app.thread_event_channels.get(&target.child),
        target.backend_id
    ));
    assert_eq!(c2_store.lock().await.attachment_prefix, Some(0));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn account_restart_distinguishes_owned_saved_work_from_passive_history() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let target = Target {
        backend_id: BackendId::new(),
        parent: ThreadId::new(),
        child: ThreadId::new(),
    };
    let (server, mut commands) = session(target.backend_id);
    let mut backends = BackendSessions::new(server);
    let displayed = backends.displayed;
    app.app_server_backend_id = Some(target.backend_id);
    app.active_thread_id = Some(target.parent);
    let conversation =
        crate::app::tests::test_thread_session(target.parent, app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session_quiet(conversation);
    let (disconnected, _record) =
        crate::owned_local_frontend::ConstructionRecord::side_preparation_fixture()?;
    let id = Uuid::new_v4();
    let missing =
        MissingSavedResume::for_disconnected_target(disconnected, handoff(id, target.child)?);
    assert!(
        app.saved_openings
            .alab_remember_saved_resume(
                target,
                id,
                Some(missing),
                None,
                backends.session_at(displayed),
                true
            )
            .is_none()
    );
    assert_account_decision(&mut app, &backends, false);
    // An entered history row owns observation alone. This fixture has no Native tenure.
    app.saved_openings
        .saved_history
        .records
        .get_mut(&id)
        .unwrap()
        .origin = Origin::Handoff;
    assert_account_decision(&mut app, &backends, true);

    app.sidebar_mut().show(
        sidebar::SidebarContent::Chooser,
        sidebar::PaneFocus::Sidebar,
    );
    app.sidebar_mut()
        .update_saved_chats(vec![target.child.into()]);
    crate::boxed_future!(app.open_saved_chat(&mut tui, backends.session_at_mut(displayed), target))
        .await?;
    let read = tokio::time::timeout(Duration::from_secs(60), commands.recv())
        .await?
        .expect("original read");
    assert!(matches!(read.operation, CloudOperation::Request(request)
        if matches!(request.as_ref(), ClientRequest::ThreadRead { params, .. } if params.thread_id == target.child.to_string())));
    assert_account_decision(&mut app, &backends, false);
    app.saved_openings.abandon();
    crate::boxed_future!(app.open_saved_chat(&mut tui, backends.session_at_mut(displayed), target))
        .await?;
    assert_eq!(app.waiting_saved_chat(), Some(target));
    read.response
        .send(Ok(Err(codex_app_server_protocol::JSONRPCErrorError {
            code: -32602,
            message: format!("thread not loaded: {}", target.child),
            data: None,
        })))
        .expect("original read reply");
    let prepared = tokio::time::timeout(
        Duration::from_secs(60),
        app.saved_openings.completions.recv(),
    )
    .await?
    .expect("original preparation");
    assert!(
        matches!(&prepared, AppEvent::SavedChatPrepared { target: actual, .. } if *actual == target)
    );
    crate::boxed_future!(app.handle_event(&mut tui, backends.session_at_mut(displayed), prepared))
        .await?;
    assert_eq!(app.saved_openings.saved_history_slots(), 0);
    assert_eq!(app.waiting_saved_chat(), Some(target));
    assert_account_decision(&mut app, &backends, false);
    app.sidebar_mut()
        .show(sidebar::SidebarContent::Chooser, sidebar::PaneFocus::Main);
    assert_account_decision(&mut app, &backends, true);
    let wake = app.saved_openings.completions.try_recv()?;
    assert!(matches!(wake, AppEvent::StartQueuedSavedChat));
    crate::boxed_future!(app.handle_event(&mut tui, backends.session_at_mut(displayed), wake))
        .await?;
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn saved_resume_handoff_never_reads_an_unowned_connection() -> Result<()> {
    let mut openings = Openings::default();
    let (remote, mut commands) = session(BackendId::new());
    let handoff = handoff(Uuid::new_v4(), ThreadId::new())?;
    let checkpoint = std::sync::Arc::downgrade(handoff.checkpoint());
    let query = std::sync::Arc::downgrade(handoff.query());
    assert!(
        openings
            .alab_restore_saved_resume_queries(&remote, vec![handoff])
            .is_err()
    );
    assert!(openings.saved_history.records.is_empty());
    assert!(checkpoint.upgrade().is_none() && query.upgrade().is_none());
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    Ok(())
}

fn query(opening: Uuid, child: ThreadId) -> Result<SavedResumeQuery> {
    Ok(serde_json::from_value(query_data(opening, child))?)
}

fn query_data(opening: Uuid, child: ThreadId) -> serde_json::Value {
    let opening = opening.simple().to_string();
    json!({
        "phase":"readSavedResume",
        "original":{
            "preparation":opening, "savedOpening":opening, "method":"thread/resume",
            "parameters":ThreadResumeParams { thread_id:child.to_string(), ..Default::default() },
            "selection":{"prospectiveCreate":ThreadStartParams::default(),"presentation":"sideConversation"}
        },
        "offer":{"preparation":opening,"claim":"a".repeat(32),"sourceOperation":"b".repeat(32),"selection":"c".repeat(32)}
    })
}

#[cfg(unix)]
fn handoff(opening: Uuid, child: ThreadId) -> Result<SavedResumeHandoff> {
    let mut data = query_data(opening, child);
    let owner = codex_app_server_client::ClientConstructionOperation::for_saved_resume(
        serde_json::from_value(data["original"]["selection"].clone())?,
        &opening.simple().to_string(),
        &child.to_string(),
    )?;
    // The actor fixtures start with already-issued checkpoint data.
    let mut checkpoint = serde_json::to_value(owner.checkpoint())?;
    checkpoint["frozen"] = data["original"].take();
    checkpoint["offer"] = data["offer"].take();
    checkpoint["mutationAttempted"] = true.into();
    let checkpoint = std::sync::Arc::new(serde_json::from_value(checkpoint)?);
    Ok(SavedResumeHandoff::from_checkpoint(&checkpoint)?.expect("issued opening"))
}

#[cfg(unix)]
#[tokio::test]
async fn detached_saved_openings_refuse_upgrade_across_unowned_same_child_backends() -> Result<()> {
    let (displayed, mut displayed_wire) = session(BackendId::new());
    let (parked, mut parked_wire) = session(BackendId::new());
    let owners = [displayed.backend_id(), parked.backend_id()];
    let backends = BackendSessions::from_initial(displayed, vec![parked]);
    let mut actor = Openings::default();
    let child = ThreadId::new();
    let parent = ThreadId::new();
    let mut openings = Vec::new();
    assert_upgrade_decision(&mut actor, &backends, true);
    assert!(actor.alab_next_saved_wake(&backends, None).is_none());
    for backend_id in owners {
        let opening = Uuid::new_v4();
        openings.push(opening);
        let (original, _record) =
            crate::owned_local_frontend::ConstructionRecord::side_preparation_fixture()?;
        let missing =
            MissingSavedResume::for_disconnected_target(original, handoff(opening, child)?);
        let server = backends.session(backend_id).expect("registered backend");
        assert!(
            actor
                .alab_remember_saved_resume(
                    Target {
                        backend_id,
                        child,
                        parent
                    },
                    opening,
                    Some(missing),
                    None,
                    server,
                    true,
                )
                .is_none()
        );
    }
    // A cached timer is just scheduling data. Revalidate against today's
    // actual reader before either arming the event loop or issuing any read.
    let (previous_parked, _retired_wire) = session(owners[1]);
    let now = TokioInstant::now();
    actor.saved_history.wakes.insert(
        owners[1],
        OwnerWake {
            reader: previous_parked.alab_physical_work_owner(),
            opening: openings[1],
            due: Some(now),
            next_batch: now,
        },
    );
    assert!(actor.alab_next_saved_wake(&backends, None).is_none());
    actor.alab_wake_saved_history(&backends, None, owners[1]);
    actor.alab_wake_saved_history(&backends, Some(owners[1]), owners[1]);
    assert!(actor.saved_history.records[&openings[1]].attempt.is_none());
    assert!(matches!(
        parked_wire.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    for _ in 0..2 {
        assert_upgrade_decision(&mut actor, &backends, false);
        assert_explicit_exit(&mut actor, &backends);
        assert_eq!(actor.saved_history.records.len(), 2);
        assert!(actor.alab_next_saved_wake(&backends, None).is_none());
        assert!(
            actor
                .alab_next_saved_wake(&backends, Some(owners[0]))
                .is_none()
        );
        for opening in &openings {
            let record = &actor.saved_history.records[opening];
            assert!(record.attempt.is_none());
            assert_eq!(
                record.missing.query().opening(),
                opening.simple().to_string()
            );
        }
        assert!(actor.completions.is_empty());
        assert!(matches!(
            displayed_wire.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            parked_wire.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }
    // Model an outstanding local reader on the parked endpoint. A missing endpoint's
    // notification only rearms the exact recorded attempt, even for identical child IDs.
    let parked_opening = openings[1];
    let parked_server = backends.session(owners[1]).expect("parked backend");
    let attempt = Uuid::new_v4();
    {
        let parked_record = actor
            .saved_history
            .records
            .get_mut(&parked_opening)
            .unwrap();
        parked_record.schedule.started();
        parked_record.attempt = Some((attempt, parked_server.alab_physical_work_owner()));
    }
    let lost_reader = |backend, opening, attempt| Observation {
        backend,
        opening,
        attempt,
        outcome: Outcome::ReadFailed,
    };
    for unrelated in [
        lost_reader(owners[0], parked_opening, attempt),
        lost_reader(owners[1], openings[0], attempt),
        lost_reader(owners[1], parked_opening, Uuid::new_v4()),
    ] {
        actor.alab_detach_saved_history_observer(&unrelated);
        let parked_record = &actor.saved_history.records[&parked_opening];
        assert!(parked_record.schedule.next.is_none());
        assert!(parked_record.attempt.as_ref().is_some_and(
            |(id, reader)| *id == attempt && reader.belongs_to_connection(parked_server)
        ));
    }
    actor.alab_detach_saved_history_observer(&lost_reader(owners[1], parked_opening, attempt));
    assert!(
        actor.saved_history.records[&parked_opening]
            .schedule
            .next
            .is_some()
    );
    assert!(
        actor.saved_history.records[&parked_opening]
            .attempt
            .is_none()
    );
    assert!(actor.saved_history.records.contains_key(&openings[0]));
    assert_upgrade_decision(&mut actor, &backends, false);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn historical_release_wakes_a_chooser_at_capacity_on_the_current_connection() -> Result<()> {
    exercise_parked_history_capacity(Outcome::Resumed).await
}

#[cfg(unix)]
#[tokio::test]
async fn parked_saved_resume_rejection_wakes_displayed_chooser_at_global_capacity() -> Result<()> {
    exercise_parked_history_capacity(Outcome::Rejected(-32021)).await
}

#[cfg(unix)]
async fn exercise_parked_history_capacity(outcome: Outcome) -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let target = Target {
        backend_id: BackendId::new(),
        parent: ThreadId::new(),
        child: ThreadId::new(),
    };
    let queued = Target {
        backend_id: BackendId::new(),
        parent: ThreadId::new(),
        child: target.child,
    };
    let (c1, mut retired_wire) = session(target.backend_id);
    let (parked, mut parked_wire) = session(target.backend_id);
    let (mut displayed, mut current_wire) = session(queued.backend_id);
    app.app_server_backend_id = Some(queued.backend_id);
    app.active_thread_id = Some(queued.parent);
    app.sidebar_mut().show(
        sidebar::SidebarContent::Chooser,
        sidebar::PaneFocus::Sidebar,
    );

    let (disconnected, _record) =
        crate::owned_local_frontend::ConstructionRecord::side_preparation_fixture()?;
    let remember = |openings: &mut Openings,
                    target: Target,
                    server: &AppServerSession,
                    release,
                    ticket: SavedResumeHandoff|
     -> Result<()> {
        let opening = Uuid::parse_str(ticket.query().opening())?;
        let missing = MissingSavedResume::for_disconnected_target(disconnected.clone(), ticket);
        assert!(
            openings
                .alab_remember_saved_resume(target, opening, Some(missing), release, server, true)
                .is_none()
        );
        Ok(())
    };
    let entered = Uuid::new_v4();
    let ticket = handoff(entered, queued.child)?;
    let checkpoint = std::sync::Arc::downgrade(ticket.checkpoint());
    let query = std::sync::Arc::downgrade(ticket.query());
    remember(&mut app.saved_openings, queued, &displayed, None, ticket)?;
    // Model the origin of an entered read-only ticket; this fixture has no Native tenure.
    app.saved_openings
        .saved_history
        .records
        .get_mut(&entered)
        .unwrap()
        .origin = Origin::Handoff;

    // Leave a counted row on the displayed backend to finish after the first slot frees.
    let below_capacity = Uuid::new_v4();
    remember(
        &mut app.saved_openings,
        queued,
        &displayed,
        None,
        handoff(below_capacity, queued.child)?,
    )?;
    for _ in 2..MAX_RECORDS {
        remember(
            &mut app.saved_openings,
            target,
            &parked,
            None,
            handoff(Uuid::new_v4(), target.child)?,
        )?;
    }
    let original = Uuid::new_v4();
    let mut release = SavedChatRelease::reserve(&c1, target, original);
    assert!(release.retire_if_owned_by(&c1));
    remember(
        &mut app.saved_openings,
        target,
        &parked,
        Some(release),
        handoff(original, target.child)?,
    )?;

    assert_eq!(app.saved_openings.saved_history_slots(), MAX_RECORDS);
    crate::boxed_future!(app.open_saved_chat(&mut tui, &mut displayed, queued)).await?;
    assert_eq!(app.waiting_saved_chat(), Some(queued));
    assert!(app.saved_openings.active.is_empty());
    assert!(matches!(
        current_wire.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    // Drive the transition below read authentication, then deliver the actual actor
    // events. None of these original reads may install a listener for the queued child.
    app.settle_saved_history_record(&displayed, entered, &Outcome::Pending);
    assert_eq!(checkpoint.strong_count(), 1);
    assert_eq!(query.strong_count(), 1);
    assert!(std::sync::Arc::ptr_eq(
        app.saved_openings.saved_history.records[&entered]
            .missing
            .query(),
        &query.upgrade().unwrap(),
    ));
    app.settle_saved_history_record(&displayed, entered, &Outcome::Unavailable);
    assert!(checkpoint.upgrade().is_none() && query.upgrade().is_none());
    assert!(
        !app.saved_openings
            .saved_history
            .records
            .contains_key(&entered)
    );
    assert_eq!(app.saved_openings.saved_history_slots(), MAX_RECORDS);
    assert!(app.saved_openings.completions.is_empty());
    app.settle_saved_history_record(&parked, original, &outcome);
    assert!(
        !app.saved_openings
            .saved_history
            .records
            .contains_key(&original)
    );
    if matches!(outcome, Outcome::Resumed) {
        // The retired parked connection owns the same slot until the original release
        // is received and joined. No wake or read can reach the displayed backend yet.
        assert_eq!(app.saved_openings.saved_history_slots(), MAX_RECORDS);
        assert!(app.saved_openings.completions.is_empty());
        assert!(matches!(
            current_wire.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        let command = tokio::time::timeout(Duration::from_secs(60), retired_wire.recv())
            .await?
            .expect("original retired unsubscribe");
        assert!(matches!(command.operation, CloudOperation::Request(request)
            if matches!(request.as_ref(), ClientRequest::ThreadUnsubscribe { params, .. } if params.thread_id == target.child.to_string())));
        command
            .response
            .send(Ok(Ok(json!({"status":"unsubscribed"}))))
            .expect("C1 response");
        let completion = tokio::time::timeout(
            Duration::from_secs(60),
            app.saved_openings.completions.recv(),
        )
        .await?
        .expect("C1 completion");
        assert!(
            matches!(&completion, AppEvent::SavedChatReleased { target: released, request, .. }
            if released == &target && request == &original)
        );
        crate::boxed_future!(app.handle_event(&mut tui, &mut displayed, completion)).await?;
    }
    assert_eq!(app.saved_openings.saved_history_slots(), MAX_RECORDS - 1);
    // A duplicate and a below-capacity completion for the waiting pane cannot
    // enqueue another wake. Only the first newly freed global slot mattered.
    app.settle_saved_history_record(&parked, original, &outcome);
    app.settle_saved_history_record(&displayed, below_capacity, &Outcome::Unavailable);
    assert_eq!(app.saved_openings.saved_history_slots(), MAX_RECORDS - 2);
    assert!(!app.saved_openings.alab_saved_history_full());
    let wake = app.saved_openings.completions.try_recv()?;
    assert!(matches!(wake, AppEvent::StartQueuedSavedChat));
    assert!(app.saved_openings.completions.is_empty());
    assert!(matches!(
        parked_wire.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        retired_wire.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    crate::boxed_future!(app.handle_event(&mut tui, &mut displayed, wake)).await?;
    assert!(app.saved_openings.active.contains_key(&queued.key()));
    let queued_read = tokio::time::timeout(Duration::from_secs(60), current_wire.recv())
        .await?
        .expect("current connection reads the previously waiting child");
    assert!(
        matches!(queued_read.operation, CloudOperation::Request(request)
        if matches!(request.as_ref(), ClientRequest::ThreadRead { params, .. } if params.thread_id == queued.child.to_string()))
    );
    queued_read
        .response
        .send(Ok(Err(codex_app_server_protocol::JSONRPCErrorError {
            code: -32602,
            message: format!("thread not loaded: {}", queued.child),
            data: None,
        })))
        .expect("current read response");
    let done = tokio::time::timeout(
        Duration::from_secs(60),
        app.saved_openings.completions.recv(),
    )
    .await?
    .expect("queued opening completed");
    assert!(matches!(&done, AppEvent::SavedChatPrepared { target, .. } if target == &queued));
    crate::boxed_future!(app.handle_event(&mut tui, &mut displayed, done)).await?;
    assert!(app.saved_openings.active.is_empty());
    assert!(app.saved_openings.completions.is_empty());
    assert!(matches!(
        current_wire.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        parked_wire.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        retired_wire.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    Ok(())
}

#[path = "saved_resume_deadline_tests.rs"]
mod deadline;
