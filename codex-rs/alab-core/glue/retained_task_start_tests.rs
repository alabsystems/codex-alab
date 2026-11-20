// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Core startup component tests, no Native SDK admission
//! or create_thread/first-turn/inheritance claim. Mounted as this module's child.

use super::*;
use crate::config_manager::ConfigManager;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingMessage;
use crate::request_processors::thread_goal_processor::ThreadGoalRequestProcessor;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_sample::SampleResponseRegistry;
use crate::thread_state::ThreadStateManager;
use crate::thread_status::ThreadWatchManager;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use app_test_support::MockResponsesConfig;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_analytics::AnalyticsEventsClient;
use codex_arg0::Arg0DispatchPaths;
use codex_goal_extension::GoalService;
use codex_login::AuthManager;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use std::collections::HashSet;
use std::future::Future;
use std::task::Context as TaskContext;
use std::task::Waker;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use wiremock::MockServer;

struct Input;
impl IncomingStreamProvider for Input {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::pending())
    }

    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(std::future::pending())
    }
}

pub(crate) struct Harness {
    pub(crate) _server: MockServer,
    pub(crate) parent: TestCodex,
    pub(crate) processor: ThreadRequestProcessor,
    pub(crate) input: Incoming,
    outgoing: mpsc::Receiver<OutgoingEnvelope>,
}

impl Harness {
    // Uses the existing listener_shutdown_tests real constructor pattern.
    // The pending provider is only an owned IO-lifetime input: no Native grant.
    pub(crate) async fn new(capacity: usize) -> Result<Self> {
        Self::new_with_store(capacity, None).await
    }

    pub(crate) async fn new_non_paginated(capacity: usize) -> Result<Self> {
        Self::new_with_store(
            capacity,
            Some(Arc::new(codex_thread_store::InMemoryThreadStore::default())),
        )
        .await
    }

    async fn new_with_store(
        capacity: usize,
        thread_store: Option<Arc<dyn codex_thread_store::ThreadStore>>,
    ) -> Result<Self> {
        let server = MockServer::start().await;
        let home = Arc::new(TempDir::new()?);
        MockResponsesConfig::new(&server.uri()).write(home.path())?;
        let builder = test_codex().with_home(home);
        let mut builder = match thread_store {
            Some(thread_store) => builder.with_thread_store(thread_store),
            None => builder,
        };
        let parent = builder.build_with_auto_env(&server).await?;
        let config = Arc::new(parent.config.clone());
        let auth = AuthManager::shared_from_config(
            config.as_ref(),
            /*enable_codex_api_key_env*/ false,
        )
        .await?;
        let config_manager =
            ConfigManager::without_managed_config_for_tests(config.codex_home.to_path_buf());
        let (outgoing_tx, outgoing) = mpsc::channel(capacity);
        let sender = Arc::new(OutgoingMessageSender::new(
            outgoing_tx,
            AnalyticsEventsClient::disabled(),
        ));
        let states = ThreadStateManager::new();
        let pending_thread_unloads = Default::default();
        let watches = ThreadWatchManager::new_with_outgoing(sender.clone());
        let skills = SkillsWatcher::new(
            parent.thread_manager.skills_service(),
            &config.codex_home,
            sender.clone(),
        );
        let goals = ThreadGoalRequestProcessor::new(
            parent.thread_manager.clone(),
            sender.clone(),
            config.clone(),
            crate::request_processors::GoalProcessorThreadState::new(
                states.clone(),
                Arc::clone(&pending_thread_unloads),
            ),
            /*state_db*/ None,
            Arc::new(GoalService::new()),
            config_manager.clone(),
        );
        let processor = ThreadRequestProcessor::new(
            auth,
            parent.thread_manager.clone(),
            sender,
            Arg0DispatchPaths::default(),
            config,
            config_manager,
            parent.thread_store.clone(),
            pending_thread_unloads,
            states,
            watches,
            Arc::new(Semaphore::new(/*permits*/ 1)),
            goals,
            /*state_db*/ None,
            /*log_db*/ None,
            skills,
            /*turn_cost_worker*/ None,
            Vec::new(),
            Arc::new(SampleResponseRegistry::default()),
        );
        Ok(Self {
            _server: server,
            parent,
            processor,
            input: Incoming::new(Input),
            outgoing,
        })
    }

