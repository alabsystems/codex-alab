// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::tests::make_test_app_with_channels;
use crate::app::tests::test_thread_session;

fn check_input_admission(
    app: &mut App,
    backends: &BackendSessions,
    refused: bool,
) -> io::Result<()> {
    check_admission_with_startup_draft(app, backends, None, refused)
}

fn check_admission_with_startup_draft(
    app: &mut App,
    backends: &BackendSessions,
    draft: Option<&ComposerDraftSnapshot>,
    refused: bool,
) -> io::Result<()> {
    let (_sender, inbox) = mpsc::unbounded_channel();
    let (answer, mut observed) = oneshot::channel();
    let (request, _cancel) = UpgradeRequest::new(move |ready| {
        let _ = answer.send(ready);
        ready
    });
    let event = app.alab_admit_upgrade_event(
        AppEvent::AlabUpgrade(request),
        &inbox,
        false,
        draft,
        backends,
    );
    if refused {
        check(
            event.is_none() && observed.try_recv() == Ok(false),
            "input observation did not refuse the original upgrade",
        )
    } else {
        check(
            matches!(event, Some(AppEvent::AlabUpgrade(_))),
            "empty input owner refused the original upgrade",
        )?;
        check(
            matches!(
                observed.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ),
            "observation claimed an upgrade before dispatch",
        )
    }
}

#[cfg(unix)]
#[tokio::test]
async fn account_switch_preserves_a_submission_queued_after_its_input_check() -> io::Result<()> {
    let fixture = Fixture::new();
    let body = async {
        let mut app = crate::app::test_support::make_test_app().await;
        let thread_id = ThreadId::new();
        app.app_server_backend_id = Some(fixture.backends.displayed_owner());
        let session = test_thread_session(thread_id, app.config.cwd.to_path_buf());
        app.chat_widget.handle_thread_session_quiet(session);
        let (sender, mut inbox) = mpsc::unbounded_channel();
        let sender = crate::app_event_sender::AppEventSender::new(sender);
        let plan = "Apply the approved change";

        for has_submission in [true, false] {
            let (reply, observed) = oneshot::channel();
            sender.send(AppEvent::AlabAccountsCheckInput {
                thread_id: Some(thread_id),
                reply,
            });
            if has_submission {
                sender.send(AppEvent::ImplementPlanInFreshSession {
                    plan: plan.to_owned(),
                });
            }
            let check = app.saved_openings.next_event(&mut inbox).await.unwrap();
            assert!(
                app.alab_admit_upgrade_event(check, &inbox, false, None, &fixture.backends)
                    .is_none()
            );
            assert_eq!(observed.await.map_err(failure)?, !has_submission);
            if has_submission {
                assert!(matches!(
                    inbox.try_recv(),
                    Ok(AppEvent::ImplementPlanInFreshSession { plan: queued }) if queued == plan
                ));
            }
        }
        Ok(())
    }
    .await;
    let cleanup = fixture.close().await;
    body.and(cleanup)
}

#[cfg(unix)]
#[tokio::test]
async fn account_switch_preserves_an_inactive_conversations_unsent_draft() -> io::Result<()> {
    let fixture = Fixture::new();
    let body = async {
        let (mut app, _events, _ops) = crate::boxed_future!(make_test_app_with_channels()).await;
        let displayed = ThreadId::new();
        let parked = ThreadId::new();
        app.app_server_backend_id = Some(fixture.backends.displayed_owner());
        let session = test_thread_session(displayed, app.config.cwd.to_path_buf());
        app.chat_widget.handle_thread_session_quiet(session);
        app.chat_widget
            .apply_external_edit("unfinished conversation".into());
        let draft = app
            .chat_widget
            .capture_thread_input_state()
            .ok_or_else(|| failure("missing draft"))?;
        app.chat_widget.apply_external_edit(String::new());
        let parked_store = app.ensure_thread_channel(parked).store.clone();
        parked_store.lock().await.input_state = Some(draft.clone());

        let (_sender, inbox) = mpsc::unbounded_channel();
        for expected in [false, true] {
            let (reply, admitted) = oneshot::channel();
            let event = AppEvent::AlabAccountsCheckInput {
                thread_id: Some(displayed),
                reply,
            };
            assert!(
                app.alab_admit_upgrade_event(
                    event,
                    &inbox,
                    /*pending_actor_work*/ false,
                    /*pending_startup_draft*/ None,
                    &fixture.backends,
                )
                .is_none()
            );
            assert_eq!(admitted.await.map_err(failure)?, expected);
            if !expected {
                assert_eq!(
                    parked_store.lock().await.input_state.take().as_ref(),
                    Some(&draft)
                );
                assert!(app.chat_widget.composer_text_with_pending().is_empty());
            }
        }
        Ok(())
    }
    .await;
    let cleanup = fixture.close().await;
    body.and(cleanup)
}

