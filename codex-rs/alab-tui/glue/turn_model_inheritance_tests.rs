// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app_command::CloudSubmissionIntent;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use crate::app_server_session::TurnPermissionsOverride;
use crate::app_server_session::user_message_submission::TurnStartSettings;
use crate::chatwidget::tests::helpers::make_chatwidget_manual_with_sender;
use crate::legacy_core::config::Config;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::TurnStartParams;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::SessionSource;

use core_test_support::alab_test_process as process;

fn compose(widget: &ChatWidget, mode: &CollaborationMode) -> AppCommand {
    widget.alab_mark_inherited_turn(AppCommand::user_turn(
        "user",
        CloudSubmissionIntent::UseKnownTurn,
        "ordinary-message".to_owned(),
        Vec::new(),
        widget.config_ref().cwd.to_path_buf(),
        AskForApproval::Never,
        None,
        mode.model().to_owned(),
        mode.reasoning_effort(),
        None,
        None,
        None,
        Some(mode.clone()),
    ))
}

async fn wire(
    thread: ThreadId,
    op: &AppCommand,
    settings_supported: bool,
    transport: ThreadParamsMode,
) -> TurnStartParams {
    let (client, mut commands, _events) = CloudClient::channel(1);
    let mut session = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        AppServerClient::Cloud(client),
        transport,
    );
    session.thread_settings_update_supported = settings_supported;
    let mut active = None;
    let (result, params) = tokio::join!(
        session.submit_user_message(
            thread,
            op,
            CloudSubmissionIntent::UseKnownTurn,
            &mut active,
            |_| Ok(None),
            || TurnStartSettings {
                approval_policy: None,
                approvals_reviewer: None,
                permissions: TurnPermissionsOverride::Preserve,
                workspace_roots: &[],
            },
        ),
        async {
            let command = commands.recv().await.expect("turn/start RPC");
            let CloudOperation::Request(request) = command.operation else {
                panic!("expected an RPC");
            };
            let ClientRequest::TurnStart { params, .. } = *request else {
                panic!("expected turn/start");
            };
            command
                .response
                .send(Ok(Err(JSONRPCErrorError {
                    code: -32602,
                    data: None,
                    message: "captured".into(),
                })))
                .expect("request still pending");
            params
        }
    );
    assert!(result.is_err());
    params
}

#[tokio::test]
async fn ordinary_turn_keeps_promoted_model_before_notification() {
    if !process::child(concat!(
        module_path!(),
        "::ordinary_turn_keeps_promoted_model_before_notification"
    )) {
        return;
    }
    let thread = ThreadId::new();
    let (mut widget, _sender, _events, _commands) = make_chatwidget_manual_with_sender().await;
    widget.thread_id = Some(thread);
    let effort = Some(ReasoningEffort::Ultra);
    assert!(!Config::alab_is_tui_start(
        &SessionSource::Cli,
        Some("codex-tui")
    ));
    assert!(Config::alab_fresh_tui_mode("gpt-6.1-sol", effort.clone()).is_none());
    codex_alab_core::activate();
    assert!(Config::alab_is_tui_start(
        &SessionSource::Cli,
        Some("codex-tui")
    ));
    assert!(!Config::alab_is_tui_start(
        &SessionSource::Cli,
        Some("external")
    ));
    assert!(!Config::alab_is_tui_start(
        &SessionSource::VSCode,
        Some("codex-tui")
    ));
    let (socket_stand_in, _, _) = CloudClient::channel(1);
    let handle = AppServerClient::Cloud(socket_stand_in).request_handle();
    assert!(!created_in_process(
        &handle,
        ThreadParamsMode::Embedded,
        &codex_app_server_protocol::SessionSource::Cli
    ));
    let fresh = Config::alab_fresh_tui_mode("gpt-6.1-sol", effort).expect("embedded TUI Default");
    assert!(
        fresh
            .settings
            .developer_instructions
            .as_deref()
            .is_some_and(|text| !text.is_empty())
    );

    for kind in [ModeKind::Default, ModeKind::Plan] {
        let mut observed = fresh.clone();
        observed.mode = kind;
        if kind == ModeKind::Plan {
            observed.settings.developer_instructions = Some("Private plan instructions".into());
        }
        widget.alab_observed_thread_mode = Some(observed.clone());
        let pending = compose(&widget, &observed);
        let mut newer = observed.clone();
        newer.settings.model = "gpt-6.2-sol".into();
        widget.alab_observed_thread_mode = Some(newer.clone());

        for transport in [ThreadParamsMode::Embedded, ThreadParamsMode::Remote] {
            let turn = wire(thread, &pending, true, transport).await;
            assert_eq!(
                (turn.model, turn.effort, turn.collaboration_mode),
                (None, None, None)
            );
        }
        for (target, supported) in [(ThreadId::new(), true), (thread, false)] {
            let turn = wire(target, &pending, supported, ThreadParamsMode::Remote).await;
            assert_eq!(turn.model.as_deref(), Some(observed.model()));
            assert_eq!(turn.collaboration_mode.as_ref(), Some(&observed));
        }
        let explicitly_older = wire(
            thread,
            &compose(&widget, &observed),
            true,
            ThreadParamsMode::Embedded,
        )
        .await;
        assert_eq!(explicitly_older.model.as_deref(), Some(observed.model()));
        assert_eq!(
            explicitly_older.collaboration_mode.as_ref(),
            Some(&observed)
        );
        let mut selected = newer;
        selected.settings.developer_instructions = Some("Explicit turn instructions".into());
        selected.settings.reasoning_effort = Some(ReasoningEffort::Low);
        let turn = wire(
            thread,
            &compose(&widget, &selected),
            true,
            ThreadParamsMode::Embedded,
        )
        .await;
        assert_eq!(turn.effort, selected.reasoning_effort());
        assert_eq!(turn.collaboration_mode, Some(selected));
    }
}