    pub(crate) fn request(&self) -> BackendStartRequest {
        BackendStartRequest {
            task_binding: None,
            parent_id: self.parent.session_configured.thread_id,
            parent: self.parent.codex.clone(),
            params: ThreadStartParams {
                model: self.parent.config.model.clone(),
                model_provider: Some(self.parent.config.model_provider_id.clone()),
                cwd: Some(self.parent.config.cwd.to_string_lossy().into_owned()),
                ephemeral: Some(true),
                ..Default::default()
            },
            app_server_client_name: None,
            app_server_client_version: None,
            client_mcp_extensions: Default::default(),
            context: OperationContext {
                span: tracing::info_span!("actual_backend_start_test_operation"),
                parent_trace: None,
                cancellation: CancellationToken::new(),
            },
        }
    }

    pub(crate) fn executor(&self) -> BackendStartExecutor {
        self.processor
            .alab_backend_start_executor(self.input.clone())
    }

    async fn thread_ids(&self) -> HashSet<ThreadId> {
        self.parent
            .thread_manager
            .list_thread_ids()
            .await
            .into_iter()
            .collect()
    }

    pub(crate) fn no_client_reply(&mut self) -> Result<()> {
        while let Ok(envelope) = self.outgoing.try_recv() {
            ensure!(
                !matches!(
                    envelope,
                    OutgoingEnvelope::ToConnection {
                        message: OutgoingMessage::Response(_) | OutgoingMessage::Error(_),
                        ..
                    }
                ),
                "backend START emitted a client response/error",
            );
        }
        Ok(())
    }

    pub(crate) async fn shutdown(&mut self) -> Result<()> {
        self.input.stop();
        let processor = self.processor.clone();
        let work = async move {
            processor.drain_background_tasks().await;
            processor.shutdown_threads().await;
            processor.clear_all_thread_listeners().await;
        };
        tokio::pin!(work);
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                tokio::select! {
                    () = &mut work => return,
                    _ = self.outgoing.recv() => {}
                }
            }
        })
        .await
        .context("real START fixture cleanup did not finish")
    }
}

pub(crate) async fn completed(start: &BackendStart) -> Result<Arc<StartedThread>> {
    let result = tokio::time::timeout(Duration::from_secs(30), start.wait())
        .await
        .context("backend START did not complete")?;
    match result {
        Ok(started) => Ok(started),
        Err(failure) => anyhow::bail!("backend START failed with code {}", failure.error.code),
    }
}

async fn failed(start: &BackendStart) -> Result<Arc<StartFailure>> {
    let result = tokio::time::timeout(Duration::from_secs(30), start.wait())
        .await
        .context("backend START failure did not complete")?;
    match result {
        Ok(_) => anyhow::bail!("invalid backend START unexpectedly succeeded"),
        Err(failure) => Ok(failure),
    }
}

