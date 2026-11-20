// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;

fn read_reply(thread: Thread, revision: &str) -> serde_json::Result<serde_json::Value> {
    serde_json::to_value(ThreadReadResponse {
        thread,
        read_state: Some(ThreadReadState {
            first_unread: Some(ThreadUnreadPosition::ThreadStart),
            revision: revision.into(),
        }),
    })
}

async fn answer_read(
    commands: &mut tokio::sync::mpsc::Receiver<CloudCommand>,
    requested: ThreadId,
    include_turns: bool,
    reply: std::result::Result<serde_json::Value, JSONRPCErrorError>,
) -> Result<()> {
    let command = commands
        .recv()
        .await
        .ok_or_else(|| eyre!("expected thread/read"))?;
    let CloudOperation::Request(request) = command.operation else {
        bail!("unexpected operation during thread lookup");
    };
    let ClientRequest::ThreadRead { params, .. } = *request else {
        bail!("expected a read with no resume or history page");
    };
    assert_eq!(params.thread_id, requested.to_string());
    assert_eq!(params.include_turns, include_turns);
    command
        .response
        .send(Ok(reply))
        .map_err(|_| eyre!("lost read request"))
}

#[tokio::test]
async fn uuid_lookup_and_paginated_read_reject_another_conversations_reply() -> Result<()> {
    let (mut app, _events, _ops) = crate::boxed_future!(make_test_app_with_channels()).await;
    let displayed = ThreadId::new();
    let requested = ThreadId::new();
    let foreign = ThreadId::new();
    app.enqueue_primary_thread_session(
        test_thread_session(displayed, app.config.cwd.to_path_buf()),
        Vec::new(),
    )
    .await?;
    let (mut server, mut commands) = channel_session();
    server
        .unread_revisions
        .insert(requested, "previous revision".into());
    let mut foreign_thread = response(&app, foreign, false)?.thread;
    foreign_thread.history_mode = ThreadHistoryMode::Paginated;
    let mut matching_thread = response(&app, requested, false)?.thread;
    matching_thread.id = requested.to_string().to_uppercase();
    let foreign_reply = read_reply(foreign_thread, "foreign revision")?;
    let matching_reply = read_reply(matching_thread, "matching revision")?;
    let paginated = JSONRPCErrorError {
        code: -32602,
        message: "paginated threads do not support thread/read(includeTurns=true)".into(),
        data: None,
    };
    let serve = async move {
        for (include_turns, reply) in [
            (false, Ok(foreign_reply.clone())),
            (true, Ok(foreign_reply.clone())),
            (true, Err(paginated)),
            (false, Ok(foreign_reply)),
            (false, Ok(matching_reply)),
        ] {
            answer_read(&mut commands, requested, include_turns, reply).await?;
        }
        Ok::<(), color_eyre::Report>(())
    };
    let client = async {
        let mut tui = crate::tui::test_support::make_test_tui()?;
        crate::boxed_future!(app.handle_event(
            &mut tui,
            &mut server,
            AppEvent::ResumeSessionByIdOrName(requested.to_string())
        ))
        .await?;
        assert_eq!(app.current_displayed_thread_id(), Some(displayed));
        // Exercise both a direct history reply and the retry without turns.
        for _ in 0..2 {
            assert!(server.thread_read(requested, true).await.is_err());
        }
        assert_eq!(
            server.unread_revisions.get(&requested).map(String::as_str),
            Some("previous revision")
        );
        assert!(!server.unread_revisions.contains_key(&foreign));
        let target = crate::lookup_session_target_with_app_server(
            crate::resume_picker::SessionPickerAction::Resume,
            &mut server,
            &app.config,
            &requested.to_string(),
        )
        .await?
        .expect("same UUID in the matching reply");
        assert_eq!(target.thread_id, requested);
        assert_eq!(
            server.unread_revisions.get(&requested).map(String::as_str),
            Some("matching revision")
        );
        Ok::<(), color_eyre::Report>(())
    };
    tokio::time::timeout(Duration::from_secs(60), async {
        tokio::try_join!(serve, client)
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn name_lookup_skips_foreign_and_malformed_read_ids_before_valid_match() -> Result<()> {
    let (app, _events, _ops) = crate::boxed_future!(make_test_app_with_channels()).await;
    let (mut server, mut commands) = channel_session();
    let stale = ThreadId::new();
    let malformed = ThreadId::new();
    let current = ThreadId::new();
    let mut listed = Vec::new();
    for id in [stale, malformed, current] {
        let mut thread = response(&app, id, false)?.thread;
        thread.name = Some("Compiler".into());
        listed.push(thread);
    }
    let matching = listed.last().expect("current listed candidate").clone();
    let mut invalid = matching.clone();
    invalid.id = "invalid-uuid".into();
    let serve = async move {
        let command = commands
            .recv()
            .await
            .ok_or_else(|| eyre!("expected thread/list"))?;
        let CloudOperation::Request(request) = command.operation else {
            bail!("expected a list request");
        };
        assert!(matches!(*request, ClientRequest::ThreadList { .. }));
        command
            .response
            .send(Ok(Ok(
                serde_json::json!({"data": listed, "nextCursor": null}),
            )))
            .map_err(|_| eyre!("lost name lookup"))?;
        for (candidate, thread) in [
            (stale, matching.clone()),
            (malformed, invalid),
            (current, matching),
        ] {
            answer_read(
                &mut commands,
                candidate,
                false,
                Ok(read_reply(thread, "seen")?),
            )
            .await?;
        }
        Ok::<(), color_eyre::Report>(())
    };
    let lookup = crate::lookup_session_target_with_app_server(
        crate::resume_picker::SessionPickerAction::Resume,
        &mut server,
        &app.config,
        "Compiler",
    );
    let (_, target) = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::try_join!(serve, lookup)
    })
    .await??;
    assert_eq!(target.expect("verified label").thread_id, current);
    assert!(!server.unread_revisions.contains_key(&stale));
    assert!(!server.unread_revisions.contains_key(&malformed));
    assert_eq!(
        server.unread_revisions.get(&current).map(String::as_str),
        Some("seen")
    );
    Ok(())
}

#[tokio::test]
async fn wrong_thread_resume_reply_stops_cli_startup_before_history_hydration() -> Result<()> {
    let foreign = ThreadId::new();
    check_refused_resume_id(foreign.to_string(), format!("thread ID {foreign}")).await
}

#[tokio::test]
async fn malformed_thread_resume_reply_stops_cli_startup_before_recovery() -> Result<()> {
    check_refused_resume_id("not-a-uuid".into(), "an invalid thread ID".into()).await
}

async fn check_refused_resume_id(returned: String, description: String) -> Result<()> {
    use crate::app_server_session::history_page::ThreadResponseIdMismatch;
    use crate::session_start::SessionStartAction;
    use crate::session_start::SessionStartConfig;
    use crate::session_start::complete_session_start;

    let (app, _events, _ops) = crate::boxed_future!(make_test_app_with_channels()).await;
    let (mut server, mut commands) = channel_session();
    let requested = ThreadId::new();
    let correct_metadata = response(&app, requested, false)?.thread;
    let mut wrong_reply = response(&app, requested, false)?;
    wrong_reply.thread.id = returned;
    wrong_reply.thread.history_mode = ThreadHistoryMode::Paginated;
    let serve = async {
        answer_read(
            &mut commands,
            requested,
            false,
            Ok(read_reply(correct_metadata, "requested")?),
        )
        .await?;
        let command = commands
            .recv()
            .await
            .ok_or_else(|| eyre!("expected thread/resume"))?;
        let CloudOperation::Request(request) = command.operation else {
            bail!("expected a resume request");
        };
        let ClientRequest::ThreadResume { params, .. } = *request else {
            bail!("expected thread/resume after reading the requested conversation");
        };
        assert_eq!(params.thread_id, requested.to_string());
        command
            .response
            .send(Ok(Ok(serde_json::to_value(wrong_reply)?)))
            .map_err(|_| eyre!("lost resume request"))?;
        Ok::<(), color_eyre::Report>(())
    };
    let settings = ResumeModelSettings::RestoreFromThread;
    let target = crate::resume_picker::SessionTarget {
        backend_id: server.backend_id(),
        path: None,
        thread_id: requested,
        cwd: None,
        history_mode: None,
    };
    let resume = server.resume_thread(&app.local_settings, app.config.clone(), requested, settings);
    let (served, result) = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::join!(serve, resume)
    })
    .await?;
    served?;
    let mismatch = result
        .as_ref()
        .expect_err("resume accepted a mismatched ID");
    assert!(mismatch.is::<ThreadResponseIdMismatch>());
    let diagnostic = format!("thread/resume for {requested} returned {description}");
    assert_eq!(mismatch.to_string(), diagnostic);
    assert!(!crate::app_server_session::is_active_writer_error(mismatch));
    let refused = complete_session_start(
        &mut server,
        SessionStartConfig {
            config: &app.config,
            local_settings: &app.local_settings,
        },
        &target,
        SessionStartAction::Resume(
            settings,
            crate::resume_permissions::ResumePermissions::default(),
        ),
        result,
        async || panic!("identity refusal must not trigger an archive or retry"),
    )
    .await
    .expect_err("CLI resume accepted the foreign thread");
    assert!(refused.to_string().ends_with(&diagnostic));
    assert!(commands.try_recv().is_err());
    Ok(())
}
