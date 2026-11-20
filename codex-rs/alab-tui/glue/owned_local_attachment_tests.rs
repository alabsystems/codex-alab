// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ordinary actual App/typed Cloud command-channel controls. No Native origin,
//! constructor, Accepted Scope, retained receipt or positive grant is created.
use super::*;
use crate::app::alab_attachment;
use crate::app::thread_attachment::ChannelMode;
use crate::app::thread_attachment::CloseState;
use crate::app::thread_attachment::PreparedAttachment;
use crate::app::thread_attachment::SubscriptionUpdates;
use crate::app_server_session::ResumeModelSettings;
use codex_app_server_client::CloudCommand;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadReadState;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSettings;
use codex_app_server_protocol::ThreadUnreadPosition;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[path = "saved_opening_wire_tests.rs"]
mod saved_opening_wire_tests;

#[path = "thread_read_identity_tests.rs"]
mod thread_read_identity_tests;

fn take_side_user_turns(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    side: ThreadId,
) -> Vec<String> {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| {
            let items = match event {
                AppEvent::CodexOp(Op::UserTurn { items, .. }) => items,
                AppEvent::SubmitThreadOp {
                    thread_id,
                    op: Op::UserTurn { items, .. },
                } => {
                    assert_eq!(thread_id, side);
                    items
                }
                _ => return None,
            };
            let [UserInput::Text { text, .. }] = items.as_slice() else {
                panic!("unexpected input in the saved side turn: {items:?}");
            };
            Some(text.clone())
        })
        .collect()
}

#[tokio::test]
async fn saved_side_replay_holds_initial_input_until_a_writable_attachment() -> Result<()> {
    use crate::app::thread_attachment::Selection;
    use crate::app::thread_attachment::SelectionMode;

    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let (mut server, mut commands) = channel_session();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let target = sidebar_loading::Target {
        backend_id: server.backend_id(),
        parent,
        child,
    };
    app.primary_thread_id = Some(parent);
    app.side_threads
        .insert(child, side::SideThreadState::new(parent));
    for thread in [parent, child] {
        let session = test_thread_session(thread, app.config.cwd.to_path_buf());
        app.ensure_thread_channel(thread)
            .store
            .lock()
            .await
            .set_session(session, Vec::new());
    }
    app.activate_thread_channel(child).await;
    app.chat_widget
        .handle_thread_session(test_thread_session(child, app.config.cwd.to_path_buf()));
    app.chat_widget
        .handle_server_notification(turn_started_notification(child, "previous"), None);
    app.chat_widget
        .apply_external_edit("queued follow-up".into());
    app.chat_widget.handle_key_event(KeyCode::Tab.into());
    app.chat_widget.set_queue_autosend_suppressed(true);
    app.chat_widget.handle_server_notification(
        turn_completed_notification(child, "previous", TurnStatus::Completed),
        None,
    );
    app.chat_widget.apply_external_edit("side draft".into());
    app.chat_widget.initial_user_message = Some("initial side input".into());
    assert_eq!(
        app.chat_widget.queued_user_message_texts(),
        ["queued follow-up"]
    );

    let select_parent = || Selection {
        mode: SelectionMode::Live,
        cached_session: None,
        pending_profile_on_unavailable_thread: false,
    };
    crate::boxed_future!(app.commit_thread_selection(
        &mut tui,
        &mut server,
        parent,
        select_parent()
    ))
    .await?;
    let read_only = response(&app, child, true)?.thread;
    let updates = SubscriptionUpdates::default();
    let metadata = server
        .attachment_view(child)
        .take_attachment_metadata(child);
    crate::boxed_future!(app.install_saved_chat(
        &mut tui,
        &mut server,
        target,
        PreparedAttachment::ReplayOnly(Box::new(read_only)),
        metadata,
        &updates,
    ))
    .await?;
    assert_eq!(app.current_displayed_thread_id(), Some(child));
    assert_eq!(
        app.thread_event_channels[&child].attachment(),
        ThreadEventAttachment::ReplayOnly
    );
    app.chat_widget.submit_initial_user_message_if_pending();
    assert!(!app.chat_widget.maybe_send_next_queued_input());
    assert_eq!(
        app.chat_widget.initial_user_message,
        Some("initial side input".into())
    );
    assert_eq!(app.chat_widget.composer_text_with_pending(), "side draft");
    assert_eq!(
        app.chat_widget.queued_user_message_texts(),
        ["queued follow-up"]
    );
    assert!(take_side_user_turns(&mut events, child).is_empty());

    crate::boxed_future!(app.commit_thread_selection(
        &mut tui,
        &mut server,
        parent,
        select_parent()
    ))
    .await?;
    let reply = response(&app, child, false)?;
    let (prepared, served) = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::join!(
            crate::app::thread_attachment::prepare_saved(
                server.attachment_view(child),
                &app.local_settings,
                &app.config,
                child
            ),
            serve_original_resume(&mut commands, reply, 2),
        )
    })
    .await?;
    served?;
    let attachment = prepared.result.map_err(|error| eyre!(error))?;
    crate::boxed_future!(app.install_saved_chat(
        &mut tui,
        &mut server,
        target,
        attachment,
        prepared.metadata,
        &updates,
    ))
    .await?;
    assert_eq!(
        app.thread_event_channels[&child].attachment(),
        ThreadEventAttachment::Live
    );
    assert_eq!(app.current_displayed_thread_id(), Some(child));
    app.chat_widget.submit_initial_user_message_if_pending();
    assert!(app.chat_widget.initial_user_message.is_none());
    assert_eq!(
        take_side_user_turns(&mut events, child),
        ["initial side input"]
    );
    assert_eq!(app.chat_widget.composer_text_with_pending(), "side draft");
    assert_eq!(
        app.chat_widget.queued_user_message_texts(),
        ["queued follow-up"]
    );
    app.chat_widget
        .handle_server_notification(turn_started_notification(child, "initial"), None);
    app.chat_widget.handle_server_notification(
        turn_completed_notification(child, "initial", TurnStatus::Completed),
        None,
    );
    // Replay-only recovery leaves this follow-up for the original edit-and-submit action.
    assert!(!app.chat_widget.maybe_send_next_queued_input());
    assert!(take_side_user_turns(&mut events, child).is_empty());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "queued follow-up"
    );
    app.chat_widget.handle_key_event(KeyCode::Enter.into());
    assert!(app.chat_widget.queued_user_message_texts().is_empty());
    assert_eq!(
        take_side_user_turns(&mut events, child),
        ["queued follow-up"]
    );
    Ok(())
}