#[tokio::test]
async fn real_ephemeral_start_installs_listener_without_frontend_queue_progress() -> Result<()> {
    let mut h = Harness::new(/*capacity*/ 1).await?;
    let result = async {
        let rules = h.parent.config.codex_home.join("rules");
        std::fs::create_dir_all(&rules)?;
        std::fs::write(rules.join("broken.rules"), "prefix_rule(")?;
        // Fill the actual outbox, then do not consume it while startup runs.
        // Successful completion cannot depend on a UI reply/notification/ACK.
        h.processor
            .outgoing
            .send_server_notification(ServerNotification::ConfigWarning(
                ConfigWarningNotification {
                    summary: "test queue occupancy".to_string(),
                    details: None,
                    path: None,
                    range: None,
                },
            ))
            .await;
        ensure!(h.outgoing.capacity() == 0, "outbox was not actually full");
        let request = h.request();
        let expected_model = request.params.model.clone();
        let expected_provider = request.params.model_provider.clone();
        let expected_cwd = h.parent.config.cwd.clone();
        let start = h.executor().begin(request).map_err(|error| {
            anyhow::anyhow!("backend START begin failed with code {}", error.code)
        })?;
        let started = completed(&start).await?;
        let again = completed(&start).await?;
        ensure!(
            start
                .warnings()
                .iter()
                .any(|warning| warning.summary == "Error parsing rules; custom rules not applied."),
            "startup warning was not retained for the actual backend operation",
        );
        ensure!(
            Arc::ptr_eq(&started, &again),
            "completion was reconstructed"
        );
        ensure!(
            started.response.thread.ephemeral,
            "thread was not ephemeral"
        );
        ensure!(
            started.response.thread.path.is_none(),
            "ephemeral thread has a rollout path"
        );
        ensure!(
            Some(&started.response.model) == expected_model.as_ref(),
            "selected model changed"
        );
        ensure!(
            Some(&started.response.model_provider) == expected_provider.as_ref(),
            "selected provider changed"
        );
        ensure!(
            started.response.cwd.as_path() == expected_cwd.as_path(),
            "selected cwd changed"
        );
        let id = ThreadId::from_string(&started.response.thread.id)?;
        let actual = h.parent.thread_manager.get_thread(id).await?;
        ensure!(
            Arc::ptr_eq(&actual, &started.thread),
            "result lost the actual runtime"
        );
        {
            let state = h.processor.thread_state_manager.thread_state(id).await;
            let state = state.lock().await;
            ensure!(
                state.listener_matches(&actual),
                "actual listener was not installed"
            );
            ensure!(
                state.listener_command_tx().is_some(),
                "listener command receiver is absent"
            );
        }
        ensure!(
            h.processor
                .thread_state_manager
                .subscribed_connection_ids(id)
                .await
                .is_empty(),
            "backend invented a UI subscriber"
        );
        ensure!(
            h.processor
                .thread_watch_manager
                .loaded_status_for_thread(&id.to_string())
                .await
                == ThreadStatus::Idle,
            "actual thread watch is not loaded/idle"
        );
        ensure!(
            h.outgoing.capacity() == 0,
            "startup consumed the held outbox"
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn early_validation_error_has_typed_completion_and_no_child_wire_error() -> Result<()> {
    let mut h = Harness::new(16).await?;
    let result = async {
        let before = h.thread_ids().await;
        let mut request = h.request();
        request.params.project_id = Some(String::new());
        let start = h
            .executor()
            .begin(request)
            .map_err(|error| anyhow::anyhow!("begin code {}", error.code))?;
        let failure = failed(&start).await?;
        ensure!(
            failure.error.code == -32600,
            "wrong validation error category"
        );
        ensure!(
            failure.error.message == "projectId must not be empty",
            "wrong validation failure"
        );
        ensure!(failure.created().is_none(), "validation captured a runtime");
        ensure!(
            matches!(failure.custody(), StartCustody::NotStarted),
            "joined early validation did not observe an unentered Core-start path",
        );
        ensure!(
            h.thread_ids().await == before,
            "validation installed another Core thread"
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn worker_policy_error_is_stored_once_without_client_publication() -> Result<()> {
    let mut h = Harness::new(16).await?;
    let result = async {
        let before = h.thread_ids().await;
        let mut request = h.request();
        // This check is inside the real worker, after actual ConfigManager load.
        request.params.daybreak_enabled = Some(true);
        let start = h
            .executor()
            .begin(request)
            .map_err(|error| anyhow::anyhow!("begin code {}", error.code))?;
        let failure = failed(&start).await?;
        let again = failed(&start).await?;
        ensure!(
            Arc::ptr_eq(&failure, &again),
            "failure completion was reconstructed"
        );
        ensure!(failure.error.code == -32600, "wrong worker error category");
        ensure!(
            failure.error.message == "daybreakEnabled is not supported for ephemeral threads",
            "wrong worker policy failure"
        );
        ensure!(
            failure.created().is_none(),
            "pre-Core policy failure captured a runtime"
        );
        ensure!(
            matches!(failure.custody(), StartCustody::NotStarted),
            "joined worker policy error did not observe an unentered Core-start path",
        );
        ensure!(
            h.thread_ids().await == before,
            "worker policy failure installed a Core thread"
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn failed_core_entry_without_captured_runtime_remains_unknown() -> Result<()> {
    let mut h = Harness::new(16).await?;
    let result = async {
        let mut request = h.request();
        request
            .params
            .config
            .get_or_insert_with(Default::default)
            .insert(
                "mcp_servers".to_string(),
                serde_json::json!({
                    "required_failure": {
                        "command": h.parent.home.path().join("missing-required-helper"),
                        "required": true,
                        "startup_timeout_sec": 10,
                    },
                }),
            );
        let start = h
            .executor()
            .begin(request)
            .map_err(|error| anyhow::anyhow!("begin code {}", error.code))?;
        let failure = failed(&start).await?;
        ensure!(
            failure
                .error
                .message
                .contains("required MCP servers failed to initialize"),
            "control did not exercise an actual required-MCP Core startup failure",
        );
        ensure!(
            matches!(failure.custody(), StartCustody::Unknown),
            "a failed entered Core-start path was reported NotStarted",
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn cancelled_wait_observation_keeps_same_real_startup_and_result() -> Result<()> {
    let mut h = Harness::new(16).await?;
    let result = async {
        let before = h.thread_ids().await;
        let start = h
            .executor()
            .begin(h.request())
            .map_err(|error| anyhow::anyhow!("begin code {}", error.code))?;
        // No yield occurs before this poll: the actual spawned startup worker
        // has not run. Cancelling only its waiter cannot lose that accepted work.
        {
            let mut observation = std::pin::pin!(start.wait());
            let mut context = TaskContext::from_waker(Waker::noop());
            ensure!(
                observation.as_mut().poll(&mut context).is_pending(),
                "wait was not pending"
            );
        }
        let started = completed(&start).await?;
        let again = completed(&start).await?;
        ensure!(
            Arc::ptr_eq(&started, &again),
            "wait cancellation started another outcome"
        );
        let id = ThreadId::from_string(&started.response.thread.id)?;
        let after = h.thread_ids().await;
        ensure!(
            after.difference(&before).copied().collect::<HashSet<_>>() == HashSet::from([id]),
            "cancelled observation changed the started runtime set"
        );
        ensure!(
            Arc::ptr_eq(
                &h.parent.thread_manager.get_thread(id).await?,
                &started.thread
            ),
            "cancelled observation lost actual runtime identity"
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn unpolled_coordinator_destruction_has_a_terminal_observation() -> Result<()> {
    let completion = Arc::new(Completion::new());
    let start = BackendStart {
        completion: completion.clone(),
    };
    let guard = CoordinatorGuard {
        completion,
        armed: true,
    };
    let coordinator = async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    };
    // Destroy the actual future without its first poll. A guard created inside
    // the future body would never run its constructor or publish failure here.
    drop(coordinator);
    let failure = failed(&start).await?;
    let repeated = failed(&start).await?;
    ensure!(
        Arc::ptr_eq(&failure, &repeated),
        "unpolled destruction did not retain one terminal observation",
    );
    ensure!(
        failure.error.code == -32603,
        "coordinator destruction was reported as a successful start",
    );
    ensure!(
        matches!(failure.custody(), StartCustody::Unknown),
        "unpolled coordinator loss became a NotStarted observation",
    );
    Ok(())
}

#[tokio::test]
async fn original_runtime_teardown_releases_an_unpolled_coordinator_waiter() -> Result<()> {
    let (observer_tx, observer_rx) = tokio::sync::oneshot::channel();
    let original_runtime = std::thread::spawn(move || -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let completion = Arc::new(Completion::new());
        let start = BackendStart {
            completion: completion.clone(),
        };
        let guard = CoordinatorGuard {
            completion,
            armed: true,
        };
        let _coordinator = runtime.spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        observer_tx
            .send(start)
            .map_err(|_| anyhow::anyhow!("teardown test observer closed"))?;
        // This runtime was never driven after spawning. Its real destruction
        // must destroy the queued future even though the observer owns Sender.
        drop(runtime);
        Ok(())
    });
    let start = tokio::time::timeout(Duration::from_secs(30), observer_rx)
        .await
        .context("original runtime did not publish its observer")??;
    let result = async {
        let failure = failed(&start).await?;
        let repeated = failed(&start).await?;
        ensure!(
            Arc::ptr_eq(&failure, &repeated),
            "runtime loss reconstructed its terminal result",
        );
        ensure!(
            failure.error.code == -32603,
            "runtime loss was reported as a successful start",
        );
        ensure!(
            matches!(failure.custody(), StartCustody::Unknown),
            "runtime destruction became a NotStarted observation",
        );
        Ok(())
    }
    .await;
    let joined = tokio::task::spawn_blocking(move || {
        original_runtime
            .join()
            .map_err(|_| anyhow::anyhow!("original runtime test thread panicked"))?
    })
    .await
    .context("original runtime join observer failed")?;
    result.and(joined)
}

#[tokio::test]
async fn coordinator_abort_keeps_actual_child_but_releases_staged_success_owners() -> Result<()> {
    let mut h = Harness::new(16).await?;
    let result = async {
        let actual_start = h
            .executor()
            .begin(h.request())
            .map_err(|error| anyhow::anyhow!("begin code {}", error.code))?;
        let actual = completed(&actual_start).await?;
        let id = ThreadId::from_string(&actual.response.thread.id)?;
        ensure!(
            Arc::ptr_eq(
                &h.parent.thread_manager.get_thread(id).await?,
                &actual.thread,
            ),
            "terminal control did not use the actual newly created runtime",
        );

        // Exercise both orders at the shared stage/terminal boundary with this
        // one real runtime. The Weak is an ownership witness for the duplicate
        // StartedThread wrapper carrying that actual child, not a map-value check.
        for stage_before_abort in [true, false] {
            let completion = Arc::new(Completion::new());
            let start = BackendStart {
                completion: completion.clone(),
            };
            let target = StartTarget::Backend {
                scope: Arc::new(BackendScope {
                    task_binding: None,
                    incoming: h.input.clone(),
                    parent_id: h.parent.session_configured.thread_id,
                    parent: h.parent.codex.clone(),
                    cancellation: CancellationToken::new(),
                }),
                completion: completion.clone(),
            };
            if stage_before_abort {
                target.retain_created(id, &actual.thread).map_err(|error| {
                    anyhow::anyhow!("actual capture rejected: {}", error.message)
                })?;
            }
            let duplicate = Arc::new(StartedThread {
                thread: actual.thread.clone(),
                response: actual.response.clone(),
            });
            let duplicate_owner = Arc::downgrade(&duplicate);
            let mut late_success = Some(duplicate);
            if stage_before_abort {
                completion.stage(Ok(late_success.take().context("missing staged success")?));
                ensure!(
                    duplicate_owner.upgrade().is_some(),
                    "control did not actually stage the runtime owner",
                );
            }
            let guard = CoordinatorGuard {
                completion: completion.clone(),
                armed: true,
            };
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let coordinator = tokio::spawn(async move {
                let _guard = guard;
                let _ = entered_tx.send(());
                std::future::pending::<()>().await;
            });
            entered_rx
                .await
                .context("coordinator did not enter its real task")?;
            coordinator.abort();
            let join = coordinator.await;
            ensure!(
                matches!(join, Err(error) if error.is_cancelled()),
                "coordinator was not actually aborted",
            );
            let terminal = failed(&start).await?;
            ensure!(
                matches!(terminal.custody(), StartCustody::Unknown),
                "coordinator abort became a joined custody observation",
            );
            if !stage_before_abort {
                // An empty live slot is not a no-birth fact: the tracked worker
                // below can still capture its actual runtime after this failure.
                ensure!(
                    terminal.created().is_none(),
                    "late-capture control was not empty"
                );
            }

            if let Some(late_success) = late_success {
                // A genuine later asynchronous worker attempts production's
                // stage operation after the coordinator's terminal publication.
                let late_target = target.clone();
                let late_thread = actual.thread.clone();
                let late_worker = tokio::spawn(async move {
                    late_target.retain_created(id, &late_thread)?;
                    completion.stage(Ok(late_success));
                    Ok::<(), JSONRPCErrorError>(())
                });
                late_worker
                    .await
                    .context("late startup worker panicked")?
                    .map_err(|error| {
                        anyhow::anyhow!("actual late capture rejected: {}", error.message)
                    })?;
            }
            let retained = terminal
                .created()
                .context("abort lost actual or late child custody")?;
            ensure!(
                Arc::ptr_eq(&retained, &actual.thread),
                "abort substituted a child lookup or another runtime"
            );
            ensure!(
                duplicate_owner.upgrade().is_none(),
                "terminal completion pinned a staged duplicate child owner",
            );
            let repeated = failed(&start).await?;
            ensure!(
                Arc::ptr_eq(&terminal, &repeated),
                "late success replaced the stored terminal failure",
            );
            ensure!(
                matches!(repeated.custody(), StartCustody::Unknown),
                "late runtime capture changed lost coordination into joined custody",
            );
            ensure!(
                Arc::ptr_eq(
                    &repeated
                        .created()
                        .context("late success lost original custody")?,
                    &actual.thread,
                ),
                "late success changed the retained child",
            );
        }
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[derive(Clone, Copy)]
enum BlockedMcpCancellation {
    WaitObservation,
    IssuingOperation,
}

async fn run_blocked_mcp_cancellation(action: BlockedMcpCancellation) -> Result<()> {
    let files = TempDir::new()?;
    let mut h = Harness::new(16).await?;
    let barrier = files.path().join("allow-child-initialize");
    let pid_file = files.path().join("blocked-child-mcp.pid");
    let before = h.thread_ids().await;
    let mut accepted: Option<BackendStart> = None;
    let result: Result<()> = async {
        let mut request = h.request();
        let command = core_test_support::stdio_server_bin()
            .context("qualified test_stdio_server helper is required")?;
        // The parent Harness is already fully started. Only this child's real
        // request override creates the required MCP process and initialize gate.
        request
            .params
            .config
            .get_or_insert_with(Default::default)
            .insert(
                "mcp_servers".to_string(),
                serde_json::json!({
                    "blocked": {
                        "command": command,
                        "required": true,
                        "startup_timeout_sec": 120,
                        "env": {
                            "MCP_TEST_INITIALIZE_BARRIER_FILE": barrier.to_string_lossy(),
                            "MCP_TEST_PID_FILE": pid_file.to_string_lossy(),
                        },
                    },
                }),
            );
        let cancellation = request.context.cancellation.clone();
        accepted = Some(h.executor().begin(request).map_err(|error| {
            anyhow::anyhow!("blocked MCP START begin failed with code {}", error.code)
        })?);
        let start = accepted
            .as_ref()
            .context("accepted startup owner is absent")?;

        core_test_support::fs_wait::wait_for_path_exists(&pid_file, Duration::from_secs(10))
            .await?;
        let pid = core_test_support::process::wait_for_pid_file(&pid_file).await?;
        ensure!(
            core_test_support::process::process_is_alive(&pid)?,
            "required child MCP process was not actually alive at the barrier",
        );
        ensure!(!barrier.exists(), "initialize barrier was already released");
        {
            // Unlike the pre-poll control, the worker has performed real policy,
            // spawn and MCP initialize IO. Only this response observation ends.
            let mut observation = std::pin::pin!(start.wait());
            let mut context = TaskContext::from_waker(Waker::noop());
            ensure!(
                observation.as_mut().poll(&mut context).is_pending(),
                "required MCP START completed while initialize remained blocked",
            );
        }
        ensure!(
            core_test_support::process::process_is_alive(&pid)?,
            "dropping the waiter terminated required startup work",
        );
        if matches!(action, BlockedMcpCancellation::IssuingOperation) {
            cancellation.cancel();
            ensure!(
                !barrier.exists(),
                "operation cancellation released initialize"
            );
            {
                let mut observation = std::pin::pin!(start.wait());
                let mut context = TaskContext::from_waker(Waker::noop());
                ensure!(
                    observation.as_mut().poll(&mut context).is_pending(),
                    "operation cancellation discarded the partly-polled startup worker",
                );
            }
            ensure!(
                core_test_support::process::process_is_alive(&pid)?,
                "operation cancellation lost the actual blocked MCP process",
            );
        }
        std::fs::write(&barrier, "ready")?;

        let (id, retained) = match action {
            BlockedMcpCancellation::WaitObservation => {
                let started = completed(start).await?;
                let repeated = completed(start).await?;
                ensure!(
                    Arc::ptr_eq(&started, &repeated),
                    "wait cancellation reconstructed the startup completion",
                );
                (
                    ThreadId::from_string(&started.response.thread.id)?,
                    started.thread.clone(),
                )
            }
            BlockedMcpCancellation::IssuingOperation => {
                let failure = failed(start).await?;
                let repeated = failed(start).await?;
                ensure!(
                    Arc::ptr_eq(&failure, &repeated),
                    "operation cancellation reconstructed its terminal failure",
                );
                ensure!(
                    failure.error.code == -32603,
                    "operation cancellation lost its failure category"
                );
                let created = failure
                    .created()
                    .context("cancelled post-birth startup lost its actual Core runtime")?;
                let captured = match failure.custody() {
                    StartCustody::Captured(thread) => thread,
                    StartCustody::NotStarted | StartCustody::Unknown => {
                        anyhow::bail!(
                            "normally joined post-birth cancellation lost captured custody"
                        )
                    }
                };
                ensure!(
                    Arc::ptr_eq(&created, &captured),
                    "typed custody substituted the originally captured runtime",
                );
                let new_ids = h
                    .thread_ids()
                    .await
                    .difference(&before)
                    .copied()
                    .collect::<Vec<_>>();
                ensure!(
                    new_ids.len() == 1,
                    "cancelled startup did not retain one actual new Core runtime"
                );
                (new_ids[0], created)
            }
        };
        ensure!(
            h.thread_ids()
                .await
                .difference(&before)
                .copied()
                .collect::<HashSet<_>>()
                == HashSet::from([id]),
            "blocked startup cancellation changed the actual new runtime set",
        );
        ensure!(
            Arc::ptr_eq(&h.parent.thread_manager.get_thread(id).await?, &retained),
            "blocked startup lost the original created runtime",
        );
        ensure!(
            h.processor
                .thread_state_manager
                .subscribed_connection_ids(id)
                .await
                .is_empty(),
            "blocked backend startup invented a UI subscriber",
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;

    // Every assertion above returns through Result, so even PID/aliveness/poll
    // failure releases initialize BEFORE waiting for actual fixture cleanup.
    // Keep the directory alive until cleanup finishes. A failed assertion must
    // not strand the required helper behind its 120-second initialization gate.
    let released = std::fs::write(&barrier, "ready")
        .context("failed to release required MCP initialize during cleanup");
    let joined: Result<()> = async {
        // Even when an assertion above failed, join accepted startup after
        // releasing its gate, then shut down each actual new Core runtime.
        // No PID kill, missing-slot/no-child assumption or SDK counter is used.
        if let Some(start) = &accepted {
            let _outcome = tokio::time::timeout(Duration::from_secs(30), start.wait())
                .await
                .context("released MCP startup did not join during cleanup")?;
        }
        let new_ids = h
            .thread_ids()
            .await
            .difference(&before)
            .copied()
            .collect::<Vec<_>>();
        for id in new_ids {
            let runtime = h.parent.thread_manager.get_thread(id).await?;
            tokio::time::timeout(Duration::from_secs(30), runtime.shutdown_and_wait())
                .await
                .context("actual child Core runtime did not shut down")??;
        }
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(released).and(joined).and(cleanup)
}

#[tokio::test]
async fn required_mcp_startup_survives_cancelled_wait_after_real_child_spawn() -> Result<()> {
    run_blocked_mcp_cancellation(BlockedMcpCancellation::WaitObservation).await
}

#[tokio::test]
async fn operation_cancel_during_required_mcp_startup_preserves_actual_child_failure() -> Result<()>
{
    run_blocked_mcp_cancellation(BlockedMcpCancellation::IssuingOperation).await
}