#[cfg(unix)]
#[tokio::test]
async fn upgrade_and_account_switch_preserve_unsent_sidebar_openings() -> io::Result<()> {
    use crate::app::side::SideThreadState;
    use crate::app::sidebar::CreationState;
    use crate::app::sidebar::SideCreation;
    use crate::chatwidget::UserMessage;

    let fixture = Fixture::new();
    let body = async {
        let (mut app, _events, _ops) = crate::boxed_future!(make_test_app_with_channels()).await;
        let displayed = ThreadId::new();
        let parent = ThreadId::new();
        let child = ThreadId::new();
        let hidden_owner = fixture.backends.entries[1].session.backend_id();
        app.app_server_backend_id = Some(fixture.backends.displayed_owner());
        let session = test_thread_session(displayed, app.config.cwd.to_path_buf());
        app.chat_widget.handle_thread_session_quiet(session);
        let (_sender, inbox) = mpsc::unbounded_channel();
        let check_admission = |app: &mut App, has_opening: bool| -> io::Result<()> {
            check_input_admission(app, &fixture.backends, has_opening)?;
            let (reply, mut decision) = oneshot::channel();
            let event = AppEvent::AlabAccountsCheckInput {
                thread_id: Some(displayed),
                reply,
            };
            assert!(
                app.alab_admit_upgrade_event(event, &inbox, false, None, &fixture.backends)
                    .is_none()
            );
            assert_eq!(decision.try_recv().map_err(failure)?, !has_opening);
            Ok(())
        };
        let image = UserMessage {
            remote_image_urls: vec!["https://example.com/opening.png".into()],
            .."".into()
        };
        let request = uuid::Uuid::new_v4();
        for state in [
            CreationState::Waiting {
                request,
                started: Instant::now(),
            },
            CreationState::Failed {
                message: "side boundary unavailable".into(),
                cleanup: Some(child),
            },
        ] {
            app.sidebars.entry(Some(parent)).or_default().creation = Some(SideCreation {
                owner: hidden_owner,
                state,
                message: Some(image.clone()),
            });
            check_admission(&mut app, true)?;
            let creation = app
                .sidebars
                .get_mut(&Some(parent))
                .unwrap()
                .creation
                .as_mut()
                .unwrap();
            assert_eq!(creation.message.take().as_ref(), Some(&image));
            check_admission(&mut app, false)?;
        }
        let mut side = SideThreadState::new(parent);
        side.opening_message = Some(image.clone());
        app.side_threads.insert(child, side);
        check_admission(&mut app, true)?;
        let side = app.side_threads.get_mut(&child).unwrap();
        assert_eq!(side.opening_message.take().as_ref(), Some(&image));
        check_admission(&mut app, false)
    }
    .await;
    let cleanup = fixture.close().await;
    body.and(cleanup)
}

