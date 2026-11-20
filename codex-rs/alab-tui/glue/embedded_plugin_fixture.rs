//! Plugin configuration for embedded servers in TUI unit tests.

use super::*;
use std::path::Component;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const PLUGINS_KEY: &str = "features.plugins";

/// Startup uses the caller's Config. Later requests independently reload the CLI overrides.
pub(super) fn isolate(
    mut config: Config,
    mut cli_overrides: Vec<(String, toml::Value)>,
) -> (Config, Vec<(String, toml::Value)>) {
    let opted_in = cli_overrides
        .iter()
        .rev()
        .find(|(key, _)| key == PLUGINS_KEY)
        .is_some_and(|(_, value)| value.as_bool() == Some(true));
    if opted_in {
        assert!(
            config
                .config_layer_stack
                .requirements()
                .marketplaces
                .as_ref()
                .is_some_and(|rules| rules.value.restrict_to_allowed_sources == Some(true)),
            "plugin fixtures must restrict their marketplace sources"
        );
    } else {
        config
            .features
            .disable(Feature::Plugins)
            .expect("disable fixture plugins");
        cli_overrides.push((PLUGINS_KEY.to_owned(), false.into()));
    }
    (config, cli_overrides)
}

/// Runs the plugin APIs with the same restricted marketplace policy at startup and on reload.
pub(crate) async fn start_server(
    config: &Config,
    repository: Option<&MockServer>,
) -> color_eyre::Result<AppServerSession> {
    let mut requirements = "[marketplaces]\nrestrict_to_allowed_sources = true\n".to_owned();
    if let Some(repository) = repository {
        requirements.push_str(&format!(
            "[marketplaces.allowed_sources.fixture]\nsource = \"git\"\nurl = \"{}/repo.git\"\n",
            repository.uri()
        ));
    }
    let path = config
        .codex_home
        .join("fixture-marketplace-requirements.toml");
    std::fs::write(&path, requirements)?;
    let loader = LoaderOverrides {
        system_requirements_path: Some(path.to_path_buf()),
        ..LoaderOverrides::without_managed_config_for_tests()
    };
    let cli = vec![(PLUGINS_KEY.to_owned(), true.into())];
    let config = ConfigBuilder::default()
        .codex_home(config.codex_home.to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(config.cwd.to_path_buf()),
            ..Default::default()
        })
        .loader_overrides(loader.clone())
        .cli_overrides(cli.clone())
        .build()
        .await?;
    let target = AppServerTarget::Embedded;
    let state_db = init_state_db_for_app_server_target(&config, &target).await?;
    Box::pin(start_app_server_for_picker(
        crate::backend_id::BackendId::new(),
        &config,
        &target,
        cli,
        loader,
        state_db,
        Arc::new(EnvironmentManager::default_for_tests()),
    ))
    .await
}

/// Serve the fixture's pre-generated dumb HTTP Git metadata and objects over loopback.
pub(crate) async fn serve_git(worktree: &Path) -> MockServer {
    let git = worktree.join(".git");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repo\.git/"))
        .respond_with(move |request: &wiremock::Request| {
            let relative = Path::new(request.url.path().trim_start_matches("/repo.git/"));
            if relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
                && let Ok(contents) = std::fs::read(git.join(relative))
            {
                ResponseTemplate::new(200).set_body_bytes(contents)
            } else {
                ResponseTemplate::new(404)
            }
        })
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn generic_embedded_plugins_stay_disabled_after_config_reload() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    std::fs::write(
        home.path().join("config.toml"),
        "[features]\nplugins = true\n",
    )?;
    let load = || {
        ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
    };
    let initial = load().build().await?;
    assert!(initial.plugins_config_input().plugins_enabled);
    let (startup, cli) = isolate(initial, Vec::new());
    assert!(!startup.plugins_config_input().plugins_enabled);
    let reloaded = load().cli_overrides(cli).build().await?;
    assert!(!reloaded.plugins_config_input().plugins_enabled);
    Ok(())
}