#[tokio::test]
async fn saved_side_replay_keeps_confirmation_through_unrelated_wakes() -> Result<()> {
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let (server, _commands) = channel_session();
    let owner = server.backend_id();
    let side = ThreadId::new();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.chat_widget
        .apply_external_edit("confirmed side draft".into());
    let mut draft = Some(app.chat_widget.composer_draft_snapshot());
    app.chat_widget.apply_external_edit(String::new());
    app.chat_widget.set_queue_autosend_suppressed(true);
    let mut confirmed = true;
    app.chat_widget
        .restore_startup_input_when_ready(&mut draft, &mut confirmed);
    assert!(draft.is_none() && !confirmed);
    let input = app.chat_widget.capture_thread_input_state();
    let session = test_thread_session(side, app.config.cwd.to_path_buf());
    let channel = app.ensure_thread_channel(side);
    channel.backend_id = Some(owner);
    let snapshot = {
        let mut store = channel.store.lock().await;
        store.set_session(session, Vec::new());
        store.input_state = input;
        store.snapshot()
    };
    app.app_server_backend_id = Some(owner);
    app.activate_thread_channel(side).await;
    app.render_thread_snapshot(&mut tui, &server, side, snapshot, false)?;
    app.chat_widget.set_queue_autosend_suppressed(false);
    app.chat_widget
        .set_initial_user_message_submit_suppressed(false);
    for resumed in [
        None,
        Some((owner, ThreadId::new())),
        Some((crate::backend_id::BackendId::new(), side)),
    ] {
        app.alab_release_reconnected_input(Some(side), resumed);
        app.chat_widget
            .restore_startup_input_when_ready(&mut draft, &mut confirmed);
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "confirmed side draft"
        );
        assert!(take_side_user_turns(&mut events, side).is_empty());
    }
    app.alab_release_reconnected_input(Some(side), Some((owner, side)));
    app.chat_widget
        .restore_startup_input_when_ready(&mut draft, &mut confirmed);
    app.chat_widget
        .restore_startup_input_when_ready(&mut draft, &mut confirmed);
    assert_eq!(
        take_side_user_turns(&mut events, side),
        ["confirmed side draft"]
    );
    Ok(())
}