#[tokio::test]
async fn ordinary_upgrade_keeps_parked_input_and_ignores_only_its_live_receiver_snapshot()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let body = async {
        let (mut app, _events, _operations) =
            crate::boxed_future!(make_test_app_with_channels()).await;
        let displayed = ThreadId::new();
        let parked = ThreadId::new();
        let first = fixture.backends.displayed_owner();
        let second = fixture.backends.entries[1].session.backend_id();
        app.app_server_backend_id = Some(first);
        let session = test_thread_session(displayed, app.config.cwd.to_path_buf());
        app.chat_widget.handle_thread_session_quiet(session);
        let empty = app
            .chat_widget
            .capture_thread_input_state()
            .ok_or_else(|| failure("missing navigation snapshot"))?;
        app.chat_widget
            .apply_external_edit("keep original parked draft".into());
        let draft = app
            .chat_widget
            .capture_thread_input_state()
            .ok_or_else(|| failure("missing parked draft"))?;
        app.chat_widget.apply_external_edit(String::new());

        let channel = app.ensure_thread_channel(displayed);
        channel.backend_id = Some(first);
        let store = channel.store.clone();
        store.lock().await.input_state = Some(draft.clone());
        app.activate_thread_channel(displayed).await;
        app.agents_overview
            .input_states
            .insert(parked, empty.clone());
        fixture.backends.entries[1]
            .parked_inputs
            .insert(displayed, empty);
        check_input_admission(&mut app, &fixture.backends, false)?;

        // Same thread UUID, separate physical backend. Only its parked map owns this draft.
        fixture.backends.entries[1]
            .parked_inputs
            .insert(displayed, draft.clone());
        check_input_admission(&mut app, &fixture.backends, true)?;
        check(
            fixture.backends.entries[1]
                .parked_inputs
                .remove(&displayed)
                .as_ref()
                == Some(&draft),
            "refusal changed other backend input",
        )?;

        // Parked thread with only a held initial prompt, and no visible composer draft.
        app.chat_widget.initial_user_message = Some("queued startup".into());
        let held = app
            .chat_widget
            .capture_thread_input_state()
            .ok_or_else(|| failure("missing held prompt"))?;
        app.chat_widget.initial_user_message = None;
        app.agents_overview
            .input_states
            .insert(parked, held.clone());
        check_input_admission(&mut app, &fixture.backends, true)?;
        check(
            app.agents_overview.input_states.remove(&parked).as_ref() == Some(&held),
            "refusal changed parked overview input",
        )?;
        let parked_store = app.ensure_thread_channel(parked).store.clone();
        parked_store.lock().await.input_state = Some(held.clone());
        check_input_admission(&mut app, &fixture.backends, true)?;
        check(
            parked_store.lock().await.input_state.take().as_ref() == Some(&held),
            "refusal changed parked channel input",
        )?;

        // A busy store cannot attest empty. Querying it cannot wait under the actor.
        let locked = parked_store.lock().await;
        check_input_admission(&mut app, &fixture.backends, true)?;
        drop(locked);
        check_input_admission(&mut app, &fixture.backends, false)?;

        // A same-ID channel bound to another backend cannot use the widget exemption.
        app.ensure_thread_channel(displayed).backend_id = Some(second);
        check_input_admission(&mut app, &fixture.backends, true)?;
        app.ensure_thread_channel(displayed).backend_id = Some(first);
        check_input_admission(&mut app, &fixture.backends, false)?;
        store.lock().await.active = false;
        check_input_admission(&mut app, &fixture.backends, true)?;
        check(
            store.lock().await.input_state.as_ref() == Some(&draft),
            "observation changed the channel's original draft",
        )
    }
    .await;
    let cleanup = fixture.close().await;
    body.and(cleanup)
}

#[tokio::test]
async fn ordinary_upgrade_keeps_provisional_input_and_requests_and_releases_completed_turns()
-> io::Result<()> {
    let fixture = Fixture::new();
    let body = exercise_provisional_input_and_completed_turn(&fixture).await;
    let cleanup = fixture.close().await;
    body.and(cleanup)
}

