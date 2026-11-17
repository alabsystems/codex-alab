// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Complete embedded inputs with an independently owned native-stream host.
//! This owner tracks runtime/IO custody, not native SDK admission or Source
//! effect settlement. Frontend EOF never ends the host's own lifetime.

use super::InProcessStartArgs;
use super::bootstrap;
use crate::analytics_utils::analytics_events_client_from_config;
use crate::config_manager::ConfigManager;
use crate::message_processor::MessageProcessor;
use crate::message_processor::MessageProcessorArgs;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingMessageSender;
use crate::plugin_config_reload::PluginStartupConfig;
use codex_alab_core::native_incoming::Incoming;
use codex_analytics::AnalyticsEventsClient;
use codex_analytics::AppServerRpcTransport;
use codex_app_server_protocol::InitializeParams;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Poll;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[path = "owned_local_runtime_dispatch.rs"]
mod dispatch;

/// Exact caller declarations for a frontend connection, never SDK authority.
#[derive(Clone)]
pub struct FrontendMetadata {
    pub initialize: InitializeParams,
    pub channel_capacity: usize,
}

type Completion = Arc<io::Result<()>>;
type CompletionWatch = watch::Receiver<Option<Completion>>;

/// One host lifetime, independent of every WebSocket client's EOF. Cancelling
/// wait/shutdown observation cannot discard or restart its accepted startup.
/// A result observes this host's completion, never native resource settlement.
pub struct RetainedRuntimeOwner {
    metadata: FrontendMetadata,
    incoming: Incoming,
    shutdown: CancellationToken,
    completion: CompletionWatch,
    bound: Option<codex_alab_aterm::frontend_control::retained_service::BoundBackend>,
}
impl RetainedRuntimeOwner {
    pub fn frontend_metadata(&self) -> &FrontendMetadata {
        &self.metadata
    }

    pub async fn wait(&self) -> Completion {
        let result = wait_completion(self.completion.clone()).await;
        // The SAME physical bind also survives early preparation failure or
        // worker loss before a Prepared/Binding existed. Observe its original
        // receive/Goal workers after Source completion on every bound path.
        // The SDK borrows/restores those same handles across cancelled waits.
        if let Some(bound) = &self.bound
            && let Err(error) = bound.shutdown().await
        {
            return Arc::new(Err(error));
        }
        result
    }

    pub async fn shutdown(&self) -> Completion {
        self.request_shutdown();
        self.wait().await
    }

