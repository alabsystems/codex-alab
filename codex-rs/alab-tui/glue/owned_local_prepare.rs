// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One owned local-auth/OSS interaction phase before the final retained runtime.
//! Actual Native control/Incoming must be bound before these credential reads.
//! Source completion is neither SDK Ready, Native Go nor an input-frontier ACK.
use crate::legacy_core::config::ConfigTomlLoadResult;
use codex_alab_core::native_incoming::Incoming;
use codex_app_server_client::EmbeddedNetworkPolicy;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_login::AuthManager;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[path = "owned_local_prepare_channel.rs"]
pub mod channel;

/// Exact results/inputs of the original local bootstrap. This has no default or
/// deserializer. The caller keeps its original parsed CLI, Arg0, worktree and
/// other assembly owners; this phase does not reconstruct any of them.
pub struct BootstrapInputs {
    pub local: ConfigTomlLoadResult,
    pub codex_home: PathBuf,
    pub config_cwd: Option<codex_utils_absolute_path::AbsolutePathBuf>,
    pub cli_kv_overrides: Vec<(String, toml::Value)>,
    pub loader_overrides: LoaderOverrides,
    pub strict_config: bool,
    pub network_policy: EmbeddedNetworkPolicy,
    pub explicit_oss_provider: Option<String>,
}

type Completion = Arc<io::Result<()>>;
type ResultWatch = watch::Receiver<Option<Completion>>;
struct Resources {
    manager: Option<Arc<AuthManager>>,
    cloud: Option<codex_cloud_config::OwnedBootstrapCloudLoader>,
    stream: Option<UnixStream>,
    selection: Option<crate::oss_selection::OssProviderSelection>,
}
struct Shared {
    inputs: Arc<BootstrapInputs>,
    incoming: Incoming,
    stop: CancellationToken,
    resources: Mutex<Resources>,
    completed: watch::Sender<Option<Completion>>,
}

/// Retain accepted preparation across cancelled observations. Dropping this
/// owner withdraws Source IO, without certifying Native effect cleanup.
pub struct PreparationOwner {
    shared: Arc<Shared>,
    completion: ResultWatch,
    worker: Option<tokio::task::JoinHandle<()>>,
    worker_failure: Option<Completion>,
    handed_off: bool,
}
struct CompletionGuard {
    shared: Arc<Shared>,
    armed: bool,
}
impl CompletionGuard {
    fn finish(mut self, result: io::Result<()>) {
        self.shared.completed.send_replace(Some(Arc::new(result)));
        self.armed = false;
    }
}
impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if self.armed {
            self.shared
                .completed
                .send_replace(Some(Arc::new(Err(io::Error::other(
                    "Source preparation outcome unobserved",
                )))));
        }
    }
}