async fn exercise_provisional_input_and_completed_turn(fixture: &Fixture) -> io::Result<()> {
    use codex_app_server_protocol::ServerRequestResolvedNotification;
    use codex_app_server_protocol::Turn;
    use codex_app_server_protocol::TurnCompletedNotification;
    use codex_app_server_protocol::TurnItemsView;
    use codex_app_server_protocol::TurnStartedNotification;
    use codex_app_server_protocol::TurnStatus;

    let (mut app, _events, _operations) = crate::boxed_future!(make_test_app_with_channels()).await;
    let thread = ThreadId::new();
    let mut startup = app.chat_widget.composer_draft_snapshot();
    startup
        .startup_local_history
        .push(crate::bottom_pane::HistoryEntry::new(
            "submitted and recalled".into(),
        ));
    check_admission_with_startup_draft(&mut app, &fixture.backends, Some(&startup), false)?;
    startup
        .pending_pastes
        .push(("[paste]".into(), "still in the first edit".into()));
    check_admission_with_startup_draft(&mut app, &fixture.backends, Some(&startup), true)?;
    check(
        startup.pending_pastes[0].1 == "still in the first edit",
        "refusal consumed the pending startup paste",
    )?;
    startup.pending_pastes.clear();
    startup.remote_image_urls.push("image-ref".into());
    check_admission_with_startup_draft(&mut app, &fixture.backends, Some(&startup), true)?;

    let request_id = RequestId::Integer(71);
    let approval = ServerRequest::FileChangeRequestApproval {
        request_id: request_id.clone(),
        params: FileChangeRequestApprovalParams {
            thread_id: thread.to_string(),
            turn_id: "turn-71".into(),
            item_id: "change-71".into(),
            started_at_ms: 0,
            reason: None,
            grant_root: None,
        },
    };
    app.enqueue_primary_thread_request(approval.clone())
        .await
        .map_err(failure)?;
    check_input_admission(&mut app, &fixture.backends, true)?;
    check(
        matches!(app.pending_primary_events.pop_front(), Some(ThreadBufferedEvent::Request(original)) if original.id() == &request_id),
        "unbound primary lost the original request",
    )?;

    let owner = fixture.backends.displayed_owner();
    app.app_server_backend_id = Some(owner);
    app.chat_widget
        .handle_thread_session_quiet(test_thread_session(thread, app.config.cwd.to_path_buf()));
    app.ensure_thread_channel(thread).backend_id = Some(owner);
    app.activate_thread_channel(thread).await;
    let store = app.ensure_thread_channel(thread).store.clone();
    store.lock().await.push_request(approval);
    check_input_admission(&mut app, &fixture.backends, true)?;
    // An indexed unresolved approval survives even after its active channel becomes parked.
    app.store_active_thread_receiver().await;
    check_input_admission(&mut app, &fixture.backends, true)?;
    store
        .lock()
        .await
        .push_notification(ServerNotification::ServerRequestResolved(
            ServerRequestResolvedNotification {
                request_id,
                thread_id: thread.to_string(),
            },
        ));
    check_input_admission(&mut app, &fixture.backends, false)?;

    let sent = app
        .chat_widget
        .submit_user_message_as_plain_user_turn("compile this crate".into())
        .ok_or_else(|| failure("original composer did not submit a user turn"))?;
    let flight = app
        .chat_widget
        .capture_thread_input_state()
        .ok_or_else(|| failure("missing submission state"))?;
    app.agents_overview.input_states.insert(thread, flight);
    check_input_admission(&mut app, &fixture.backends, true)?;
    let crate::app_command::AppCommand::UserTurn {
        client_user_message_id,
        ..
    } = &sent
    else {
        return Err(failure("expected the original user turn command"));
    };
    app.chat_widget.user_message_admitted(
        crate::app_server_session::user_message_submission::UserMessageAdmission::Started(
            "turn-72".into(),
        ),
        &sent,
        client_user_message_id,
    );
    let mut turn = Turn {
        id: "turn-72".into(),
        root_turn_id: None,
        items_view: TurnItemsView::Full,
        items: Vec::new(),
        status: TurnStatus::InProgress,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    };
    app.chat_widget.handle_server_notification(
        ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: thread.to_string(),
            turn: turn.clone(),
        }),
        None,
    );
    turn.status = TurnStatus::Completed;
    app.chat_widget.handle_server_notification(
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: thread.to_string(),
            turn,
        }),
        None,
    );
    let completed = app
        .chat_widget
        .capture_thread_input_state()
        .ok_or_else(|| failure("missing completed state"))?;
    app.agents_overview.input_states.insert(thread, completed);
    check_input_admission(&mut app, &fixture.backends, false)?;
    let (request, _cancel) = UpgradeRequest::new(|ready| ready);
    check(
        crate::app::alab_session::accept_upgrade(&app, request),
        "completed live turn with only a retained retry copy refused its upgrade",
    )
}
