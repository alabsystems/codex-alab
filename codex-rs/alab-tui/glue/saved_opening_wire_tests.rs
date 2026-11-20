// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app_server_session::alab_session_work::saved_chat::SavedChatRelease;
use crate::app_server_session::alab_session_work::saved_chat::SavedChatReleaseOutcome;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadSettingsUpdatedNotification;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;
use color_eyre::eyre::WrapErr;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::oneshot;

#[path = "saved_opening_reconnect.rs"]
mod saved_reconnect;

#[tokio::test]
async fn reconnect_refuses_a_wrong_thread_reply_after_one_resume_request() -> Result<()> {
    use crate::app::reconnect::ReconnectPresentation;

    for presentation in [
        ReconnectPresentation::Conversation,
        ReconnectPresentation::Overview,
    ] {
        crate::boxed_future!(check_wrong_thread_reconnect(presentation)).await?;
    }
    Ok(())
}

async fn check_wrong_thread_reconnect(
    presentation: crate::app::reconnect::ReconnectPresentation,
) -> Result<()> {
    use crate::app::reconnect::reconnect;

    let app = make_test_app().await;
    let (server, _commands) = channel_session();
    let requested = ThreadId::new();
    let expected = response(&app, requested, false)?;
    let foreign = response(&app, ThreadId::new(), false)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let requests = std::cell::Cell::new(0);
    let peer = async {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await?;
            disconnect::serve_reconnect_requests(
                tokio_tungstenite::accept_async(stream).await?,
                |request| {
                    let value = match request.method.as_str() {
                        "thread/resume" => {
                            assert_eq!(
                                request.params.as_ref().unwrap()["threadId"],
                                requested.to_string()
                            );
                            requests.set(requests.get() + 1);
                            assert_eq!(
                                requests.get(),
                                1,
                                "wrong thread ID triggered a second resume"
                            );
                            json!({"result": foreign})
                        }
                        "thread/read" => json!({"result": {"thread": expected.thread}}),
                        method => composer_read(method),
                    };
                    std::future::ready(Some(value))
                },
            )
            .await?;
        }
        Ok::<(), color_eyre::Report>(())
    };
    let attempt = crate::boxed_future!(reconnect(
        server.backend_id(),
        AppServerTarget::Remote { endpoint },
        app.config.clone(),
        app.local_settings.clone(),
        Some(requested),
        None,
        server.thread_tool_transport(),
        presentation,
    ));
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(60), attempt) => {
            assert!(result?.is_err(), "reconnect adopted the foreign thread");
        },
        done = peer => {
            done?;
            bail!("reconnect opened a second transport after the wrong thread reply");
        }
    }
    assert_eq!(requests.get(), 1);
    Ok(())
}