    fn request_shutdown(&self) {
        // Close actual admission synchronously, even if the worker has never
        // been polled or its preparation is waiting on managed/bootstrap IO.
        self.incoming.stop();
        if let Some(bound) = &self.bound {
            // Restrict automatic/new SDK entry now, but preserve the SAME
            // controller for exact cancellation-only registry cleanup.
            bound.begin_source_shutdown();
        }
        self.shutdown.cancel();
    }
}
impl Drop for RetainedRuntimeOwner {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

async fn wait_completion(mut completion: CompletionWatch) -> Completion {
    loop {
        if let Some(result) = completion.borrow_and_update().clone() {
            return result;
        }
        if completion.changed().await.is_err() {
            // Runtime destruction can discard the worker itself. This error
            // reports missing observation, never successful cleanup/settlement.
            return Arc::new(Err(io::Error::other(
                "retained runtime completion unavailable",
            )));
        }
    }
}

fn input_result(incoming: &Incoming) -> io::Result<()> {
    match incoming.failure() {
        Some(error) => Err(io::Error::new(error.kind(), error)),
        None => Ok(()),
    }
}

/// Own all caller inputs and the exact provider before any asynchronous work.
/// Preparation uses the existing embedded bootstrap; no Config is substituted.
/// A runtime must already exist, as for the original embedded constructor.
pub fn start_retained(
    args: InProcessStartArgs,
    incoming: Incoming,
) -> io::Result<RetainedRuntimeOwner> {
    start_retained_with_source(args, incoming, None, None, None, None)
}

/// The backend supplies its genuine original source builder after final local
/// preparation. Existing no-builder hosts expose no worktree capability.
pub fn start_retained_with_worktree_source(
    args: InProcessStartArgs,
    incoming: Incoming,
    source: codex_core::config::ConfigBuilder,
) -> io::Result<RetainedRuntimeOwner> {
    start_retained_with_source(
        args,
        incoming,
        Some(codex_alab_task_worktree::SourceConfig::new(source)),
        None,
        None,
        None,
    )
}

/// Same full inputs plus the caller's actual held Source scheduling gate.
/// Native permission/Go remain with the real backend continuation owner.
pub fn start_retained_with_worktree_source_and_goal_gate(
    args: InProcessStartArgs,
    incoming: Incoming,
    source: codex_core::config::ConfigBuilder,
    gate: codex_alab_core::retained_goal_gate::Gate,
) -> io::Result<RetainedRuntimeOwner> {
    start_retained_with_source(
        args,
        incoming,
        Some(codex_alab_task_worktree::SourceConfig::new(source)),
        Some(gate),
        None,
        None,
    )
}

/// Exact original full inputs plus the backend-issued initial selection
/// resource. Gate/Incoming/claim carry no Native permission or readiness.
pub fn start_retained_ordinary(
    args: InProcessStartArgs,
    incoming: Incoming,
    source: codex_core::config::ConfigBuilder,
    gate: codex_alab_core::retained_goal_gate::Gate,
    initial: super::InitialSessionPlan,
) -> io::Result<RetainedRuntimeOwner> {
    start_retained_with_source(
        args,
        incoming,
        Some(codex_alab_task_worktree::SourceConfig::new(source)),
        Some(gate),
        Some(initial),
        None,
    )
}

/// The physical bind owns the ONE goal owner. Its exact Incoming and Gate
/// cannot be supplied independently or reconstructed from selection metadata.
#[cfg(unix)]
pub fn start_retained_ordinary_bound(
    args: InProcessStartArgs,
    bound: codex_alab_aterm::frontend_control::retained_service::BoundBackend,
    source: codex_core::config::ConfigBuilder,
    initial: super::InitialSessionPlan,
) -> io::Result<RetainedRuntimeOwner> {
    let incoming = bound.incoming();
    let gate = bound.goal_gate();
    let mut owner = start_retained_with_source(
        args,
        incoming,
        Some(codex_alab_task_worktree::SourceConfig::new(source)),
        Some(gate),
        Some(initial),
        Some(bound.clone()),
    )?;
    owner.bound = Some(bound); // Custody even if preparation never produces a processor.
    Ok(owner)
}

fn start_retained_with_source(
    args: InProcessStartArgs,
    incoming: Incoming,
    source: Option<codex_alab_task_worktree::SourceConfig>,
    gate: Option<codex_alab_core::retained_goal_gate::Gate>,
    initial: Option<super::InitialSessionPlan>,
    bound: Option<codex_alab_aterm::frontend_control::retained_service::BoundBackend>,
) -> io::Result<RetainedRuntimeOwner> {
    let metadata = FrontendMetadata {
        initialize: args.initialize.clone(),
        channel_capacity: args.channel_capacity,
    };
    let preparing_input = incoming.clone();
    start_owned(
        metadata,
        incoming,
        move || async move {
            let prepared = match (gate, initial) {
                (None, None) => prepare(args, preparing_input.clone()).await?,
                (Some(gate), None) => {
                    prepare_with_goal_gate(args, preparing_input.clone(), gate).await?
                }
                (gate, Some(initial)) => {
                    prepare_inner(args, preparing_input.clone(), gate, Some(initial), bound).await?
                }
            };
            if let (Some(prepared), Some(source)) = (&prepared, source)
                && let Err(error) = prepared
                    .processor
                    .alab_install_worktree_source(&prepared.task_mcp, source)
            {
                preparing_input.fail(error);
            }
            Ok(prepared)
        },
        |prepared, incoming, stop| async move {
            match prepared {
                Some(prepared) => dispatch::run(prepared, incoming, stop).await,
                None => input_result(&incoming),
            }
        },
    )
}

fn start_owned<P, Preparation, T, R, Runtime>(
    metadata: FrontendMetadata,
    incoming: Incoming,
    prepare: P,
    run: R,
) -> io::Result<RetainedRuntimeOwner>
where
    P: FnOnce() -> Preparation + Send + 'static,
    Preparation: Future<Output = io::Result<T>> + Send + 'static,
    T: Send + 'static,
    R: FnOnce(T, Incoming, CancellationToken) -> Runtime + Send + 'static,
    Runtime: Future<Output = io::Result<()>> + Send + 'static,
{
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| io::Error::other("retained runtime requires an active Tokio runtime"))?;
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let owner_incoming = incoming.clone();
    let completion_incoming = incoming.clone();
    let (completion, observed) = watch::channel(None);
    // There is only one worker: no monitor can panic and detach a hidden child.
    // Its accepted work outlives observers, and its result remains stored.
    runtime.spawn(async move {
        let running = AtomicBool::new(false);
        let terminal_stop = stop.clone();
        let work = run_owned(prepare, run, incoming, stop, &running);
        let result = capture_work(work, &running).await;
        completion_incoming.stop();
        terminal_stop.cancel();
        let result = result.and_then(|()| input_result(&completion_incoming));
        completion.send_replace(Some(Arc::new(result)));
    });
    Ok(RetainedRuntimeOwner {
        metadata,
        incoming: owner_incoming,
        shutdown,
        completion: observed,
        bound: None,
    })
}