/// Run only the original OSS preparation branch. Config-server OAuth keeps its
/// original foreground handling; this component refuses that branch rather
/// than creating an otherwise unused manager to make a frontend eligible.
pub fn start_oss(inputs: BootstrapInputs, incoming: Incoming) -> io::Result<PreparationOwner> {
    start_with_detection(inputs, incoming, crate::oss_selection::detect_oss_provider)
}
fn start_with_detection<F, Fut>(
    inputs: BootstrapInputs,
    incoming: Incoming,
    detection: F,
) -> io::Result<PreparationOwner>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = crate::oss_selection::OssProviderDetection> + Send + 'static,
{
    if inputs
        .local
        .config_layer_stack
        .requirements_toml()
        .config_server
        .is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "config-server preparation retains its original foreground path",
        ));
    }
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| io::Error::other("Source preparation requires Tokio"))?;
    let (completed, completion) = watch::channel(None);
    let shared = Arc::new(Shared {
        inputs: Arc::new(inputs),
        incoming,
        stop: CancellationToken::new(),
        resources: Mutex::new(Resources {
            manager: None,
            cloud: None,
            stream: None,
            selection: None,
        }),
        completed,
    });
    // Guard is inside the spawned future before its first poll. Resources stay
    // outside the polled work, including on unpolled destruction or panic.
    let guard = CompletionGuard {
        shared: shared.clone(),
        armed: true,
    };
    let state = shared.clone();
    let worker = runtime.spawn(async move {
        let result = run(state, detection).await;
        guard.finish(result);
    });
    Ok(PreparationOwner {
        shared,
        completion,
        worker: Some(worker),
        worker_failure: None,
        handed_off: false,
    })
}
async fn run<F, Fut>(shared: Arc<Shared>, detection: F) -> io::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = crate::oss_selection::OssProviderDetection>,
{
    if stopped_input(&shared) {
        return Err(stopped());
    }
    let inputs = &shared.inputs;
    // Same original TUI bootstrap auth settings and local network binding; the
    // shared factory is also the ordinary cloud loader's actual consumer.
    let auth = inputs.network_policy.bind_bootstrap_auth(
        crate::legacy_core::config::bootstrap_auth_config(&inputs.codex_home, &inputs.local)?,
    );
    let started = std::sync::atomic::AtomicBool::new(false);
    let bootstrap = async {
        started.store(true, std::sync::atomic::Ordering::Release);
        codex_cloud_config::PreparedBootstrapAuth::prepare(auth, false).await
    };
    tokio::pin!(bootstrap);
    let bootstrap = tokio::select! {
        biased;
        () = shared.stop.cancelled() => {
            if !started.load(std::sync::atomic::Ordering::Acquire) { return Err(stopped()); }
            bootstrap.await
        }
        () = shared.incoming.withdrawn() => {
            if !started.load(std::sync::atomic::Ordering::Acquire) { return Err(stopped()); }
            bootstrap.await
        }
        result = &mut bootstrap => result,
    }?;
    let manager = bootstrap.manager().clone();
    let cloud_owner = bootstrap.into_owned_loader();
    let cloud = cloud_owner.loader().clone();
    {
        let mut resources = shared
            .resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        resources.manager = Some(manager);
        resources.cloud = Some(cloud_owner);
    }
    if stopped_input(&shared) {
        return Err(stopped());
    }
    let stream = tokio::select! {
        biased;
        () = shared.stop.cancelled() => return Err(stopped()),
        () = shared.incoming.withdrawn() => return Err(stopped()),
        stream = shared.incoming.accept() => stream?.ok_or_else(stopped)?,
    };
    shared
        .resources
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .stream = Some(stream);
    let mut lease = StreamLease::take(shared.clone())?;
    let selected = tokio::select! {
        biased;
        () = shared.stop.cancelled() => return Err(stopped()),
        () = shared.incoming.withdrawn() => return Err(stopped()),
        result = async {
            // Original precedence: an explicit OSS provider skips the managed
            // reload; otherwise reload actual layers using the real loader.
            let reloaded;
            let source = if inputs.explicit_oss_provider.is_none() {
                reloaded = crate::legacy_core::config::load_config_toml_with_layer_stack(
                    &inputs.codex_home, inputs.config_cwd.as_ref(), inputs.cli_kv_overrides.clone(),
                    codex_config::ConfigLoadOptions { loader_overrides: inputs.loader_overrides.clone(),
                        strict_config: inputs.strict_config, cloud_config_bundle: cloud }).await?;
                &reloaded.config_toml
            } else { &inputs.local.config_toml };
            let detection = match crate::legacy_core::config::resolve_oss_provider(inputs.explicit_oss_provider.as_deref(), source) {
                Some(provider) => crate::oss_selection::OssProviderDetection::AutoSelected(
                    crate::oss_selection::OssProviderSelection { provider, manually_selected: false }),
                None => detection().await,
            };
            channel::select(lease.stream()?, detection).await
        } => result?,
    };
    shared
        .resources
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .selection = Some(selected);
    Ok(())
}
fn stopped_input(shared: &Shared) -> bool {
    shared.stop.is_cancelled() || shared.incoming.is_stopped()
}
struct StreamLease {
    shared: Arc<Shared>,
    stream: Option<UnixStream>,
}
impl StreamLease {
    fn take(shared: Arc<Shared>) -> io::Result<Self> {
        let stream = shared
            .resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stream
            .take();
        if stream.is_none() {
            return Err(io::Error::other("Source preparation stream unavailable"));
        }
        Ok(Self { shared, stream })
    }
    fn stream(&mut self) -> io::Result<&mut UnixStream> {
        self.stream
            .as_mut()
            .ok_or_else(|| io::Error::other("Source preparation stream unavailable"))
    }
}
impl Drop for StreamLease {
    fn drop(&mut self) {
        self.shared
            .resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stream = self.stream.take();
    }
}
// Keep the original refresh handle outside a cancelled shutdown future.
struct CloudLease {
    shared: Arc<Shared>,
    cloud: Option<codex_cloud_config::OwnedBootstrapCloudLoader>,
}
impl CloudLease {
    fn take(shared: Arc<Shared>) -> Self {
        let cloud = shared
            .resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cloud
            .take();
        Self { shared, cloud }
    }
}
impl Drop for CloudLease {
    fn drop(&mut self) {
        self.shared
            .resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cloud = self.cloud.take();
    }
}
fn stopped() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Source preparation withdrew")
}
impl PreparationOwner {
    pub async fn wait(&self) -> Completion {
        let mut completion = self.completion.clone();
        loop {
            if let Some(result) = completion.borrow_and_update().clone() {
                return result;
            }
            if completion.changed().await.is_err() {
                return Arc::new(Err(io::Error::other(
                    "Source preparation completion unavailable",
                )));
            }
        }
    }
    pub async fn shutdown(&mut self) -> Completion {
        self.shared.incoming.stop();
        self.shared.stop.cancel();
        let result = self.wait().await;
        // Keep wait custody if shutdown observation is cancelled mid-join.
        if let Some(worker) = self.worker.as_mut()
            && worker.await.is_err()
        {
            self.worker_failure = Some(Arc::new(Err(io::Error::other(
                "Source preparation worker outcome unobserved",
            ))));
        }
        self.worker.take();
        let stream = self
            .shared
            .resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stream
            .take();
        drop(stream);
        let mut cloud = CloudLease::take(self.shared.clone());
        let refresh = if let Some(cloud) = cloud.cloud.as_mut() {
            Some(cloud.shutdown().await)
        } else {
            None
        };
        // Preserve the primary preparation error while still draining refresh.
        if result.is_err() {
            return result;
        }
        if let Some(failure) = &self.worker_failure {
            return failure.clone();
        }
        if let Some(refresh) = refresh
            && refresh.is_err()
        {
            return refresh;
        }
        result
    }
    pub async fn take_ready(mut self) -> io::Result<PreparedBootstrap> {
        let result = self.wait().await;
        if let Some(worker) = self.worker.as_mut()
            && worker.await.is_err()
        {
            self.worker.take();
            self.worker_failure = Some(Arc::new(Err(io::Error::other(
                "Source preparation worker outcome unobserved",
            ))));
            let _ = self.shutdown().await;
            return Err(io::Error::other("Source preparation worker failed"));
        }
        self.worker.take();
        if let Err(error) = result.as_ref() {
            let _ = self.shutdown().await;
            return Err(io::Error::new(error.kind(), "Source preparation failed"));
        }
        let (manager, cloud, selection, stream) = {
            let mut resources = self
                .shared
                .resources
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            (
                resources.manager.take(),
                resources.cloud.take(),
                resources.selection.take(),
                resources.stream.take(),
            )
        };
        drop(stream); // Final WebSocket needs a fresh genuinely admitted delivery.
        let manager =
            manager.ok_or_else(|| io::Error::other("Source bootstrap manager missing"))?;
        let cloud = cloud.ok_or_else(|| io::Error::other("Source bootstrap loader missing"))?;
        let selection =
            selection.ok_or_else(|| io::Error::other("Source bootstrap selection missing"))?;
        self.handed_off = true;
        Ok(PreparedBootstrap {
            manager,
            cloud,
            inputs: self.shared.inputs.clone(),
            incoming: Some(self.shared.incoming.clone()),
            selection,
        })
    }
}
impl Drop for PreparationOwner {
    fn drop(&mut self) {
        if !self.handed_off {
            self.shared.incoming.stop();
            self.shared.stop.cancel();
        }
    }
}