#[tokio::test]
async fn reconnect_reply_for_a_parked_thread_keeps_the_displayed_side_input_separate() -> Result<()>
{
    use crate::app::reconnect::ReconnectPresentation;
    use crate::app::reconnect::reconnect;
    use crate::app::thread_attachment::Selection;
    use crate::app::thread_attachment::SelectionMode;

    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let (mut server, _commands) = channel_session();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let owner = server.backend_id();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    app.app_server_backend_id = Some(owner);
    app.primary_thread_id = Some(parent);
    app.side_threads
        .insert(child, side::SideThreadState::new(parent));
    for id in [parent, child] {
        let session = test_thread_session(id, app.config.cwd.to_path_buf());
        let channel = app.ensure_thread_channel(id);
        channel.backend_id = Some(owner);
        channel.store.lock().await.set_session(session, Vec::new());
    }
    app.activate_thread_channel(parent).await;
    app.chat_widget
        .handle_thread_session(test_thread_session(parent, app.config.cwd.to_path_buf()));
    app.chat_widget
        .apply_external_edit("parked parent draft".into());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    app.app_server_target = AppServerTarget::Remote { endpoint };
    let reply = response(&app, parent, false)?;
    let (issued, saw_request) = oneshot::channel();
    let (send_reply, resume_reply) = oneshot::channel();
    let proxy = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut handoff = Some((issued, resume_reply));
        disconnect::serve_reconnect_requests(
            tokio_tungstenite::accept_async(stream).await?,
            |request| {
                let mut wait = None;
                let value = match request.method.as_str() {
                    "thread/resume" => {
                        assert_eq!(
                            request.params.as_ref().unwrap()["threadId"],
                            parent.to_string()
                        );
                        let (issued, release) = handoff.take().expect("parent resume issued once");
                        issued.send(()).expect("display transition still pending");
                        wait = Some(release);
                        json!({"result": reply})
                    }
                    "thread/read" => json!({"result": {"thread": reply.thread}}),
                    "thread/list" | "thread/loaded/list" => {
                        json!({"result": {"data": [], "nextCursor": null}})
                    }
                    "thread/goal/get" => json!({"result": {"goal": null}}),
                    method => composer_read(method),
                };
                async move {
                    if let Some(release) = wait {
                        release
                            .await
                            .expect("display switched before the reconnect reply");
                    }
                    Some(value)
                }
            },
        )
        .await
    });
    let mut pending = crate::boxed_future!(reconnect(
        owner,
        app.app_server_target.clone(),
        app.config.clone(),
        app.local_settings.clone(),
        Some(parent),
        None,
        server.thread_tool_transport(),
        ReconnectPresentation::Conversation,
    ));
    tokio::select! {
        _ = &mut pending => bail!("reconnect finished before the parked thread's reply"),
        issued = tokio::time::timeout(Duration::from_secs(60), saw_request) => issued??,
    }
    crate::boxed_future!(app.commit_thread_selection(
        &mut tui,
        &mut server,
        child,
        Selection {
            mode: SelectionMode::Live,
            cached_session: None,
            pending_profile_on_unavailable_thread: false,
        }
    ))
    .await?;
    let mut parent_input = app.thread_event_channels[&parent]
        .store
        .lock()
        .await
        .input_state
        .clone()
        .expect("parked parent draft");
    parent_input.reconnect_pending = true;
    app.chat_widget
        .apply_external_edit("displayed side draft".into());
    app.chat_widget.initial_user_message = Some("displayed side initial".into());
    send_reply.send(()).expect("waiting reconnect reply");
    let connected = tokio::time::timeout(Duration::from_secs(60), pending).await??;
    crate::boxed_future!(app.finish_reconnect(
        &mut tui,
        &mut server,
        &mut events,
        connected,
        "2.0.0"
    ))
    .await?;
    assert_eq!(app.current_displayed_thread_id(), Some(child));
    assert_eq!(
        app.thread_event_channels[&child].attachment(),
        ThreadEventAttachment::ReplayOnly
    );
    assert_eq!(
        app.thread_event_channels[&parent].attachment(),
        ThreadEventAttachment::Live
    );
    assert_eq!(
        app.thread_event_channels[&parent]
            .store
            .lock()
            .await
            .input_state,
        Some(parent_input)
    );
    app.chat_widget
        .set_initial_user_message_submit_suppressed(false);
    app.chat_widget.submit_initial_user_message_if_pending();
    assert_eq!(
        app.chat_widget.initial_user_message,
        Some("displayed side initial".into())
    );
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "displayed side draft"
    );
    assert!(take_side_user_turns(&mut events, child).is_empty());
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

// The two backend fixtures support the same read-only inventories used by the
// composer. Other optional reads get the normal response of a limited server.
// Unexpected commands, including unsubscribe on the replacement, fail the test.
fn composer_read(method: &str) -> serde_json::Value {
    match method {
        "plugin/installed" => json!({"result": {"marketplaces": []}}),
        "skills/list" => json!({"result": {"data": []}}),
        "app/list" | "thread/backgroundTerminals/list" => {
            json!({"result": {"data": [], "nextCursor": null}})
        }
        _ if matches!(method.rsplit('/').next(), Some("list" | "read" | "get")) => {
            json!({"error": {"code": -32601, "message": "Method not found"}})
        }
        _ => panic!("unexpected saved-thread command {method}"),
    }
}

