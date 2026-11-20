// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Full ordinary local objects, owned by the backend process that will use them.
//! No terminal, frontend manager, standalone defaults or Native readiness is made.
use super::*;
#[path = "owned_local_initial_plan.rs"]
mod initial_plan;
use codex_alab_core::native_incoming::Incoming;
use codex_config::CloudConfigBundleSnapshot;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;

/// Real original first cloud owner. OSS additionally owns its actual picker
/// interaction; other launches use the original managed constructor directly.
enum InitialCloud {
    Oss(owned_local_prepare::PreparedBootstrap),
    Managed(codex_cloud_config::ManagedBootstrapOwner),
}
impl InitialCloud {
    fn loader(&self) -> &CloudConfigBundleLoader {
        match self {
            Self::Oss(owner) => owner.cloud_bundle(),
            Self::Managed(owner) => owner.loader(),
        }
    }
    async fn shutdown(&mut self) -> Arc<std::io::Result<()>> {
        match self {
            Self::Oss(owner) => owner.shutdown().await,
            Self::Managed(owner) => owner.shutdown().await,
        }
    }
}

/// Preparation returns these actual objects, not serialized handles or a ready
/// flag. The real continuation/admission constructor must consume them once.
pub struct PreparedOrdinaryBackend {
    inputs: owned_local_bootstrap::PreparedLocalInputs,
    pub(crate) source_builder: ConfigBuilder,
    initial: Option<codex_app_server_client::InitialSessionPlan>,
    pub(crate) guards: BackendGuards,
    pub(crate) cli: Cli,
    pub(crate) harness: ConfigOverrides,
    pub(crate) selected_oss: Option<owned_local_ui_snapshot::OssChoice>,
    pub(crate) cloud_snapshot: CloudConfigBundleSnapshot,
}
pub(crate) struct BackendGuards {
    incoming: Incoming,
    first_cloud: InitialCloud,
    final_cloud: codex_cloud_config::ManagedBootstrapOwner,
    pub(crate) worktree: Option<ManagedTuiWorktree>,
    otel: Option<codex_otel::OtelProvider>,
    telemetry_shutdown: Option<futures::future::BoxFuture<'static, std::io::Result<()>>>,
    telemetry_completed: Option<Arc<std::io::Result<()>>>,
    log_db: Option<log_db::LogDbLayer>,
    state_db: Option<StateDbHandle>,
    file_log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}
impl BackendGuards {
    /// Actual final-runtime shutdown MUST precede this drain. This observes
    /// Source resources, never Native effects/cohort settlement.
    pub(crate) async fn shutdown(&mut self) -> std::io::Result<()> {
        let first = self.first_cloud.shutdown().await;
        let final_cloud = self.final_cloud.shutdown().await;
        if let Some(log_db) = &self.log_db {
            log_db.flush().await;
        }
        self.log_db.take();
        if let Some(state_db) = &self.state_db {
            state_db.close().await;
        }
        self.state_db.take();
        let telemetry = if let Some(result) = &self.telemetry_completed {
            result.clone()
        } else {
            if self.telemetry_shutdown.is_none()
                && let Some(otel) = self.otel.take()
            {
                self.telemetry_shutdown = Some(
                    otel.shutdown_with_timeout(INTERACTIVE_OTEL_SHUTDOWN_TIMEOUT)
                        .boxed(),
                );
            }
            // Borrow the same accepted shutdown future. A canceled observer
            // leaves it in this actual backend owner rather than restarting it.
            let result = match self.telemetry_shutdown.as_mut() {
                Some(shutdown) => shutdown.await,
                None => Ok(()),
            };
            let result = Arc::new(result);
            self.telemetry_completed = Some(result.clone());
            self.telemetry_shutdown.take();
            result
        };
        self.file_log_guard.take();
        if let Some(worktree) = &self.worktree {
            worktree.report_startup_failure();
        }
        first.as_ref().as_ref().map_err(|error| {
            std::io::Error::new(error.kind(), "initial cloud cleanup unobserved")
        })?;
        final_cloud
            .as_ref()
            .as_ref()
            .map_err(|error| std::io::Error::new(error.kind(), "final cloud cleanup unobserved"))?;
        telemetry
            .as_ref()
            .as_ref()
            .copied()
            .map_err(|error| std::io::Error::new(error.kind(), "original telemetry cleanup failed"))
    }
}