/// Actual partial-constructor resources for completing original final policy.
/// The same manager remains retained through the final runtime lifetime; this
/// does not infer actual SDK handover or permit model/goal continuation.
pub struct PreparedBootstrap {
    manager: Arc<AuthManager>,
    cloud: codex_cloud_config::OwnedBootstrapCloudLoader,
    inputs: Arc<BootstrapInputs>,
    incoming: Option<Incoming>,
    selection: crate::oss_selection::OssProviderSelection,
}
pub struct PreparedRuntime {
    // Drop the runtime (withdraw IO) before releasing bootstrap resources.
    runtime: codex_app_server_client::RetainedRuntimeOwner,
    _bootstrap: PreparedBootstrap,
}
impl PreparedBootstrap {
    pub fn bootstrap_manager(&self) -> &Arc<AuthManager> {
        &self.manager
    }
    pub fn cloud_bundle(&self) -> &CloudConfigBundleLoader {
        self.cloud.loader()
    }
    pub fn original_inputs(&self) -> &BootstrapInputs {
        &self.inputs
    }
    pub fn provider(&self) -> &str {
        &self.selection.provider
    }
    pub fn manually_selected(&self) -> bool {
        self.selection.manually_selected
    }
    /// Call on any fallible final-policy/assembly failure before handoff. Keep
    /// this owner while observing shutdown; cancellation retains the same join.
    pub async fn shutdown(&mut self) -> Completion {
        if let Some(incoming) = &self.incoming {
            incoming.stop();
        }
        self.cloud.shutdown().await
    }
    /// Caller finishes the ORIGINAL final assembly with these real inputs and
    /// loader, then consumes that actual result through db75's complete factory.
    /// The same Incoming allocation changes its sole reader; no goal is released.
    pub async fn start_runtime(
        self,
        inputs: super::owned_local_bootstrap::PreparedLocalInputs,
    ) -> color_eyre::Result<PreparedRuntime> {
        let incoming = self
            .incoming
            .as_ref()
            .ok_or_else(|| io::Error::other("Source preparation input unavailable"))?
            .clone();
        let result = inputs.start_retained(incoming).await;
        self.complete_runtime(result).await
    }
    /// The required builder is the backend's original source builder after
    /// final real CLI/harness/loader/cloud choices, never a reconstructed one.
    pub async fn start_runtime_with_worktree_source(
        self,
        inputs: super::owned_local_bootstrap::PreparedLocalInputs,
        source: crate::legacy_core::config::ConfigBuilder,
    ) -> color_eyre::Result<PreparedRuntime> {
        let incoming = self
            .incoming
            .as_ref()
            .ok_or_else(|| io::Error::other("Source preparation input unavailable"))?
            .clone();
        let result = inputs
            .start_retained_with_worktree_source(incoming, source)
            .await;
        self.complete_runtime(result).await
    }
    // Both genuine factory paths keep the same partial bootstrap owner and
    // drain its original refresh before returning an assembly failure.
    async fn complete_runtime(
        mut self,
        result: color_eyre::Result<codex_app_server_client::RetainedRuntimeOwner>,
    ) -> color_eyre::Result<PreparedRuntime> {
        let runtime = match result {
            Ok(runtime) => runtime,
            Err(error) => {
                let cleanup = self.shutdown().await;
                if cleanup.is_err() {
                    return Err(
                        error.wrap_err("bootstrap cloud refresh cleanup remains unobserved")
                    );
                }
                return Err(error);
            }
        };
        // The actual runtime now owns the same sticky input allocation.
        self.incoming.take();
        Ok(PreparedRuntime {
            runtime,
            _bootstrap: self,
        })
    }
}
impl Drop for PreparedBootstrap {
    fn drop(&mut self) {
        if let Some(incoming) = &self.incoming {
            incoming.stop();
        }
    }
}
impl PreparedRuntime {
    pub fn runtime(&self) -> &codex_app_server_client::RetainedRuntimeOwner {
        &self.runtime
    }
    /// Finish the actual final Source worker and the original bootstrap cloud
    /// refresh. This is local task observation, never Native effects closure.
    pub async fn shutdown(&mut self) -> Completion {
        let runtime = self.runtime.shutdown().await;
        let refresh = self._bootstrap.cloud.shutdown().await;
        if runtime.is_err() {
            runtime
        } else if refresh.is_err() {
            refresh
        } else {
            runtime
        }
    }
}

#[cfg(test)]
#[path = "owned_local_prepare_tests.rs"]
mod tests;