async fn run_owned<P, Preparation, T, R, Runtime>(
    prepare: P,
    run: R,
    incoming: Incoming,
    stop: CancellationToken,
    running: &AtomicBool,
) -> io::Result<()>
where
    P: FnOnce() -> Preparation,
    Preparation: Future<Output = io::Result<T>>,
    R: FnOnce(T, Incoming, CancellationToken) -> Runtime,
    Runtime: Future<Output = io::Result<()>>,
{
    let started = AtomicBool::new(false);
    let preparation = async {
        started.store(true, Ordering::Release);
        prepare().await
    };
    tokio::pin!(preparation);
    let prepared = tokio::select! {
        biased;
        () = incoming.withdrawn() => {
            stop.cancel();
            if !started.load(Ordering::Acquire) {
                return input_result(&incoming);
            }
            // Preserve the exact partly-polled startup until its real outcome.
            // Dropping its waiter is not evidence that managed/SDK IO settled.
            preparation.await
        }
        () = stop.cancelled() => {
            incoming.stop();
            if !started.load(Ordering::Acquire) {
                return input_result(&incoming);
            }
            preparation.await
        }
        result = &mut preparation => result,
    };
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            input_result(&incoming)?;
            return Err(error);
        }
    };
    running.store(true, Ordering::Release);
    // The dispatcher receives the same closed input/token if preparation was
    // withdrawn. It must clean Prepared without starting a transport acceptor.
    run(prepared, incoming, stop).await
}

/// Catch polling and final future destruction separately. A polling panic must
/// not destroy its future while already unwinding, and no panic payload is
/// formatted into the stored public IO error. The dispatcher remains responsible
/// for owning/joining its children before its future releases them.
async fn capture_work<F>(work: F, running: &AtomicBool) -> io::Result<()>
where
    F: Future<Output = io::Result<()>>,
{
    let mut work = Some(Box::pin(work));
    let outcome = poll_fn(|context| {
        let polled = catch_unwind(AssertUnwindSafe(|| match work.as_mut() {
            Some(work) => work.as_mut().poll(context),
            None => Poll::Pending,
        }));
        match polled {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(result)) => Poll::Ready(Ok(result)),
            Err(_) => Poll::Ready(Err(())),
        }
    })
    .await;
    if catch_unwind(AssertUnwindSafe(|| drop(work.take()))).is_err() {
        return Err(io::Error::other(
            "retained runtime work destruction panicked",
        ));
    }
    match outcome {
        Ok(result) => result,
        Err(()) if running.load(Ordering::Acquire) => {
            Err(io::Error::other("retained runtime task panicked"))
        }
        Err(()) => Err(io::Error::other("retained runtime preparation panicked")),
    }
}

