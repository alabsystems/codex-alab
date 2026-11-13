// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The same native producer and App, initialized normally inside one private child.

use super::CaseOutcome;
use super::Fixture;
use crate::app_command::AppCommand;
use crate::app_command::CloudSubmissionIntent;
use crate::app_server_session::AppServerSessionEvent;
use crate::app_server_session::TurnPermissionsOverride;
use crate::app_server_session::user_message_submission::TurnStartSettings;
use crate::app_server_session::user_message_submission::UserMessageAdmission;
use crate::backend_id::BackendId;
use codex_alab_aterm::Endpoint;
use codex_alab_core::session::Launch;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::now_ms;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

fn fixture_cli() -> clap::Command {
    clap::Command::new("codex")
        .subcommand(clap::Command::new("resume").arg(clap::Arg::new("session_id")))
}

fn deliver(
    app: &mut crate::app::App,
    owner: BackendId,
    event: AppServerSessionEvent,
) -> color_eyre::Result<bool> {
    if event.backend_id != owner {
        color_eyre::eyre::bail!("the native producer changed backend ownership");
    }
    app.alab_backend_bookkeeping.displayed(owner);
    super::super::stream(app, owner, &event.event);
    match event.event {
        AppServerEvent::ServerNotification(notification) => {
            let completed = matches!(&*notification, ServerNotification::TurnCompleted(_));
            crate::app::alab_backend_bookkeeping::routed(app, owner, &notification);
            app.chat_widget
                .handle_server_notification(*notification, None);
            Ok(completed)
        }
        AppServerEvent::ServerRequest(_) => {
            color_eyre::eyre::bail!("the text-only fixture asked for an unexpected server request");
        }
        AppServerEvent::Lagged { .. }
        | AppServerEvent::Disconnected { .. }
        | AppServerEvent::ThreadStreamReconnected(_)
        | AppServerEvent::ThreadStreamDisconnected(_) => {
            color_eyre::eyre::bail!("the fixture's native event stream lost continuity");
        }
    }
}