fn channel_session() -> (AppServerSession, tokio::sync::mpsc::Receiver<CloudCommand>) {
    let (client, commands, _events) =
        codex_app_server_client::CloudClient::channel(/*capacity*/ 1);
    let session = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        codex_app_server_client::AppServerClient::Cloud(client),
        crate::ThreadParamsMode::Embedded,
    );
    (session, commands)
}

fn response(app: &App, id: ThreadId, cold: bool) -> Result<ThreadResumeResponse> {
    let thread = serde_json::from_value(serde_json::json!({
        "id":id.to_string(), "sessionId":ThreadId::new().to_string(),
        "preview":"original saved response", "ephemeral":false,
        "modelProvider":"response-provider", "createdAt":1, "updatedAt":2,
        "status":if cold { serde_json::json!({"type":"notLoaded"}) } else { serde_json::json!({"type":"idle"}) },
        "cwd":app.config.cwd, "cliVersion":"test", "source":"cli",
        "historyMode":"legacy", "turns":[test_turn("snapshot", TurnStatus::Completed, Vec::new())]
    }))?;
    Ok(ThreadResumeResponse {
        thread,
        model: "snapshot-model".into(),
        model_provider: "response-provider".into(),
        service_tier: Some("snapshot-tier".into()),
        disabled_plugin_ids: Vec::new(),
        cwd: app.config.cwd.clone(),
        runtime_workspace_roots: vec![app.config.cwd.clone()],
        instruction_sources: Vec::new(),
        approval_policy: codex_app_server_protocol::AskForApproval::Never,
        approvals_reviewer: codex_app_server_protocol::ApprovalsReviewer::User,
        sandbox: codex_app_server_protocol::SandboxPolicy::ReadOnly {
            network_access: false,
        },
        active_permission_profile: None,
        reasoning_effort: None,
        collaboration_mode: None,
        multi_agent_mode: Default::default(),
        initial_turns_page: None,
        turns_backwards_cursor: None,
        items_backwards_cursor: None,
    })
}

async fn serve_original_resume(
    commands: &mut tokio::sync::mpsc::Receiver<CloudCommand>,
    mut response: ThreadResumeResponse,
    expected_reads: usize,
) -> Result<ThreadResumeParams> {
    let mut reads = 0;
    let mut unsupported = None;
    loop {
        let command = commands
            .recv()
            .await
            .ok_or_else(|| eyre!("original request missing"))?;
        let CloudOperation::Request(request) = command.operation else {
            bail!("attachment changed its original request transport");
        };
        let (value, resumed) = match request.as_ref() {
            codex_app_server_protocol::ClientRequest::ThreadRead { params, .. } => {
                assert_eq!(params.thread_id, response.thread.id);
                assert!(!params.include_turns);
                reads += 1;
                assert!(reads <= expected_reads);
                (
                    serde_json::to_value(ThreadReadResponse {
                        thread: response.thread.clone(),
                        read_state: Some(ThreadReadState {
                            first_unread: Some(ThreadUnreadPosition::ThreadStart),
                            revision: "snapshot-read-revision".into(),
                        }),
                    })?,
                    None,
                )
            }
            codex_app_server_protocol::ClientRequest::ThreadResume { params, .. } => {
                assert_eq!(reads, expected_reads);
                assert_eq!(params.thread_id, response.thread.id);
                if params.exclude_turns {
                    assert!(unsupported.is_none(), "original history fallback repeated");
                    unsupported = Some(params.clone());
                    // Ordinary old-server wire reply exercises the ORIGINAL
                    // full-policy pagination fallback. No Native/Core claim.
                    command
                        .response
                        .send(Ok(Err(codex_app_server_protocol::JSONRPCErrorError {
                            code: -32602,
                            message: "unknown field excludeTurns".into(),
                            data: None,
                        })))
                        .map_err(|_| eyre!("original fallback observer disappeared"))?;
                    continue;
                }
                let mut expected = unsupported
                    .take()
                    .ok_or_else(|| eyre!("original history fallback missing"))?;
                expected.exclude_turns = false;
                assert_eq!(
                    params, &expected,
                    "fallback changed complete original policy"
                );
                response.thread.status = codex_app_server_protocol::ThreadStatus::Idle;
                (serde_json::to_value(&response)?, Some(params.clone()))
            }
            _ => bail!(
                "unexpected original attachment method {}",
                request.method_name()
            ),
        };
        command
            .response
            .send(Ok(Ok(value)))
            .map_err(|_| eyre!("original response observer disappeared"))?;
        if let Some(params) = resumed {
            return Ok(params);
        }
    }
}

