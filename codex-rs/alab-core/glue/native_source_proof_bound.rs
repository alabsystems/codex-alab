// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit debug fixture inputs, NEVER preserved ordinary TUI defaults.
//! Actual Native bind precedes source/auth. ONE full-input bound runtime owns
//! its real Config/AuthManager/initial primary/GoalGate and survives frontend EOF.
use codex_alab_aterm::frontend_control::retained_service;
use codex_app_server::in_process::InitialSessionKind;
use codex_app_server::in_process::InitialSessionPlan;
use codex_app_server_client::EmbeddedNetworkPolicy;
use codex_app_server_client::InProcessClientStartArgs;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecServerRuntimeOptions;
use codex_feedback::CodexFeedback;
use std::io::Write as _;
use std::io::{self};
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

const BACKEND: &str = "--alab-retained-service";
const MODE: &str = "--alab-native-bound-proof";
pub(crate) fn requested() -> bool {
    std::env::args_os().nth(1).is_some_and(|arg| arg == BACKEND)
        && std::env::args_os().nth(2).is_some_and(|arg| arg == MODE)
}
fn unavailable() -> io::Error {
    io::Error::other("bound Source SDK fixture unavailable")
}
fn data_directory() -> io::Result<PathBuf> {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.len() != 4 || args[1] != BACKEND || args[2] != MODE {
        return Err(unavailable());
    }
    let home = std::env::var_os("CODEX_HOME").ok_or_else(unavailable)?;
    let home = std::fs::canonicalize(home)?;
    let path = PathBuf::from(&args[3]);
    let md = std::fs::symlink_metadata(&path)?;
    // SAFETY: side-effect-free real user identity; no pointer or mutable state.
    let uid = unsafe { libc::geteuid() };
    if !path.is_absolute()
        || path.canonicalize()? != path
        || path.parent() != Some(home.as_path())
        || !md.is_dir()
        || md.uid() != uid
        || md.mode() & 0o077 != 0
    {
        return Err(unavailable());
    }
    Ok(path)
}
fn publish_selection(directory: &Path, plan: &InitialSessionPlan) -> io::Result<()> {
    // Selection DATA only. It is not CurrentOrigin, a Core/SDK receipt or Go.
    // Actual acceptance still requires Native's SAME real A/F1/current caller.
    let bytes = serde_json::to_vec(&serde_json::json!({"schema":1,"initialClaim":plan.claim()}))
        .map_err(|_| unavailable())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join("initial-selection.json"))?;
    file.write_all(&bytes)?;
    file.sync_all()
}

pub(crate) async fn run(arg0_paths: codex_arg0::Arg0DispatchPaths) -> io::Result<()> {
    // Reject shape/paths before opening any SDK/controller or reading auth.
    let directory = data_directory()?;
    let controller = codex_alab_tui::frontend_control::open().await?;
    let bound = retained_service::bind_backend(controller).await?;
    let result = async {
        codex_alab_core::native_source_launch::enter().await?;
        run_owned(arg0_paths, bound.clone(), &directory).await
    }
    .await;
    let drained = bound.shutdown().await;
    result.and(drained).map_err(|_| unavailable())
}
async fn run_owned(
    arg0_paths: codex_arg0::Arg0DispatchPaths,
    bound: retained_service::BoundBackend,
    directory: &Path,
) -> io::Result<()> {
    let codex_home = PathBuf::from(std::env::var_os("CODEX_HOME").ok_or_else(unavailable)?);
    // Honest fixture policy: no model turn/network provider; actual config,
    // AuthManager/native SDK construction and task/primary startup still run.
    let overrides = vec![
        (
            "features.shell_snapshot".into(),
            toml::Value::Boolean(false),
        ),
        ("features.plugins".into(), toml::Value::Boolean(false)),
        (
            "model".into(),
            toml::Value::String("native-retained-fixture".into()),
        ),
        (
            "model_provider".into(),
            toml::Value::String("native_retained_fixture".into()),
        ),
        (
            "model_providers.native_retained_fixture".into(),
            toml::Value::Table(toml::toml! {
                name="Native retained fixture"
                base_url="http://127.0.0.1:0/v1"
                supports_websockets=false
                requires_openai_auth=false
            }),
        ),
    ];
    let loader = LoaderOverrides::without_managed_config_for_tests();
    let source = ConfigBuilder::default()
        .codex_home(codex_home.clone())
        .fallback_cwd(Some(codex_home))
        .cli_overrides(overrides.clone())
        .loader_overrides(loader.clone());
    let mut config = source.clone().build().await.map_err(|_| unavailable())?;
    let network = EmbeddedNetworkPolicy::load(&loader).await;
    network.activate(&mut config);
    let environment = EnvironmentManager::prepare_from_env()
        .await
        .map_err(|_| unavailable())?;
    let paths = ExecServerRuntimeOptions::from_optional_paths(
        arg0_paths.codex_self_exe.clone(),
        arg0_paths.codex_linux_sandbox_exe.clone(),
    )?;
    #[cfg(target_os = "macos")]
    let paths = paths.with_allowed_symlinked_codex_home(
        codex_config::allowed_symlinked_codex_home(&config.config_layer_stack, &config.codex_home),
    );
    let environment = environment
        .build(Some(paths), network.bind(config.http_client_factory()))
        .map_err(|_| unavailable())?;
    let plan = InitialSessionPlan::new(InitialSessionKind::Start, None);
    publish_selection(directory, &plan)?;
    let args = InProcessClientStartArgs {
        required_human_approval: true,
        arg0_paths,
        config: Arc::new(config),
        cli_overrides: overrides,
        loader_overrides: loader,
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        embedded_network_policy: network,
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(environment),
        config_warnings: Vec::new(),
        session_source: codex_protocol::protocol::SessionSource::Cli,
        enable_codex_api_key_env: false,
        client_name: "codex-alab-native-bound-proof".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        experimental_api: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: 64,
    };
    // The existing exhaustive conversion installs the original real local
    // thread-loader occurrence. No reconstructed processor/Noop SDK grant.
    let owner = args.start_retained_ordinary_bound(bound, source, plan)?;
    let result = owner.wait().await;
    result.as_ref().as_ref().copied().map_err(|_| unavailable())
}