struct Prepared {
    frontier: Option<Arc<crate::request_processors::RetainedFrontierBinding>>,
    initial_session: Option<Arc<crate::request_processors::InitialSessionOperation>>,
    task_mcp: crate::message_processor::alab_task_mcp::OwnedTaskMcpHost,
    processor: Arc<MessageProcessor>,
    outgoing: Arc<OutgoingMessageSender>,
    outgoing_rx: mpsc::Receiver<OutgoingEnvelope>,
    startup_warnings_rx: mpsc::UnboundedReceiver<String>,
    analytics_flush: AnalyticsEventsClient,
    capacity: usize,
    compression: codex_app_server_transport::WebSocketCompression,
}

/// Probe the actual owner's withdrawal future once at a startup boundary. A
/// healthy owner must not block preparation; this is not a readiness/grant flag.
async fn startup_withdrawn(incoming: &Incoming) -> bool {
    tokio::select! {
        biased;
        () = incoming.withdrawn() => true,
        () = std::future::ready(()) => false,
    }
}

async fn prepare(args: InProcessStartArgs, incoming: Incoming) -> io::Result<Option<Prepared>> {
    prepare_inner(args, incoming, None, None, None).await
}

async fn prepare_with_goal_gate(
    args: InProcessStartArgs,
    incoming: Incoming,
    gate: codex_alab_core::retained_goal_gate::Gate,
) -> io::Result<Option<Prepared>> {
    prepare_inner(args, incoming, Some(gate), None, None).await
}

