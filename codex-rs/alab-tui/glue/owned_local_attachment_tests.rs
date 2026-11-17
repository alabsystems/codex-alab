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