#[tokio::test]
async fn saved_chat_release_reports_the_original_wire_reply() -> Result<()> {
    let (replies, outcomes): (Vec<_>, Vec<_>) = [
        (
            Some(json!({"result": {"status": "unsubscribed"}})),
            SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::Unsubscribed),
        ),
        (
            Some(json!({"result": {"status": "notSubscribed"}})),
            SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::NotSubscribed),
        ),
        (
            Some(json!({"result": {"status": "notLoaded"}})),
            SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::NotLoaded),
        ),
        (
            Some(json!({"error": {"code": -32602, "message": "invalid thread"}})),
            SavedChatReleaseOutcome::Rejected { code: -32602 },
        ),
        (
            Some(json!({"result": {"status": "unrecognized"}})),
            SavedChatReleaseOutcome::InvalidReply,
        ),
        (None, SavedChatReleaseOutcome::TransportError),
    ]
    .into_iter()
    .unzip();
    let target = sidebar_loading::Target {
        backend_id: crate::backend_id::BackendId::new(),
        parent: ThreadId::new(),
        child: ThreadId::new(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let (wire, mut requests) = mpsc::unbounded_channel();
    let proxy = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut replies = replies.into_iter();
        disconnect::serve_reconnect_requests(
            tokio_tungstenite::accept_async(stream).await?,
            |req| {
                assert_eq!(req.method, "thread/unsubscribe");
                assert_eq!(
                    req.params.unwrap()["threadId"],
                    json!(target.child.to_string())
                );
                wire.send(req.id).unwrap();
                std::future::ready(replies.next().expect("unexpected unsubscribe"))
            },
        )
        .await
    });
    let original = AppServerSession::new(
        target.backend_id,
        crate::connect_remote_app_server(endpoint).await?,
        crate::app_server_session::ThreadParamsMode::Remote,
    );
    let (sender, mut events) = mpsc::unbounded_channel();
    let sender = AppEventSender::new(sender);
    for outcome in outcomes {
        let operation = Uuid::new_v4();
        SavedChatRelease::reserve(&original, target, operation).publish(sender.clone());
        let event = tokio::time::timeout(Duration::from_secs(60), events.recv())
            .await?
            .expect("original release completion");
        let AppEvent::SavedChatReleased {
            target: actual,
            request,
            receipt,
        } = event
        else {
            bail!("expected saved-chat release completion");
        };
        assert_eq!((actual, request), (target, operation));
        let wire_request = RequestId::String(format!("saved-chat-release-{operation}"));
        assert_eq!(requests.try_recv()?, wire_request);
        assert_eq!(receipt.wire_request, wire_request);
        assert_eq!(receipt.outcome, outcome);
    }
    original.shutdown().await?;
    proxy.await??;
    assert!(requests.try_recv().is_err());
    assert!(events.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn held_saved_resume_installs_or_unsubscribes_once_on_its_original_connection() -> Result<()>
{
    #[cfg(unix)]
    {
        use std::io::IsTerminal;
        use std::os::fd::FromRawFd;
        const CHILD: &str = "ALAB_SAVED_OPENING_PTY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // The install case renders a protected modal and drains crossterm's process-wide
            // input reader. Give only this test process a fresh terminal, even under headless CI.
            let (mut master, mut slave) = (-1, -1);
            // SAFETY: openpty initializes both descriptors on success. The other outputs are optional.
            if unsafe {
                libc::openpty(
                    &raw mut master,
                    &raw mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: openpty gave us two new owned descriptors.
            let (_master, input) = unsafe {
                (
                    std::fs::File::from_raw_fd(master),
                    std::fs::File::from_raw_fd(slave),
                )
            };
            for fd in [master, slave] {
                // SAFETY: the files above keep these descriptors open until the child exits.
                if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            let module = module_path!().split_once("::").unwrap().1;
            let test = format!(
                "{module}::held_saved_resume_installs_or_unsubscribes_once_on_its_original_connection"
            );
            let output = tokio::time::timeout(
                Duration::from_secs(130),
                tokio::process::Command::new(std::env::current_exe()?)
                    .args(["--exact", &test, "--show-output", "--color", "never"])
                    .env(CHILD, "1")
                    .stdin(input)
                    .kill_on_drop(true)
                    .output(),
            )
            .await??;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success() && stdout.lines().any(|line| line.trim() == test),
                "exact saved-opening PTY test did not pass: {stdout}\n{stderr}"
            );
            return Ok(());
        }
        assert!(std::io::stdin().is_terminal());
    }
    for scenario in [SavedResume::Install, SavedResume::Reconnect] {
        tokio::time::timeout(
            Duration::from_secs(60),
            crate::boxed_future!(held_saved_resume(scenario)),
        )
        .await?
        .wrap_err(format!("held saved resume {scenario:?}"))?;
    }
    Ok(())
}

#[tokio::test]
async fn malformed_saved_resume_releases_original_subscription_after_replacement_attaches()
-> Result<()> {
    for scenario in [
        SavedResume::MalformedAfterReconnect,
        SavedResume::MalformedLegacyRetry,
    ] {
        tokio::time::timeout(
            Duration::from_secs(60),
            crate::boxed_future!(held_saved_resume(scenario)),
        )
        .await?
        .wrap_err(format!("saved-thread malformed reply {scenario:?}"))?;
    }
    Ok(())
}

#[tokio::test]
async fn wrong_thread_resume_reply_releases_only_the_original_requested_subscription() -> Result<()>
{
    tokio::time::timeout(
        Duration::from_secs(60),
        crate::boxed_future!(held_saved_resume(SavedResume::WrongThreadLegacyRetry)),
    )
    .await??;
    Ok(())
}

#[tokio::test]
async fn refresh_removing_a_saved_child_preserves_the_parent_and_releases_the_original()
-> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(60),
        crate::boxed_future!(held_saved_resume(SavedResume::RemovedByRefresh)),
    )
    .await??;
    Ok(())
}