pub(super) async fn run(fixture: &Fixture) -> color_eyre::Result<CaseOutcome> {
    const PROGRESS: &str = "The isolated fixture work is in progress.";
    const FINAL: &str = "Continue implementing the remaining fixture changes.";
    const INPUT: &str = "Work on the isolated fixture task.";
    let (finish, finished) = tokio::sync::oneshot::channel();
    let (next, next_held) = tokio::sync::oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![
                    responses::ev_response_created("bootstrap"),
                    responses::ev_message_item_added("message", ""),
                    responses::ev_output_text_delta(PROGRESS),
                ]),
            },
            StreamingSseChunk {
                gate: Some(finished),
                body: responses::sse(vec![
                    responses::ev_assistant_message("message", FINAL),
                    responses::ev_completed("bootstrap"),
                ]),
            },
        ],
        vec![StreamingSseChunk {
            gate: Some(next_held),
            body: responses::sse_completed("continuation"),
        }],
    ])
    .await;
    let catalog = fixture.codex_home.join("catalog.json");
    std::fs::write(&catalog, include_str!("../../models-manager/models.json"))?;
    let quoted = |path: &Path| serde_json::to_string(&path.to_string_lossy());
    let config_text = format!(
        "model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\n\
         model_catalog_json = {}\ncli_auth_credentials_store = \"file\"\n\
         mcp_oauth_credentials_store = \"file\"\ncheck_for_update_on_startup = false\n\
         [features]\napps = false\nshell_snapshot = false\n\
         [model_providers.fixture]\nname = \"fixture\"\nbase_url = \"{}/v1\"\n\
         wire_api = \"responses\"\nrequires_openai_auth = false\n\
         request_max_retries = 0\nstream_max_retries = 0\nstream_idle_timeout_ms = 3600000\n\
         [projects.{}]\ntrust_level = \"trusted\"\n",
        quoted(&catalog)?,
        model.uri(),
        quoted(&fixture.project)?
    );
    std::fs::write(fixture.codex_home.join("config.toml"), &config_text)?;
    std::fs::write(
        fixture.codex_home.join("auth.json"),
        br#"{"OPENAI_API_KEY":"private-fixture-not-a-key"}"#,
    )?;
    let config = crate::legacy_core::config::ConfigBuilder::default()
        .codex_home(fixture.codex_home.clone())
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .harness_overrides(crate::legacy_core::config::ConfigOverrides {
            cwd: Some(fixture.project.clone()),
            ..Default::default()
        })
        .build()
        .await?;
    assert_eq!(config.codex_home.as_path(), fixture.codex_home);
    assert_eq!(config.cwd.as_path(), fixture.project);
    assert!(
        config
            .model_provider
            .base_url
            .as_deref()
            .is_some_and(|base| base.starts_with(model.uri()))
    );
    assert!(
        config.model_catalog.is_some(),
        "the native producer uses the static catalog"
    );
    assert!(
        config.mcp_servers.get().is_empty(),
        "no MCP or external startup work"
    );
    let (mut app, mut events, _) = crate::app::tests::make_test_app_with_channels().await;
    if let Some(home) = app._test_codex_home.as_mut() {
        home.disable_cleanup(true);
    }
    app.config = config.clone();
    app.chat_widget.alab_fixture_config(config);
    app.launch_cwd = fixture.project.clone();
    app.local_settings = crate::local_settings::LocalSettings::from(&app.config);
    assert_eq!(
        app.chat_widget.config_ref().codex_home.as_path(),
        fixture.codex_home
    );
    let old = fixture.root.join("old-image");
    let target_directory = fixture.root.join("7/bin");
    std::fs::create_dir_all(&target_directory)?;
    let target = target_directory.join("codex");
    for path in [&old, &target] {
        std::fs::write(path, b"captured image identity; never executed")?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    let old_id = codex_alab_tui::upgrade::build_id(&old)
        .ok_or_else(|| color_eyre::eyre::eyre!("owned old identity"))?;
    let shared = codex_alab_core::session::shared();
    assert!(shared.view().primary_thread.is_none());
    assert!(shared.set_launch(Launch {
        args: vec!["codex".into()],
        argv0: Some(target),
        current_exe: Some(old),
        exe_id: Some((old_id.dev, old_id.ino)),
        cwd: Some(fixture.project.clone()),
        vars: std::env::vars_os().collect(),
        ..Default::default()
    }));
    codex_alab_tui::resume_argv::set_command(fixture_cli);
    // Only this exact sanitized child can reach normal process-wide initialization.
    codex_alab_core::activate();
    assert_eq!(
        Endpoint::from_env(),
        Some(Endpoint::explicit(&fixture.sock, &fixture.sid))
    );
    // This sanitized child supplies inert image files, not a signed package.
    // Retain the real producer, manager inventory and conditional TUI commit.
    let preparation = codex_alab_tui::upgrade::test_support::scoped_preparation(shared)
        .map_err(color_eyre::eyre::Error::msg)?;
    let mut native = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let native_owner = native.backend_id();
    let started = native.start_thread(&app.config).await?;
    // Complete the independent launch handshake before starting model work.
    assert!(
        codex_alab_tui::test_tasks::wait_for_launch_probe(shared).await?,
        "the private launch probe completed within its observation budget"
    );
    assert!(
        shared.launch_witness().is_some(),
        "the private launch probe succeeded"
    );
    let thread = started.session.thread_id;
    let rollout = started
        .session
        .rollout_path
        .clone()
        .ok_or_else(|| color_eyre::eyre::eyre!("the actual native rollout"))?;
    app.active_thread_id = Some(thread);
    app.primary_thread_id = Some(thread);
    app.chat_widget.handle_thread_session_quiet(started.session);
    let sender = app.app_event_tx.clone();
    codex_alab_tui::life::on_upgrade(move |request| {
        sender.send(crate::app_event::AppEvent::AlabUpgrade(request));
    });
    let frames = app.chat_widget.alab_frame_requester();
    codex_alab_tui::boxes::set_waker(move || frames.schedule_frame());
    assert!(
        shared.has_waker(),
        "the normal TUI frame requester is installed"
    );
    let intent = CloudSubmissionIntent::UseKnownTurn;
    let input = AppCommand::user_turn(
        "user",
        intent,
        "bootstrap-input".into(),
        vec![UserInput::Text {
            text: INPUT.into(),
            text_elements: Vec::new(),
        }],
        fixture.project.clone(),
        codex_app_server_protocol::AskForApproval::Never,
        None,
        "gpt-5.6-sol".into(),
        None,
        None,
        None,
        None,
        None,
    );
    let (admission, _) = native
        .submit_user_message(
            thread,
            &input,
            intent,
            &mut None,
            |_| Ok(None),
            || TurnStartSettings {
                approval_policy: None,
                approvals_reviewer: None,
                permissions: TurnPermissionsOverride::Preserve,
                workspace_roots: &[],
            },
        )
        .await
        .map_err(|failure| color_eyre::eyre::eyre!("native bootstrap admission: {failure:?}"))?;
    let UserMessageAdmission::StartResponse(turn) = admission else {
        color_eyre::eyre::bail!("the native bootstrap did not start a turn");
    };
    loop {
        let event = native
            .next_event()
            .await
            .ok_or_else(|| color_eyre::eyre::eyre!("actual bootstrap event"))?;
        let progress = match &event.event {
            AppServerEvent::ServerNotification(notification) => {
                let text = serde_json::to_string(notification)?;
                text.contains(PROGRESS) && text.contains(&thread.to_string())
            }
            _ => false,
        };
        assert!(
            !deliver(&mut app, native_owner, event)?,
            "bootstrap stays gated before timing control"
        );
        if progress {
            break;
        }
    }
    assert!(
        shared.view().primary_thread.is_none(),
        "no claim RPC while advancing time"
    );
    assert!(app.chat_widget.is_user_turn_pending_or_running());
    assert_eq!(model.requests().await.len(), 1, "one actual bootstrap POST");
    let keeper = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    tokio::time::pause();
    let before = tokio::time::Instant::now();
    tokio::time::advance(Duration::from_secs(121)).await;
    assert_eq!(
        tokio::time::Instant::now().duration_since(before),
        Duration::from_secs(121)
    );
    tokio::time::resume();
    keeper.abort();
    let _ = keeper.await;
    finish
        .send(())
        .map_err(|()| color_eyre::eyre::eyre!("release the one real SSE completion"))?;
    let completion = loop {
        let event = native
            .next_event()
            .await
            .ok_or_else(|| color_eyre::eyre::eyre!("bootstrap terminal notification"))?;
        if let AppServerEvent::ServerNotification(notification) = &event.event
            && let ServerNotification::TurnCompleted(completed) = &**notification
        {
            assert_eq!(completed.thread_id, thread.to_string());
            assert_eq!(completed.turn.id, turn);
            assert!(
                completed.turn.error.is_none(),
                "the bootstrap genuinely succeeded"
            );
            break event;
        }
        deliver(&mut app, native_owner, event)?;
    };
    assert!(
        rollout.is_file(),
        "the real successful turn materialized its rollout"
    );
    let held = fixture.case == "held-completion";
    let mut completion = Some(completion);
    if !held {
        deliver(
            &mut app,
            native_owner,
            completion
                .take()
                .ok_or_else(|| color_eyre::eyre::eyre!("the delivered-first completion"))?,
        )?;
    }
    super::super::observe(&app);
    assert_eq!(
        shared.view().primary_thread.as_deref(),
        Some(thread.to_string().as_str())
    );
    let mut rejected_before_delivery = false;
    let accepted = loop {
        tokio::select! {
            _ = model.wait_for_request_count(2) => break false,
            event = events.recv() => {
                if let Some(crate::app_event::AppEvent::AlabUpgrade(request)) = event {
                    let owed = shared.owed_act()
                        .ok_or_else(|| color_eyre::eyre::eyre!("a genuine completed point is carried"))?;
                    let origin = owed.native_origin.as_ref()
                        .ok_or_else(|| color_eyre::eyre::eyre!("bound native origin"))?;
                    assert_eq!(origin.turn_id, turn);
                    assert_eq!(origin.phase, NativePhase::Complete);
                    assert!(owed.not_before_ms <= now_ms(), "the real-work point is due");
                    let committed = super::super::accept_upgrade(&app, request);
                    if held && !rejected_before_delivery {
                        assert!(!committed, "the actual still-running widget rejects leave");
                        rejected_before_delivery = true;
                        // No await: catch-up finishes before the evaluator can resume fallback.
                        deliver(&mut app, native_owner, completion.take()
                            .ok_or_else(|| color_eyre::eyre::eyre!("the held actual completion"))?)?;
                        super::super::observe(&app);
                        assert!(!app.chat_widget.is_user_turn_pending_or_running());
                        assert!(matches!(app.chat_widget.alab_program_state(), ProgramState::Idle | ProgramState::Done | ProgramState::Error));
                    }
                    if committed { break true; }
                }
            }
            event = native.next_event() => {
                deliver(&mut app, native_owner, event
                    .ok_or_else(|| color_eyre::eyre::eyre!("the native stream remains continuous"))?)?;
            }
        }
    };
    if held {
        assert!(
            rejected_before_delivery,
            "the candidate reached real TUI rejection first"
        );
    }
    let requests = model.requests().await;
    std::fs::write(
        fixture.root.join("model-requests.json"),
        serde_json::to_vec_pretty(&requests)?,
    )?;
    let bodies = requests
        .iter()
        .map(|body| serde_json::from_slice::<serde_json::Value>(body))
        .collect::<Result<Vec<_>, _>>()?;
    let last_user_text = |body: &serde_json::Value| -> color_eyre::Result<Option<String>> {
        let Some(item) = body["input"]
            .as_array()
            .and_then(|input| input.iter().rev().find(|item| item["role"] == "user"))
        else {
            return Ok(None);
        };
        let content = item["content"]
            .as_array()
            .ok_or_else(|| color_eyre::eyre::eyre!("a Responses user message has content"))?;
        Ok(Some(
            content
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    };
    assert_eq!(last_user_text(&bodies[0])?.as_deref(), Some(INPUT));
    assert_eq!(bodies[0]["model"], "gpt-5.6-sol");
    if !accepted {
        assert_eq!(
            requests.len(),
            2,
            "one actual continuation follows bootstrap"
        );
        assert_eq!(bodies[1]["model"], bodies[0]["model"]);
        let continued = last_user_text(&bodies[1])?
            .ok_or_else(|| color_eyre::eyre::eyre!("the actual continuation user message"))?;
        assert!(continued.contains(super::CONTINUE_TEXT));
        assert_ne!(
            continued, INPUT,
            "the denied handoff did not replay bootstrap"
        );
    }
    drop(next);
    native.shutdown().await?;
    model.shutdown().await;
    let upgrade_status = codex_alab_tui::upgrade::update_report("isolated native fixture");
    assert!(
        preparation.preparations() > 0,
        "the package-preparation fixture was exercised: {upgrade_status}"
    );
    drop(preparation);
    Ok(CaseOutcome {
        case: fixture.case.clone(),
        completed: true,
        accepted_upgrade: accepted,
        continuation_posts_before_upgrade: requests.len().saturating_sub(1),
        upgrade_status,
    })
}