async fn prepare_inner(
    args: InProcessStartArgs,
    incoming: Incoming,
    gate: Option<codex_alab_core::retained_goal_gate::Gate>,
    initial: Option<super::InitialSessionPlan>,
    bound: Option<codex_alab_aterm::frontend_control::retained_service::BoundBackend>,
) -> io::Result<Option<Prepared>> {
    if startup_withdrawn(&incoming).await {
        return Ok(None);
    }
    // Exhaustive destructuring is intentional: a future upstream input must
    // require an explicit decision here rather than silently becoming Default.
    let InProcessStartArgs {
        arg0_paths,
        mut config,
        cli_overrides,
        loader_overrides,
        strict_config,
        cloud_config_bundle,
        embedded_network_policy,
        thread_config_loader,
        feedback,
        log_db,
        state_db,
        environment_manager,
        mut config_warnings,
        session_source,
        enable_codex_api_key_env,
        initialize,
        channel_capacity,
    } = args;
    // Metadata was cloned into the owner before this worker started. Its exact
    // InitializeParams remain available to the future real frontend bridge.
    // No part of it initializes the backend globally.
    drop(initialize);

    // Same warning pass as in_process::start, not a config substitute.
    let execpolicy_warning =
        codex_core::check_execpolicy_for_warnings(&config.config_layer_stack).await;
    if startup_withdrawn(&incoming).await {
        return Ok(None);
    }
    if let Ok(Some(error)) = execpolicy_warning {
        let (path, range) = crate::exec_policy_warning_location(&error);
        config_warnings.push(codex_app_server_protocol::ConfigWarningNotification {
            summary: "Error parsing rules; custom rules not applied.".to_owned(),
            details: Some(error.to_string()),
            path,
            range,
        });
    }
    // Necessary inline duplication from in_process.rs469–493: no existing
    // prepared-runtime factory exposes these exact managed-policy steps.
    let config_manager = ConfigManager::new(
        config.codex_home.to_path_buf(),
        cli_overrides,
        loader_overrides,
        strict_config,
        cloud_config_bundle,
        arg0_paths.clone(),
        thread_config_loader,
    )
    .with_embedded_network_policy(embedded_network_policy);
    let startup_cwd =
        (!config.config_layer_stack.is_projectless()).then(|| config.cwd.to_path_buf());
    if startup_withdrawn(&incoming).await {
        return Ok(None);
    }
    let authentication =
        bootstrap::configure(&config_manager, &mut config, enable_codex_api_key_env).await;
    if startup_withdrawn(&incoming).await {
        // The already-polled bootstrap completed before its original handles
        // are dropped. Withdrawal cannot start a fresh processor afterward.
        return Ok(None);
    }
    let auth_manager = authentication?;
    let installation = codex_core::resolve_installation_id(&config.codex_home).await;
    if startup_withdrawn(&incoming).await {
        return Ok(None);
    }
    let installation_id = installation?;
    let capacity = channel_capacity.max(1);
    let runtime_auth_slot = config
        .features
        .enabled(codex_features::Feature::RuntimeAgentInstances)
        .then(codex_runtime_agent_instance::RuntimeAuthSlot::default);
    let analytics = analytics_events_client_from_config(
        Arc::clone(&auth_manager),
        config.as_ref(),
        runtime_auth_slot.clone(),
    );
    let analytics_flush = analytics.clone();
    let (outgoing_tx, outgoing_rx) = mpsc::channel(capacity);
    let outgoing = Arc::new(OutgoingMessageSender::alab_for_retained_input(
        outgoing_tx,
        analytics.clone(),
        incoming.clone(),
    ));
    let warning = crate::log_write_warning::LogWriteWarningReporter::new(
        feedback.clone(),
        &outgoing,
        &config,
    );
    if let Some(log_db) = &log_db {
        log_db.set_failure_reporter(warning.clone());
        if log_db.has_write_failure() {
            warning.notify_failure();
        }
    }
    let compression = if config
        .features
        .enabled(codex_features::Feature::AppServerWebSocketCompression)
    {
        codex_app_server_transport::WebSocketCompression::Zstd
    } else {
        codex_app_server_transport::WebSocketCompression::Plain
    };
    if startup_withdrawn(&incoming).await {
        return Ok(None);
    }
    let (task_listener, task_address) =
        crate::message_processor::alab_task_mcp::bind_listener(&incoming).await?;
    if startup_withdrawn(&incoming).await {
        return Ok(None);
    }
    let (startup_warnings_tx, startup_warnings_rx) = mpsc::unbounded_channel();
    let processor = Arc::new(MessageProcessor::new(MessageProcessorArgs {
        // These three None/Current choices preserve the existing embedded
        // constructor, whose argument type supplies no alternate providers.
        cloud_skill_provider: None,
        outgoing: outgoing.clone(),
        analytics_events_client: analytics,
        arg0_paths,
        config,
        startup_cwd,
        config_manager,
        environment_manager,
        feedback,
        log_db,
        state_db,
        config_warnings,
        startup_warnings_tx,
        session_source,
        user_verification: Arc::new(crate::user_verification::Service::new(auth_manager.clone())),
        auth_manager,
        runtime_auth_slot,
        installation_id,
        code_mode_session_provider: None,
        // Actual transport accounting changes; no input Config object does.
        rpc_transport: AppServerRpcTransport::Websocket,
        remote_control_handle: None,
        plugin_startup_tasks: Some(PluginStartupConfig::Current),
    }));
    if let Some(gate) = gate
        && let Err(error) = processor.alab_bind_retained_goal_gate(gate)
    {
        // Keep the actual partially prepared processor in common dispatch
        // cleanup; no early error drops its background tasks unobserved.
        incoming.fail(io::Error::other(error));
    }
    let initial_session = initial.map(|plan| {
        let (actual, installed) = match bound.as_ref() {
            Some(bound) => processor.alab_make_initial_session_bound(plan, bound.clone()),
            None => processor.alab_make_initial_session(plan, incoming.clone()),
        };
        if let Err(error) = installed {
            incoming.fail(error);
        }
        actual
    });
    let frontier = bound.map(|bound| {
        let (actual, installed) = processor.alab_make_frontier(bound);
        if let Err(error) = installed {
            incoming.fail(error);
        }
        actual
    });
    // Same Gate and actual operation owner precede every task/root publication.
    // Failed installation stays in common cleanup; no acceptor opens.
    let (task_mcp, installed) =
        processor.alab_make_task_mcp_host(task_listener, task_address, incoming.clone());
    if let Err(error) = installed {
        incoming.fail(error);
    }
    Ok(Some(Prepared {
        frontier,
        initial_session,
        task_mcp,
        processor,
        outgoing,
        outgoing_rx,
        startup_warnings_rx,
        analytics_flush,
        capacity,
        compression,
    }))
}

#[cfg(test)]
#[path = "owned_local_runtime_tests.rs"]
mod tests;
