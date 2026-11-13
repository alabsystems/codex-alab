// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the actual configuration boundary in isolated ALab and upstream processes.

use super::*;
use crate::app::test_support::make_test_app;
use crate::app_event::AppEvent;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use codex_config::LoaderOverrides;
use std::time::Duration;

const CASE: &str = "ALAB_ULTRA_DEFAULT_CASE";
const COMPLETE: &str = "ALAB_ULTRA_DEFAULT_OK";

#[tokio::test]
async fn alab_ultra_default_survives_same_model_selection() {
    assert!(!codex_alab_core::active());
    let (_, module) = module_path!().split_once("::").expect("test module");
    let child = format!("{module}::alab_ultra_default_child");
    for case in [
        "same-model",
        "changed-model",
        "low-default",
        "vanilla",
        "upstream",
        "persist-restart",
        "persist-restart-upstream",
    ] {
        let mut command =
            tokio::process::Command::new(std::env::current_exe().expect("test binary"));
        command
            .args(["--exact", &child, "--nocapture", "--test-threads=1"])
            .env(CASE, case)
            // Match alab/unit-tests.sh for the full embedded-server fixture.
            .env("RUST_MIN_STACK", "8388608")
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .expect("Ultra default child hang detector")
            .expect("Ultra default child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{case}: {stdout}\n{stderr}");
        assert!(stdout.contains(COMPLETE), "{case}: {stdout}\n{stderr}");
    }
    assert!(!codex_alab_core::active());
}

#[tokio::test]
#[expect(
    clippy::print_stdout,
    reason = "parent verifies the isolated test actually ran"
)]
async fn alab_ultra_default_child() {
    let (_, module) = module_path!().split_once("::").expect("test module");
    let name = format!("{module}::alab_ultra_default_child");
    if std::env::args().skip(1).collect::<Vec<_>>()
        != ["--exact", &name, "--nocapture", "--test-threads=1"]
    {
        return;
    }
    let Ok(case) = std::env::var(CASE) else {
        return;
    };
    assert!(matches!(
        case.as_str(),
        "same-model"
            | "changed-model"
            | "low-default"
            | "vanilla"
            | "upstream"
            | "persist-restart"
            | "persist-restart-upstream"
    ));
    if !matches!(case.as_str(), "upstream" | "persist-restart-upstream") {
        codex_alab_core::activate();
    }
    if case == "vanilla" {
        codex_alab_core::set_enhanced(false);
    }
    if matches!(
        case.as_str(),
        "persist-restart" | "persist-restart-upstream"
    ) {
        let expected = if case == "persist-restart" {
            ReasoningEffortConfig::Ultra
        } else {
            ReasoningEffortConfig::Medium
        };
        Box::pin(persisted_effort_starts_a_fresh_server(expected))
            .await
            .expect("Ultra persistence and restart");
        println!("{COMPLETE}");
        return;
    }
    let mut app = make_test_app().await;
    app.config.model = Some(
        if case == "changed-model" {
            "old-model"
        } else {
            "gpt-5.5"
        }
        .into(),
    );
    let configured = if case == "low-default" {
        ReasoningEffortConfig::Low
    } else {
        ReasoningEffortConfig::Ultra
    };
    app.config.model_reasoning_effort = Some(configured.clone());
    app.chat_widget.set_reasoning_effort(Some(configured));

    let default = app.on_apply_advanced_reasoning("gpt-5.5", ReasoningEffortConfig::Ultra);
    let expected = match case.as_str() {
        "same-model" => ReasoningEffortConfig::Ultra,
        "low-default" => ReasoningEffortConfig::Low,
        _ => ReasoningEffortConfig::Medium,
    };
    assert_eq!(
        default,
        Some(expected.clone()),
        "value returned for persistence"
    );
    assert_eq!(app.config.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(app.config.model_reasoning_effort, Some(expected));
    assert_eq!(app.chat_widget.current_model(), "gpt-5.5");
    assert_eq!(
        app.chat_widget.current_reasoning_effort(),
        Some(ReasoningEffortConfig::Ultra)
    );
    println!("{COMPLETE}");
}

async fn persisted_effort_starts_a_fresh_server(
    expected: ReasoningEffortConfig,
) -> color_eyre::Result<()> {
    let mut app = make_test_app().await;
    let home = app.config.codex_home.to_path_buf();
    let cwd = app.config.cwd.to_path_buf();
    let config_path = home.join("config.toml");
    std::fs::write(
        &config_path,
        "# Keep my startup default.\nmodel = 'gpt-5.5'\nmodel_reasoning_effort = 'ultra'\n",
    )?;
    app.config.model = Some("gpt-5.5".into());
    app.config.model_reasoning_effort = Some(ReasoningEffortConfig::Ultra);
    app.chat_widget.set_model("gpt-5.5");
    app.chat_widget
        .set_reasoning_effort(Some(ReasoningEffortConfig::Ultra));

    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let started = server.start_thread(&app.config).await?;
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    Box::pin(app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::ApplyAdvancedReasoning {
            model: "gpt-5.5".into(),
            effort: ReasoningEffortConfig::Ultra,
        },
    ))
    .await?;

    let persisted: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path)?)?;
    assert_eq!(persisted["model"].as_str(), Some("gpt-5.5"));
    assert_eq!(
        persisted["model_reasoning_effort"].as_str(),
        Some(expected.to_string().as_str())
    );
    assert_eq!(
        app.chat_widget.current_reasoning_effort(),
        Some(ReasoningEffortConfig::Ultra)
    );
    server.shutdown().await?;

    // Rebuild from the file after shutdown, not from the old App's in-memory settings.
    let fresh_config = Box::pin(
        ConfigBuilder::default()
            .codex_home(home)
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .harness_overrides(ConfigOverrides {
                cwd: Some(cwd),
                ..Default::default()
            })
            .build(),
    )
    .await?;
    assert_eq!(fresh_config.model_reasoning_effort, Some(expected.clone()));
    let mut restarted =
        Box::pin(crate::start_embedded_app_server_for_picker(&fresh_config)).await?;
    let fresh = restarted.start_thread(&fresh_config).await?;
    assert_eq!(fresh.session.model, "gpt-5.5");
    assert_eq!(fresh.session.reasoning_effort, Some(expected));
    restarted.shutdown().await?;
    Ok(())
}