#[tokio::test]
async fn saved_loaded_and_cold_preparation_preserve_original_rpc_policy_and_receipt() -> Result<()>
{
    for cold in [false, true] {
        let mut app = make_test_app().await;
        app.config.developer_instructions = Some("original developer seed".into());
        let child = ThreadId::new();
        let reply = response(&app, child, cold)?;
        let (mut session, mut commands) = channel_session();
        let view = session.attachment_view(child);
        let (prepared, params) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                super::super::thread_attachment::prepare_saved(
                    view,
                    &app.local_settings,
                    &app.config,
                    child
                ),
                serve_original_resume(&mut commands, reply, 2),
            )
        })
        .await?;
        let params = params?;
        assert!(!params.exclude_turns);
        assert!(params.model.is_none());
        assert!(params.model_provider.is_none());
        assert!(params.approval_policy.is_none());
        assert!(params.runtime_workspace_roots.is_none());
        if cold {
            let instructions = params
                .developer_instructions
                .ok_or_else(|| eyre!("cold side policy missing"))?;
            let original = codex_context_fragments::SideConversationPolicy::Attached
                .instructions(Some("original developer seed"));
            assert!(instructions.contains(&original));
            assert_eq!(instructions.matches("original developer seed").count(), 1);
        } else {
            assert!(params.developer_instructions.is_none());
        }
        assert!(prepared.metadata.subscribed);
        // Apply the real opaque metadata through its original consumer. The
        // public session state/query observes its behavior without field access.
        session.install_attachment_metadata(child, prepared.metadata, false);
        assert_eq!(
            session.unread_revisions.get(&child).map(String::as_str),
            Some("snapshot-read-revision")
        );
        assert!(!session.supports_paginated_history());
        let PreparedAttachment::Live(started) = prepared.result.map_err(|error| eyre!(error))?
        else {
            bail!("ordinary successful attachment changed its result kind");
        };
        assert_eq!(started.session.thread_id, child);
        assert_eq!(started.session.model, "snapshot-model");
        assert_eq!(started.turns[0].id, "snapshot");
        #[cfg(unix)]
        assert!(started.alab_construction.is_none());
        assert!(commands.try_recv().is_err());
    }
    Ok(())
}