impl Drop for BackendGuards {
    fn drop(&mut self) {
        // Withdrawal only. The async main owner must observe real drains;
        // destruction supplies no completed-cleanup/Native effects claim.
        self.incoming.stop();
    }
}

fn current(incoming: &Incoming) -> std::io::Result<()> {
    if incoming.is_stopped() || incoming.withdrawn().now_or_never().is_some() {
        Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "ordinary backend owner withdrew",
        ))
    } else {
        Ok(())
    }
}

/// The caller is the independent backend main owner, not a cancellable HTTP/UI
/// observer. Actual private binding has already happened before this function.
/// Configuration-server OAuth stays in the original foreground helper; its
/// original loader independently checks the real stored authorization again.
pub async fn prepare(
    cli: Cli,
    arg0_paths: Arg0DispatchPaths,
    loader: LoaderOverrides,
    incoming: Incoming,
) -> std::io::Result<PreparedOrdinaryBackend> {
    prepare_inner(cli, arg0_paths, loader, incoming, None).await
}
/// Required actual physical owner from original backend Native dispatch. The
/// ordinary plan is never advertised by the legacy pure-Gate component path.
pub struct PreparedBoundOrdinaryBackend {
    prepared: PreparedOrdinaryBackend,
    bound: codex_alab_tui::frontend_control::retained_service::BoundBackend,
}
pub async fn prepare_bound(
    cli: Cli,
    arg0_paths: Arg0DispatchPaths,
    loader: LoaderOverrides,
    bound: codex_alab_tui::frontend_control::retained_service::BoundBackend,
) -> std::io::Result<PreparedBoundOrdinaryBackend> {
    let input = bound.incoming();
    let mut work = Box::pin(
        AssertUnwindSafe(async {
            // SAME original constructor stage, after physical backend binding and
            // before any config/auth reader. Its actual Native denial never falls back.
            codex_alab_core::native_source_launch::enter().await?;
            prepare_inner(cli, arg0_paths, loader, input.clone(), Some(&bound)).await
        })
        .catch_unwind(),
    );
    let result = work.as_mut().await;
    let destroyed = catch_unwind(AssertUnwindSafe(|| drop(work)));
    match (result, destroyed) {
        (Ok(Ok(prepared)), Ok(())) => Ok(PreparedBoundOrdinaryBackend { prepared, bound }),
        (result, _) => {
            input.stop();
            bound.stop_goal_admission();
            let error = match result {
                Ok(Err(error)) => error,
                Ok(Ok(mut prepared)) => {
                    let _ = prepared.guards.shutdown().await;
                    std::io::Error::other("bound preparation destruction unobserved")
                }
                Err(_) => {
                    input.fail(std::io::Error::other(
                        "bound preparation outcome unobserved",
                    ));
                    std::io::Error::other("bound preparation panicked")
                }
            };
            let _ = bound.shutdown().await;
            Err(error)
        }
    }
}
impl PreparedBoundOrdinaryBackend {
    pub async fn start(self) -> std::io::Result<OrdinaryBackendRuntime> {
        self.prepared
            .start_owned(OrdinaryConstruction::Bound(self.bound))
            .await
    }
}
async fn prepare_inner(
    mut cli: Cli,
    arg0_paths: Arg0DispatchPaths,
    loader: LoaderOverrides,
    incoming: Incoming,
    bound: Option<&codex_alab_tui::frontend_control::retained_service::BoundBackend>,
) -> std::io::Result<PreparedOrdinaryBackend> {
    current(&incoming)?;
    if bound.is_some_and(|actual| actual.incoming() != incoming) {
        return Err(std::io::Error::other(
            "bound preparation changed actual input",
        ));
    }
    let target = AppServerTarget::Embedded; // Owned local PROCESS policy, never a remote workspace cast.
    if is_workload_identity_selected() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "workload-identity startup retains its original target",
        ));
    }
    if cli.shared.worktree && (cli.fork_picker || cli.fork_last) {
        return Err(std::io::Error::other(
            "`codex fork --worktree` requires an explicit session ID",
        ));
    }
    let strict_config = cli.strict_config;
    let (sandbox, approval) = owned_local_launch_policy::permissions(&cli);
    let (mut cli_kv_overrides, _) = owned_local_launch_policy::config_overrides(&mut cli)?;
    let codex_home = find_codex_home()?.to_path_buf();
    let loader = owned_local_launch_policy::profile(&cli, &codex_home, loader);
    let network = codex_app_server_client::EmbeddedNetworkPolicy::load(&loader).await;
    current(&incoming)?;
    let environment = if should_load_configured_environments(&loader, &target) {
        EnvironmentManager::prepare_from_codex_home(&codex_home).await
    } else {
        EnvironmentManager::prepare_from_env().await
    }
    .map_err(std::io::Error::other)?;
    current(&incoming)?;
    if cli.shared.worktree && environment.default_environment_is_remote() {
        return Err(std::io::Error::other(
            "`--worktree` is only supported for local sessions",
        ));
    }
    let cwd = config_cwd_for_app_server_target(
        cli.cwd.as_deref(),
        &target,
        environment.default_environment_is_remote(),
    )?;
    let local = owned_local_backend_policy::bootstrap(
        &codex_home,
        cwd.as_ref(),
        cli_kv_overrides.clone(),
        loader.clone(),
        strict_config,
        CloudConfigBundleLoader::default(),
    )
    .await?; // Original initial local load intentionally precedes cloud.
    current(&incoming)?;
    let (mut first_cloud, selected_oss) = if cli.oss {
        let mut owner = owned_local_prepare::start_oss(
            owned_local_prepare::BootstrapInputs {
                local,
                codex_home: codex_home.clone(),
                config_cwd: cwd.clone(),
                cli_kv_overrides: cli_kv_overrides.clone(),
                loader_overrides: loader.clone(),
                strict_config,
                network_policy: network.clone(),
                explicit_oss_provider: cli.oss_provider.clone(),
            },
            incoming.clone(),
        )?;
        let result = owner.wait().await;
        if result.is_err() {
            let _ = owner.shutdown().await;
            return Err(std::io::Error::other("actual OSS preparation failed"));
        }
        let prepared = owner.take_ready().await?;
        let choice = owned_local_ui_snapshot::OssChoice {
            provider: prepared.provider().into(),
            manually_selected: prepared.manually_selected(),
        };
        (InitialCloud::Oss(prepared), Some(choice))
    } else {
        let auth = network.bind_bootstrap_auth(
            target.auth_config_for_cloud_loader(bootstrap_auth_config(&codex_home, &local)?),
        );
        (
            InitialCloud::Managed(
                codex_cloud_config::ManagedBootstrapOwner::prepare(
                    auth,
                    local.config_layer_stack.requirements_toml(),
                    false,
                )
                .await?,
            ),
            None,
        )
    };
    // Everything after a real cloud worker exists runs under common cleanup.
    let mut final_cloud: Option<codex_cloud_config::ManagedBootstrapOwner> = None;
    let mut worktree: Option<ManagedTuiWorktree> = None;
    let mut state_db: Option<StateDbHandle> = None;
    let mut log_db: Option<log_db::LogDbLayer> = None;
    let mut otel: Option<codex_otel::OtelProvider> = None;
    let mut file_log_guard: Option<tracing_appender::non_blocking::WorkerGuard> = None;
    let mut work = Box::pin(AssertUnwindSafe(async {
        current(&incoming)?;
        if let Some(choice) = &selected_oss {
            cli_kv_overrides.push(("model_provider".into(), toml::Value::String(choice.provider.clone())));
        }
        let selected = selected_oss.as_ref().map(|choice| choice.provider.as_str());
        let mut harness = alab_local_config::configuration_overrides(&cli, &arg0_paths, approval, sandbox,
            cli.cwd.clone(), selected);
        let mut config = load_config_with_worktree_source_policy(cli_kv_overrides.clone(), harness.clone(), loader.clone(),
            first_cloud.loader().clone(), strict_config, None, None).await?;
        network.activate(&mut config);
        current(&incoming)?;
        final_cloud = Some(codex_cloud_config::ManagedBootstrapOwner::prepare(
            network.bind_bootstrap_auth(target.auth_config_for_cloud_loader(config.auth_config())),
            config.config_layer_stack.requirements_toml(), false).await?);
        let mut cloud = final_cloud.as_ref().ok_or_else(|| std::io::Error::other("final cloud owner missing"))?.loader().clone();
        current(&incoming)?;
        if cli.shared.worktree {
            let (destination, bundle, managed) = worktree_startup::prepare(&mut cli, config, &mut harness,
                cli_kv_overrides.clone(), loader.clone(), strict_config, &target, &arg0_paths, cloud, &network)
                .await.map_err(|error| std::io::Error::other(error.to_string()))?;
            config = destination; cloud = bundle; worktree = Some(managed);
            network.activate(&mut config);
        }
        current(&incoming)?;
        // Freeze exactly once and use this SAME actual snapshot for both final
        // backend Config and UI DATA; retain the LIVE backend loader for subsequent
        // original runtime/source-policy reloads, including real binding/EMA.
        let cloud_snapshot = cloud.get_snapshot().await;
        let frozen = CloudConfigBundleLoader::from_snapshot(cloud_snapshot.clone());
        config = load_config_with_worktree_source_policy(cli_kv_overrides.clone(), harness.clone(), loader.clone(),
            frozen.clone(), strict_config, None, worktree.as_ref()).await?;
        network.activate(&mut config);
        current(&incoming)?;
        let runtime_paths = ExecServerRuntimeOptions::from_optional_paths(arg0_paths.codex_self_exe.clone(), arg0_paths.codex_linux_sandbox_exe.clone())?;
        #[cfg(target_os = "macos")]
        let runtime_paths = runtime_paths.with_allowed_symlinked_codex_home(
            codex_config::allowed_symlinked_codex_home(&config.config_layer_stack, &config.codex_home));
        let environment_manager = Arc::new(environment.build(Some(runtime_paths), target.environment_http_client_factory(&config, &network))
            .map_err(std::io::Error::other)?);
        current(&incoming)?;
        otel = match catch_unwind(AssertUnwindSafe(|| codex_app_server_client::build_otel_provider(&config,
            codex_alab_core::version::running_version(env!("CARGO_PKG_VERSION")), None, true))) {
            Ok(Ok(provider)) => provider,
            Ok(Err(error)) => {
                config.startup_warnings.push(format!("Could not create otel exporter: {error}"));
                None
            }
            Err(_) => {
                config.startup_warnings.push("Could not create otel exporter: panicked during initialization".into());
                None
            }
        };
        // Same original process recorder; warnings use existing actual startup
        // notifications once the real frontend initializes its connection.
        let _ = owned_local_logging::process_metrics(otel.as_ref(), originator().value.as_str());
        state_db = init_state_db_for_app_server_target(&config, &target).await?;
        current(&incoming)?;
        set_default_client_residency_requirement(config.enforce_residency.value());
        if let Some(warning) = add_dir_warning_message(&cli.add_dir, &config.permissions.effective_permission_profile(), config.cwd.as_path()) {
            return Err(std::io::Error::other(format!("Error adding directories: {warning}")));
        }
        // Preserve the original direct restriction read (including env=true,
        // unrestricted initial auth selection and logout-on-violation). This
        // occurs in the actual bound backend, never a foreground manager.
        // Its genuine Native reader scope remains an activation prerequisite.
        enforce_login_restrictions(&config.auth_config()).await?;
        current(&incoming)?;
        let prepared_file_logging = owned_local_logging::prepare_file(&config)?;
        let feedback = codex_feedback::CodexFeedback::new();
        if let Some(choice) = &selected_oss { codex_utils_oss::ensure_oss_provider_ready(&choice.provider, &config).await?; }
        current(&incoming)?;
        log_db = state_db.clone().map(|state| log_db::start(state, Arc::new(feedback.clone())));
        file_log_guard = owned_local_logging::install(prepared_file_logging, &feedback, log_db.clone(), otel.as_ref());
        let source_builder = owned_worktree_source_builder::create(&config.codex_home, cli_kv_overrides.clone(),
            harness.clone(), loader.clone(), cloud.clone());
        // Original worktree preparation may canonicalize the explicit fork ID.
        // Freeze only method-vs-picker intent, preserving all actual CLI fields.
        let initial = bound.map(|_| codex_app_server_client::InitialSessionPlan::new(initial_plan::kind(&cli),
            worktree.as_ref().map(ManagedTuiWorktree::backend_binding)));
        let mut ui_snapshot = owned_local_ui_snapshot::UiSnapshot::from_backend(&cloud_snapshot, config.cwd.clone(), selected_oss.clone())?;
        if let Some(initial) = &initial { ui_snapshot = ui_snapshot.with_initial_session(initial.claim())?; }
        // This endpoint carries configuration DATA only. It is neither the
        // earlier picker connection nor the final WebSocket input endpoint.
        let mut stream = tokio::select! {
            biased;
            () = incoming.withdrawn() => return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "ordinary backend owner withdrew")),
            stream = incoming.accept() => stream?.ok_or_else(|| std::io::Error::other("UI snapshot endpoint unavailable"))?,
        };
        owned_local_ui_snapshot::write(&mut stream, &ui_snapshot).await?;
        drop(stream);
        current(&incoming)?;
        Ok::<_, std::io::Error>((owned_local_bootstrap::PreparedLocalInputs {
            arg0_paths: arg0_paths.clone(), config, cli_kv_overrides: cli_kv_overrides.clone(), loader_overrides: loader.clone(), strict_config,
            cloud_config_bundle: cloud, feedback, log_db: log_db.clone(), state_db: state_db.clone(),
            environment_manager, embedded_network_policy: network,
        }, source_builder, initial, harness, cloud_snapshot))
    }).catch_unwind());
    let result = work.as_mut().await;
    let destroyed = catch_unwind(AssertUnwindSafe(|| drop(work)));
    let prepared = match (result, destroyed) {
        (Ok(result), Ok(())) => result,
        _ => Err(std::io::Error::other(
            "ordinary backend preparation outcome unobserved",
        )),
    };
    let error = match prepared {
        Ok((inputs, source_builder, initial, harness, cloud_snapshot)) => {
            if let Some(final_cloud) = final_cloud.take() {
                return Ok(PreparedOrdinaryBackend {
                    inputs,
                    source_builder,
                    initial,
                    cli,
                    harness,
                    selected_oss,
                    cloud_snapshot,
                    guards: BackendGuards {
                        incoming,
                        first_cloud,
                        final_cloud,
                        worktree,
                        otel,
                        telemetry_shutdown: None,
                        telemetry_completed: None,
                        log_db,
                        state_db,
                        file_log_guard,
                    },
                });
            }
            std::io::Error::other("successful preparation lost final cloud owner")
        }
        Err(error) => error,
    };
    // Preserve actual resource custody on every failed/panicking preparation.
    // Error detail stays private at this process boundary, not in Native labels.
    incoming.stop();
    let _ = first_cloud.shutdown().await;
    if let Some(owner) = final_cloud.as_mut() {
        let _ = owner.shutdown().await;
    }
    if let Some(log_db) = &log_db {
        log_db.flush().await;
    }
    drop(log_db);
    if let Some(state_db) = state_db {
        state_db.close().await;
    }
    if let Some(otel) = otel {
        let _ = otel
            .shutdown_with_timeout(INTERACTIVE_OTEL_SHUTDOWN_TIMEOUT)
            .await;
    }
    drop(file_log_guard);
    if let Some(worktree) = &worktree {
        worktree.report_startup_failure();
    }
    Err(error)
}