#[tokio::test]
async fn refresh_reordering_a_saved_child_keeps_its_original_live_opening() -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(60),
        crate::boxed_future!(held_saved_resume(SavedResume::ReorderedByRefresh)),
    )
    .await??;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SavedResume {
    Install,
    RemovedByRefresh,
    ReorderedByRefresh,
    Reconnect,
    MalformedAfterReconnect,
    MalformedLegacyRetry,
    WrongThreadLegacyRetry,
}

async fn held_saved_resume(scenario: SavedResume) -> Result<()> {
    let wrong_thread = scenario == SavedResume::WrongThreadLegacyRetry;
    let legacy_retry = wrong_thread || scenario == SavedResume::MalformedLegacyRetry;
    let malformed = matches!(
        scenario,
        SavedResume::MalformedAfterReconnect | SavedResume::MalformedLegacyRetry
    );
    let reconnect = matches!(scenario, SavedResume::Reconnect) || malformed || wrong_thread;
    let removed_by_refresh = scenario == SavedResume::RemovedByRefresh;
    let refresh_peer =
        (removed_by_refresh || scenario == SavedResume::ReorderedByRefresh).then(ThreadId::new);
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let parent = ThreadId::new();
    let child = ThreadId::new();
    app.chat_widget
        .handle_thread_session(test_thread_session(parent, app.config.cwd.to_path_buf()));
    app.active_thread_id = Some(parent);
    app.primary_thread_id = Some(parent);
    enable_side_retention(&mut app)?;
    app.sidebar_mut().show(
        sidebar::SidebarContent::Chooser,
        sidebar::PaneFocus::Sidebar,
    );
    let mut choices = vec![child.into()];
    if let Some(peer) = refresh_peer {
        choices.push(peer.into());
        app.chat_widget
            .apply_external_edit("unfinished parent draft".into());
    }
    app.sidebar_mut().update_saved_chats(choices);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    while events.try_recv().is_ok() {}

    let listener = Arc::new(tokio::net::TcpListener::bind("127.0.0.1:0").await?);
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let reply = response(&app, child, false)?;
    let mut resumed = reply.clone();
    if wrong_thread {
        resumed.thread.id = ThreadId::new().to_string();
        resumed.thread.history_mode = codex_app_server_protocol::ThreadHistoryMode::Paginated;
        resumed.turns_backwards_cursor = Some("foreign-turn-page".into());
    }
    let (received, pending_resume) = oneshot::channel();
    let mut received = Some(received);
    let first_listener = listener.clone();
    let proxy = tokio::spawn(async move {
        let mut unsubscribe_request = None;
        let mut legacy_params = None;
        let (stream, _) = first_listener.accept().await?;
        let socket = tokio_tungstenite::accept_async(stream).await?;
        let methods = disconnect::serve_reconnect_requests(socket, |request| {
            let (response, wait) = match request.method.as_str() {
                "thread/read" => (json!({"result": {"thread": reply.thread}}), None),
                "thread/resume" if legacy_retry && legacy_params.is_none() => {
                    let mut parameters = request.params.unwrap();
                    assert_eq!(parameters["threadId"], child.to_string());
                    assert_eq!(
                        parameters.as_object_mut().unwrap().remove("excludeTurns"),
                        Some(json!(true))
                    );
                    legacy_params = Some((request.id, parameters));
                    (
                        json!({"error":{"code":-32602,"message":"unknown field excludeTurns"}}),
                        None,
                    )
                }
                "thread/resume" => {
                    assert_eq!(
                        request.params.as_ref().unwrap()["threadId"],
                        child.to_string()
                    );
                    if let Some((rejected_id, parameters)) = &legacy_params {
                        assert_ne!(&request.id, rejected_id);
                        assert_eq!(request.params.as_ref().unwrap(), parameters);
                    }
                    let (release, wait) = oneshot::channel();
                    received
                        .take()
                        .expect("duplicate saved thread/resume")
                        .send(release)
                        .unwrap();
                    let mut result = json!(resumed);
                    if malformed {
                        result["model"] = json!({"unexpected":"object"});
                    }
                    (json!({"result": result}), Some(wait))
                }
                "thread/unsubscribe" => {
                    assert!(unsubscribe_request.replace(request.id).is_none());
                    assert_eq!(
                        request.params.as_ref().unwrap()["threadId"],
                        child.to_string()
                    );
                    (
                        json!({"result": ThreadUnsubscribeResponse {
                            status: ThreadUnsubscribeStatus::Unsubscribed,
                        }}),
                        None,
                    )
                }
                method => (composer_read(method), None),
            };
            async move {
                if let Some(wait) = wait {
                    wait.await.expect("release the original reply");
                }
                Some(response)
            }
        })
        .await?;
        Ok::<_, color_eyre::Report>((methods, unsubscribe_request))
    });
    let backend_id = crate::backend_id::BackendId::new();
    app.app_server_target = AppServerTarget::Remote {
        endpoint: endpoint.clone(),
    };
    let mut server = AppServerSession::new(
        backend_id,
        crate::connect_remote_app_server(endpoint).await?,
        crate::app_server_session::ThreadParamsMode::Remote,
    );
    let target = sidebar_loading::Target {
        backend_id,
        parent,
        child,
    };
    let before = server
        .alab_capture_original_session_jobs()?
        .observe()
        .await?;
    before.current(&server)?;
    app.open_saved_chat(&mut tui, &mut server, target).await?;
    let release = pending_resume.await?;
    assert!(before.current(&server).is_err());
    let cut = server.alab_capture_original_session_jobs()?;
    {
        let mut completion = std::pin::pin!(cut.observe());
        assert!(futures::poll!(&mut completion).is_pending());
    }

    // Selecting the same saved child while the backend holds its response shares the opening.
    app.open_saved_chat(&mut tui, &mut server, target).await?;
    let original_store = app.thread_event_channels[&child].store.clone();
    if let Some(peer) = refresh_peer {
        let request = uuid::Uuid::new_v4();
        app.sidebar_mut()
            .set_discovery(crate::sidebar_chooser::Discovery::Loading {
                request,
                started: std::time::Instant::now(),
                retry: false,
            });
        let mut choices = vec![peer.into()];
        if !removed_by_refresh {
            choices.push(child.into());
        }
        let refresh = AppEvent::SavedSideChatsLoaded {
            backend_id,
            parent,
            request,
            result: Ok(crate::app_event::SavedSideChatData::Choices(choices)),
        };
        crate::boxed_future!(app.handle_event(&mut tui, &mut server, refresh)).await?;
        let tabs = app.sidebar().unwrap().chat_tabs();
        assert_eq!(tabs.choice(1), Some(peer));
        assert_eq!(tabs.choice(2), (!removed_by_refresh).then_some(child));
    } else if reconnect {
        app.handle_app_server_event(
            &server,
            AppServerEvent::ServerNotification(Box::new(turn_started_notification(
                child,
                saved_reconnect::BUFFERED_TURN,
            ))),
        )
        .await;
        assert!(app.begin_reconnect());
        assert!(app.sidebar().unwrap().opening.is_none());
    } else {
        app.config.features.enable(Feature::MisalignmentReview)?;
        let updated = ThreadSettingsUpdatedNotification {
            thread_id: child.to_string(),
            thread_settings: current_settings(&app),
        };
        app.handle_app_server_event(
            &server,
            AppServerEvent::ServerNotification(Box::new(
                ServerNotification::ThreadSettingsUpdated(updated),
            )),
        )
        .await;
        // The held resume contains only "snapshot". This failure arrived without a turn/start.
        app.handle_app_server_event(
            &server,
            AppServerEvent::ServerNotification(Box::new(ServerNotification::Error(
                codex_app_server_protocol::ErrorNotification {
                    thread_id: child.to_string(),
                    turn_id: "live-refusal".into(),
                    will_retry: false,
                    error: AppServerTurnError {
                        message: "Chat stopped".into(),
                        codex_error_info: Some(
                            AppServerCodexErrorInfo::MisalignmentPolicyViolation,
                        ),
                        additional_details: None,
                        misalignment: None,
                    },
                },
            ))),
        )
        .await;
        drain_app_events(&mut app, &mut tui, &mut server, &mut events).await?;
        assert!(
            app.can_read_terminal_after_thread_switch(&tui, &events),
            "the chooser must remain interactive while the backend holds its reply"
        );
    }
    let delayed_reply = if malformed || wrong_thread {
        Some(release)
    } else {
        release.send(()).unwrap();
        cut.observe().await?.current(&server)?;
        None
    };
    let replacement_proxy = if reconnect {
        Some(
            crate::boxed_future!(saved_reconnect::finish_and_reopen(
                &mut app,
                &mut tui,
                &mut server,
                &mut events,
                target,
                listener,
                saved_reconnect::OriginalOpening {
                    store: original_store,
                    delayed_reply,
                    wrong_thread,
                },
            ))
            .await?,
        )
    } else if refresh_peer.is_some() {
        let prepared =
            saved_reconnect::next_saved_event(&mut app, &mut tui, &mut server, &mut events).await?;
        let AppEvent::SavedChatPrepared {
            target: actual,
            request,
            result,
        } = &prepared
        else {
            bail!("original resume did not publish its successful preparation");
        };
        assert_eq!(*actual, target);
        assert!(matches!(&result.result, Ok(PreparedAttachment::Live(_))));
        let original = *request;
        crate::boxed_future!(app.handle_event(&mut tui, &mut server, prepared)).await?;
        if removed_by_refresh {
            assert_eq!(app.current_displayed_thread_id(), Some(parent));
            assert_eq!(
                app.chat_widget.composer_text_with_pending(),
                "unfinished parent draft"
            );
            assert!(app.sidebar().unwrap().opening.is_none());
            assert!(!app.thread_event_channels.contains_key(&child));
            let released =
                saved_reconnect::next_saved_event(&mut app, &mut tui, &mut server, &mut events)
                    .await?;
            let AppEvent::SavedChatReleased {
                target: actual,
                request,
                receipt,
            } = &released
            else {
                bail!("abandoned original resume did not publish its release");
            };
            assert_eq!((*actual, *request), (target, original));
            assert_eq!(
                receipt.wire_request,
                RequestId::String(format!("saved-chat-release-{original}"))
            );
            assert_eq!(
                receipt.outcome,
                SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::Unsubscribed)
            );
            crate::boxed_future!(app.handle_event(&mut tui, &mut server, released)).await?;
        } else {
            assert_eq!(app.current_displayed_thread_id(), Some(child));
            let channel = &app.thread_event_channels[&child];
            assert_eq!(channel.attachment(), ThreadEventAttachment::Live);
            assert!(Arc::ptr_eq(&original_store, &channel.store));
        }
        None
    } else {
        assert!(events.is_empty());
        assert!(
            !app.can_read_terminal_after_thread_switch(&tui, &events),
            "a published saved-thread reply must precede input for the old view"
        );
        assert!(!app.chat_widget.has_active_view());
        let old_frame = tui.terminal.previous_buffer().clone();
        app.render_startup_frame(&mut tui, &events)?;
        assert_eq!(tui.terminal.previous_buffer(), &old_frame);
        let prepared = loop {
            let event = app.saved_openings.next_event(&mut events).await.unwrap();
            if let AppEvent::SavedChatPrepared { target: actual, .. } = &event {
                assert_eq!(*actual, target);
                break event;
            }
            crate::boxed_future!(app.handle_event(&mut tui, &mut server, event)).await?;
        };
        crate::boxed_future!(app.handle_event(&mut tui, &mut server, prepared)).await?;
        assert_eq!(app.current_displayed_thread_id(), Some(child));
        drain_app_events(&mut app, &mut tui, &mut server, &mut events).await?;
        assert!(app.can_read_terminal_after_thread_switch(&tui, &events));
        app.render_startup_frame(&mut tui, &events)?;
        assert_ne!(tui.terminal.previous_buffer(), &old_frame);
        let channel = &app.thread_event_channels[&child];
        assert!(Arc::ptr_eq(&channel.store, &original_store));
        assert_eq!(channel.attachment(), ThreadEventAttachment::Live);
        let store = channel.store.lock().await;
        assert_eq!(store.session.as_ref().unwrap().model, "newer-live-model");
        assert_eq!(store.turns[0].id, "snapshot");
        assert_eq!(store.latest_turn_id.as_deref(), Some("live-refusal"));
        assert!(app.chat_widget.has_misalignment_policy_violation());
        None
    };
    // Inventory workers retain the actor sender until their last RPC reply is
    // consumed. Retire the UI first and keep its home and transport for them.
    let _home = app._test_codex_home.take();
    drop(app);
    drop(tui);
    while events.recv().await.is_some() {}
    let final_read = server
        .thread_read(child, false)
        .await
        .wrap_err(if reconnect {
            "read from the replacement after its inventory settled"
        } else {
            "read from the original after its inventory settled"
        })?;
    assert_eq!(final_read.id, child.to_string());
    server
        .shutdown()
        .await
        .wrap_err("shut down displayed saved-thread connection")?;
    let (methods, unsubscribe_request) = proxy
        .await
        .wrap_err("join original saved-thread peer")?
        .wrap_err("original saved-thread peer responses")?;
    if let Some((replacement_proxy, operation)) = replacement_proxy {
        assert_eq!(
            unsubscribe_request,
            Some(RequestId::String(format!("saved-chat-release-{operation}")))
        );
        replacement_proxy
            .await
            .wrap_err("join replacement saved-thread peer")?
            .wrap_err("replacement saved-thread peer responses")?;
    }
    assert_eq!(
        methods.iter().filter(|m| *m == "thread/resume").count(),
        if legacy_retry { 2 } else { 1 }
    );
    assert_eq!(
        methods
            .iter()
            .filter(|m| *m == "thread/unsubscribe")
            .count(),
        usize::from(reconnect || removed_by_refresh)
    );
    if reconnect {
        let unsubscribe = methods
            .iter()
            .position(|m| m == "thread/unsubscribe")
            .unwrap();
        if malformed || wrong_thread {
            let resume = methods.iter().rposition(|m| m == "thread/resume").unwrap();
            assert!(
                !methods[resume + 1..unsubscribe].iter().any(|m| matches!(
                    m.as_str(),
                    "thread/read" | "thread/turns/list" | "thread/items/list"
                )),
                "an unusable success cannot fetch or page another conversation"
            );
        }
        assert!(
            methods[unsubscribe + 1..]
                .iter()
                .any(|m| m == "thread/read")
        );
    }
    Ok(())
}