fn current_settings(app: &App) -> ThreadSettings {
    ThreadSettings {
        disabled_plugin_ids: Vec::new(),
        environments: None,
        cwd: app.config.cwd.clone(),
        approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
        approvals_reviewer: codex_app_server_protocol::ApprovalsReviewer::AutoReview,
        sandbox_policy: codex_app_server_protocol::SandboxPolicy::ReadOnly {
            network_access: false,
        },
        active_permission_profile: None,
        model: "newer-live-model".into(),
        model_provider: "newer-live-provider".into(),
        service_tier: Some("newer-live-tier".into()),
        effort: None,
        summary: None,
        collaboration_mode: codex_protocol::config_types::CollaborationMode {
            mode: codex_protocol::config_types::ModeKind::Default,
            settings: codex_protocol::config_types::Settings {
                model: "newer-live-model".into(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        },
        multi_agent_mode: Default::default(),
        personality: None,
    }
}

#[tokio::test]
async fn original_reused_channel_keeps_newer_settings_turn_and_read_receipt() -> Result<()> {
    for closed in [false, true] {
        let mut app = make_test_app().await;
        let parent = ThreadId::new();
        let child = ThreadId::new();
        app.primary_thread_id = Some(parent);
        app.side_threads
            .insert(child, side::SideThreadState::new(parent));
        let (mut server, mut commands) = channel_session();
        let reply = response(&app, child, false)?;
        let (prepared, served) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                super::super::thread_attachment::prepare_saved(
                    server.attachment_view(child),
                    &app.local_settings,
                    &app.config,
                    child
                ),
                serve_original_resume(&mut commands, reply, 2),
            )
        })
        .await?;
        served?;
        let attachment = prepared.result.map_err(|error| eyre!(error))?;
        let original_store = app.ensure_thread_channel(child).store.clone();
        {
            let mut store = original_store.lock().await;
            store.attachment_prefix = Some(0);
            store.set_active_turn_id("newer-live-turn".into());
            store.push_notification(turn_started_notification(child, "newer-live-turn"));
        }
        server
            .unread_revisions
            .insert(child, "newer-live-read".into());
        let updates = SubscriptionUpdates {
            settings: Some(current_settings(&app)),
            turn_changed: true,
            read_state_changed: true,
            close: if closed {
                CloseState::Closed
            } else {
                CloseState::Open
            },
        };
        server.install_attachment_metadata(child, prepared.metadata, updates.read_state_changed);
        let live = app
            .install_thread_attachment(&mut server, child, attachment, ChannelMode::Reuse(&updates))
            .await;
        assert_eq!(live, !closed);
        let channel = app
            .thread_event_channels
            .get(&child)
            .ok_or_else(|| eyre!("original channel missing"))?;
        assert!(Arc::ptr_eq(&channel.store, &original_store));
        let store = original_store.lock().await;
        let session = store
            .session
            .as_ref()
            .ok_or_else(|| eyre!("original snapshot not installed"))?;
        assert_eq!(session.model, "newer-live-model");
        assert_eq!(session.model_provider_id, "newer-live-provider");
        assert_eq!(session.service_tier.as_deref(), Some("newer-live-tier"));
        assert!(session.forked_from_id.is_none());
        assert_eq!(store.turns[0].id, "snapshot");
        assert_eq!(
            store.active_turn_id.as_deref(),
            (!closed).then_some("newer-live-turn")
        );
        assert!(store.attachment_prefix.is_none());
        assert_eq!(
            server.unread_revisions.get(&child).map(String::as_str),
            Some("newer-live-read")
        );
        assert_eq!(app.primary_thread_id, Some(parent));
        assert!(commands.try_recv().is_err());
    }
    Ok(())
}

#[tokio::test]
async fn generic_selection_keeps_replace_live_for_actual_primary_and_actual_side() -> Result<()> {
    for side in [false, true] {
        let mut app = make_test_app().await;
        let parent = ThreadId::new();
        let thread = if side { ThreadId::new() } else { parent };
        app.primary_thread_id = Some(parent);
        if side {
            app.side_threads
                .insert(thread, side::SideThreadState::new(parent));
        }
        let old_store = app.ensure_thread_channel(thread).store.clone();
        old_store
            .lock()
            .await
            .push_notification(turn_started_notification(thread, "old-buffer"));
        let (mut server, mut commands) = channel_session();
        let reply = response(&app, thread, false)?;
        let (prepared, served) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                alab_attachment::prepare_selection(
                    &app,
                    &mut server,
                    thread,
                    ResumeModelSettings::PreserveExistingThread
                ),
                serve_original_resume(&mut commands, reply, 1),
            )
        })
        .await?;
        served?;
        let prepared = prepared?;
        assert!(alab_attachment::is_live(&prepared));
        let live = app
            .install_thread_attachment(&mut server, thread, prepared, ChannelMode::ReplaceLive)
            .await;
        assert!(live);
        let channel = app
            .thread_event_channels
            .get(&thread)
            .ok_or_else(|| eyre!("replacement channel missing"))?;
        assert!(!Arc::ptr_eq(&old_store, &channel.store));
        let store = channel.store.lock().await;
        assert_eq!(
            store.session.as_ref().map(|session| session.thread_id),
            Some(thread)
        );
        assert_eq!(store.turns[0].id, "snapshot");
        assert!(store.buffer.is_empty());
        assert_eq!(app.primary_thread_id, Some(parent));
        assert!(commands.try_recv().is_err());
    }
    Ok(())
}