/// Source runtime plus the SAME preparation owners. The Native backend main
/// separately retains BoundBackend and the actual continuation Owner/Hold.
/// No frontend observer owns this object or its whole startup/shutdown future.
pub struct OrdinaryBackendRuntime {
    runtime: codex_app_server_client::RetainedRuntimeOwner,
    guards: BackendGuards,
    cli: Cli,
    harness: ConfigOverrides,
    selected_oss: Option<owned_local_ui_snapshot::OssChoice>,
    cloud_snapshot: CloudConfigBundleSnapshot,
}
impl PreparedOrdinaryBackend {
    /// This consumes all original objects once through the original factory.
    /// The caller owns this entire future, not a cancellable UI/HTTP observer.
    pub async fn start(
        self,
        gate: codex_alab_core::retained_goal_gate::Gate,
    ) -> std::io::Result<OrdinaryBackendRuntime> {
        self.start_owned(OrdinaryConstruction::Gate(gate)).await
    }
    async fn start_owned(
        self,
        construction: OrdinaryConstruction,
    ) -> std::io::Result<OrdinaryBackendRuntime> {
        let Self {
            inputs,
            source_builder,
            initial,
            mut guards,
            cli,
            harness,
            selected_oss,
            cloud_snapshot,
        } = self;
        let incoming = guards.incoming.clone();
        let mut work = Box::pin(
            AssertUnwindSafe(async move {
                match construction {
                    OrdinaryConstruction::Gate(gate) => {
                        inputs
                            .start_retained_with_worktree_source_and_goal_gate(
                                incoming,
                                source_builder,
                                gate,
                            )
                            .await
                    }
                    OrdinaryConstruction::Bound(bound) => {
                        if incoming != bound.incoming() {
                            return Err(color_eyre::eyre::eyre!(
                                "ordinary constructor changed actual backend input"
                            ));
                        }
                        let initial = initial.ok_or_else(|| {
                            color_eyre::eyre::eyre!(
                                "bound ordinary constructor has no original plan"
                            )
                        })?;
                        inputs
                            .start_retained_ordinary_bound(bound, source_builder, initial)
                            .await
                    }
                }
            })
            .catch_unwind(),
        );
        let result = work.as_mut().await;
        let destroyed = catch_unwind(AssertUnwindSafe(|| drop(work)));
        match (result, destroyed) {
            (Ok(Ok(runtime)), Ok(())) => Ok(OrdinaryBackendRuntime {
                runtime,
                guards,
                cli,
                harness,
                selected_oss,
                cloud_snapshot,
            }),
            (result, destroyed) => {
                guards.incoming.stop();
                let error = match result {
                    Ok(Ok(runtime)) => {
                        // An actual returned owner remains in custody even if
                        // the assembly future's final destruction panicked.
                        let _ = runtime.shutdown().await;
                        std::io::Error::other("ordinary assembly destruction was unobserved")
                    }
                    Ok(Err(error)) => std::io::Error::other(error.to_string()),
                    Err(_) => std::io::Error::other("ordinary assembly panicked"),
                };
                let _ = destroyed;
                let _ = guards.shutdown().await;
                Err(error)
            }
        }
    }
}
impl OrdinaryBackendRuntime {
    pub fn runtime(&self) -> &codex_app_server_client::RetainedRuntimeOwner {
        &self.runtime
    }
    pub fn original_cli(&self) -> &Cli {
        &self.cli
    }
    pub fn original_harness(&self) -> &ConfigOverrides {
        &self.harness
    }
    pub fn cloud_snapshot(&self) -> &CloudConfigBundleSnapshot {
        &self.cloud_snapshot
    }
    pub fn selected_oss(&self) -> Option<(&str, bool)> {
        self.selected_oss
            .as_ref()
            .map(|choice| (choice.provider.as_str(), choice.manually_selected))
    }
    /// Source runtime drain precedes the original refresh/DB/telemetry owners.
    /// Caller retains/polls this whole backend-main future through completion;
    /// dropping it makes no observed-cleanup or Native settlement claim.
    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        let runtime = self.runtime.shutdown().await;
        let resources = self.guards.shutdown().await;
        runtime.as_ref().as_ref().map_err(|error| {
            std::io::Error::new(error.kind(), "ordinary Source runtime cleanup failed")
        })?;
        resources
    }
}

#[cfg(test)]
#[path = "owned_local_backend_prepare_tests.rs"]
mod tests;

// The existing component constructor remains available; only Bound is the
// full ordinary Native path. Both use the SAME original assembly/cleanup body.
enum OrdinaryConstruction {
    Gate(codex_alab_core::retained_goal_gate::Gate),
    Bound(codex_alab_tui::frontend_control::retained_service::BoundBackend),
}
