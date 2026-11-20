// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::focus_palette::PtyCodex;
use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_models_manager::sol_family::newest_eligible_successor;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_string_contains;
use wiremock::matchers::method;
use wiremock::matchers::path;

const SEED: &str = "Remember the cedar marker from this conversation.";
const SEED_REPLY: &str = "I have recorded the cedar marker.";
const CONTINUE: &str = "Continue the same conversation using the cedar marker.";
const CONTINUED_REPLY: &str = "The cedar marker survived the resume.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_resume_sends_the_followed_or_pinned_model_with_the_original_history() -> Result<()>
{
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let workspace = tempfile::tempdir()?;
    let cwd = workspace.path().canonicalize()?;
    let catalog: Vec<ModelPreset> = codex_models_manager::bundled_models_response()?
        .models
        .into_iter()
        .filter(|model| model.supported_in_api)
        .map(ModelPreset::from)
        .collect();
    for (saved, pin) in [
        ("gpt-6.1-sol", None),
        ("gpt-6-astra", None),
        ("gpt-6.1-sol", Some("gpt-6.1-sol")),
    ] {
        let successor = newest_eligible_successor(&catalog, saved, Some(&ReasoningEffort::Ultra))
            .expect("the bundled API catalog has a newer eligible model in this family");
        let expected = pin.unwrap_or(catalog[successor].model.as_str());
        let home = codex_alab_tui::test_paths::short_tempdir()?;
        codex_login::login_with_api_key(
            home.path(),
            "focus-palette-test",
            codex_login::AuthCredentialsStoreMode::File,
            Default::default(),
        )?;
        let backend = responses::start_mock_server().await;
        Mock::given(method("GET"))
            .and(path("/backend-api/codex/responses"))
            .respond_with(ResponseTemplate::new(426))
            .mount(&backend)
            .await;
        let base = backend.uri();
        let trusted = toml::Value::String(cwd.display().to_string());
        std::fs::write(
            home.path().join("config.toml"),
            format!(
                r#"openai_base_url = "{base}/backend-api/codex"
chatgpt_base_url = "{base}/backend-api"
model_reasoning_effort = "ultra"
cli_auth_credentials_store = "file"
features.enable_request_compression = false
features.daemon_auto_start = false
analytics.enabled = false
[projects.{trusted}]
trust_level = "trusted"
"#
            ),
        )?;
        let mut seed = TestAppServer::builder()
            .with_program(&codex)
            // The standalone app-server's hidden startup flag is absent from the production CLI.
            .with_plugin_startup_tasks()
            .with_args(&["app-server"])
            .without_auto_env()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
        let first = responses::mount_sse_once_match(
            &backend,
            body_string_contains(SEED),
            responses::sse(vec![
                responses::ev_assistant_message("seed-message", SEED_REPLY),
                responses::ev_completed("seed-response"),
            ]),
        )
        .await;
        let request_id = seed
            .send_thread_start_request(ThreadStartParams {
                model: Some(saved.to_owned()),
                cwd: Some(cwd.display().to_string()),
                ..Default::default()
            })
            .await?;
        let thread = seed
            .read_response::<ThreadStartResponse>(request_id)
            .await?
            .thread;
        let completed = seed
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: thread.id.clone(),
                input: vec![UserInput::Text {
                    text: SEED.to_owned(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        assert_eq!(completed.turn.status, TurnStatus::Completed);
        let original = first.single_request().body_json();
        assert_eq!(original["model"], saved);
        assert_eq!(original["reasoning"]["effort"], "xhigh");
        assert!(seed.shutdown_gracefully().await?.success());
        let continued = responses::mount_sse_once_match(
            &backend,
            body_string_contains(CONTINUE),
            responses::sse(vec![
                responses::ev_assistant_message("continued-message", CONTINUED_REPLY),
                responses::ev_completed("continued-response"),
            ]),
        )
        .await;
        let mut args = vec!["--no-daemon", "--no-alt-screen"];
        if let Some(pin) = pin {
            args.extend(["-m", pin]);
        }
        args.extend(["resume", thread.id.as_str(), CONTINUE]);
        let mut terminal = PtyCodex::start_cli(&cwd, home, &args)?;
        terminal.wait_for_screen(CONTINUED_REPLY)?;
        let request = continued.single_request();
        assert_eq!(
            request.header("thread-id").as_deref(),
            Some(thread.id.as_str())
        );
        assert_eq!(request.body_json()["model"], expected);
        assert_eq!(
            request.body_json()["reasoning"]["effort"],
            original["reasoning"]["effort"]
        );
        assert!(
            request
                .message_input_texts("user")
                .iter()
                .any(|text| text == SEED)
        );
        assert!(request.input().iter().any(|item| {
            item["role"] == "assistant"
                && item["content"].as_array().is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part["type"] == "output_text" && part["text"] == SEED_REPLY)
                })
        }));
    }
    Ok(())
}
