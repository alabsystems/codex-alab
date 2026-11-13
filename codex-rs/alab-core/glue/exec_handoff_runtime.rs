// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exec's account handoff controller. The native host owns selection and
//! authorization; Core owns the durable stop and original-turn admission.

use codex_app_server_client::InProcessAppServerClient;
use codex_app_server_client::InProcessAppServerRequestHandle;
use codex_protocol::ThreadId;
use tokio::sync::mpsc;

#[path = "exec_bootstrap.rs"]
mod bootstrap;
#[cfg(unix)]
use bootstrap::Checkpoint;
pub use bootstrap::RecoveredWorktree;
pub use bootstrap::WorktreeSnapshot;

/// Records the final loader invocation, including the headless-policy retry.
#[derive(Default)]
pub struct ConfigCapture(std::sync::Mutex<Option<super::ConfigOverrides>>);
impl ConfigCapture {
    pub fn record(&self, overrides: &super::ConfigOverrides) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(overrides.clone());
    }
    pub async fn finish(
        &self,
        recovery: Option<&Recovery>,
        config: &mut super::Config,
    ) -> anyhow::Result<Option<codex_core::config::ExecConfiguration>> {
        if !codex_alab_core::active() {
            return Ok(None);
        }
        #[cfg(unix)]
        if let Some(recovery) = recovery {
            let configuration = recovery
                .inner
                .memory
                .tree
                .threads()
                .first()
                .ok_or_else(|| anyhow::anyhow!("tree capsule has no root"))?
                .configuration();
            *config = configuration.restore(config).await?;
            return Ok(Some(configuration.clone()));
        }
        #[cfg(not(unix))]
        let _ = recovery;
        let overrides = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| anyhow::anyhow!("exec configuration inputs were not captured"))?;
        Ok(Some(codex_core::config::ExecConfiguration::capture(
            config, &overrides,
        )?))
    }
}

pub struct Controller {
    actions: mpsc::UnboundedReceiver<Action>,
    worker: Option<tokio::task::JoinHandle<()>>,
    active: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Controller {
    pub async fn start(
        handle: InProcessAppServerRequestHandle,
        thread: ThreadId,
        enabled: bool,
        worktree: Option<WorktreeSnapshot>,
        configuration: Option<codex_core::config::ExecConfiguration>,
    ) -> Self {
        tracing::debug!(target: "codex_alab_exec_handoff", enabled, active = codex_alab_core::active(), "starting exec handoff controller");
        let (send, actions) = mpsc::unbounded_channel();
        let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        #[cfg(unix)]
        let worker_active = std::sync::Arc::clone(&active);
        #[cfg(unix)]
        let worker = if enabled
            && codex_alab_core::active()
            && let Some(configuration) = configuration
        {
            Some(tokio::spawn(async move {
                if let Err(error) = unix::run(
                    handle,
                    thread,
                    worker_active,
                    send.clone(),
                    worktree,
                    configuration,
                )
                .await
                {
                    let _ = send.send(Action(ActionKind::Failed(error)));
                }
            }))
        } else {
            None
        };
        #[cfg(not(unix))]
        let worker = {
            let _ = (handle, thread, enabled, send, worktree, configuration);
            None
        };
        Self {
            actions,
            worker,
            active,
        }
    }
    pub fn handling(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Acquire)
    }
    pub async fn next(&mut self) -> Action {
        match self.actions.recv().await {
            Some(action) => action,
            None if self.handling() => Action(ActionKind::Failed(
                "exec handoff worker ended before replacement".into(),
            )),
            None => std::future::pending().await,
        }
    }
}

pub struct Action(ActionKind);
enum ActionKind {
    Failed(String),
    #[cfg(unix)]
    Replace(Box<unix::Replacement>),
}
impl Action {
    pub async fn finish(
        self,
        renderer: &dyn super::EventProcessor,
        error_seen: bool,
        interrupts: &mut mpsc::UnboundedReceiver<()>,
    ) -> anyhow::Result<()> {
        #[cfg(not(unix))]
        let _ = (renderer, error_seen);
        tokio::select! {
            biased;
            Some(_) = interrupts.recv() => Err(anyhow::anyhow!("native exec handoff interrupted")),
            result = async {
                match self.0 {
                    ActionKind::Failed(error) => Err(anyhow::anyhow!(error)),
                    #[cfg(unix)]
                    ActionKind::Replace(replacement) => (*replacement).execute(renderer, error_seen).await,
                }
            } => result,
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

pub struct Recovery {
    #[cfg(unix)]
    inner: unix::Recovery,
}
impl Recovery {
    pub async fn note_completion(
        &self,
        notification: &codex_app_server_protocol::ServerNotification,
        error_seen: bool,
    ) {
        #[cfg(unix)]
        if successful_completion(
            notification,
            &self.inner.grant.origin.thread,
            &self.inner.grant.origin.turn,
            error_seen,
        ) {
            self.inner.complete().await;
        }
        #[cfg(not(unix))]
        let _ = (notification, error_seen);
    }

    pub async fn load(home: &std::path::Path) -> anyhow::Result<Option<Self>> {
        #[cfg(unix)]
        {
            Ok(unix::Recovery::load(home)
                .await?
                .map(|inner| Self { inner }))
        }
        #[cfg(not(unix))]
        {
            let _ = home;
            Ok(None)
        }
    }
    pub fn worktree(&self, requested: bool) -> anyhow::Result<Option<RecoveredWorktree>> {
        #[cfg(unix)]
        {
            self.inner.checkpoint.worktree(requested)
        }
        #[cfg(not(unix))]
        {
            let _ = requested;
            unreachable!()
        }
    }

    pub fn restore_renderer(
        &self,
        renderer: &mut dyn super::EventProcessor,
    ) -> anyhow::Result<bool> {
        #[cfg(unix)]
        {
            self.inner.restore_renderer(renderer)
        }
        #[cfg(not(unix))]
        {
            let _ = renderer;
            unreachable!()
        }
    }
    pub async fn resume(
        &self,
        client: &InProcessAppServerClient,
        ids: &mut super::RequestIdSequencer,
        config: &super::Config,
        reviewer: Option<codex_app_server_protocol::ApprovalsReviewer>,
    ) -> anyhow::Result<(
        codex_app_server_protocol::ThreadHistoryMode,
        codex_protocol::protocol::SessionConfiguredEvent,
    )> {
        let _ = (ids, reviewer);
        #[cfg(unix)]
        {
            let request = self.inner.bootstrap_request(config.clone())?;
            let (mode, configured) = client.bootstrap_exec(request).await?;
            Ok((mode.into(), configured))
        }
        #[cfg(not(unix))]
        {
            let _ = (client, config);
            unreachable!()
        }
    }

    pub async fn recover(&self, client: &InProcessAppServerClient) -> anyhow::Result<String> {
        #[cfg(unix)]
        {
            self.inner.recover(client).await
        }
        #[cfg(not(unix))]
        {
            let _ = client;
            unreachable!()
        }
    }
}

#[cfg(unix)]
fn successful_completion(
    notification: &codex_app_server_protocol::ServerNotification,
    thread: &str,
    turn: &str,
    error_seen: bool,
) -> bool {
    !error_seen
        && matches!(notification,
        codex_app_server_protocol::ServerNotification::TurnCompleted(event)
        if event.thread_id == thread && event.turn.id == turn
            && event.turn.status == codex_app_server_protocol::TurnStatus::Completed
            && event.turn.error.is_none())
}

#[cfg(all(test, unix))]
mod completion_tests {
    use super::*;
    use codex_app_server_protocol::ServerNotification;
    use codex_app_server_protocol::Turn;
    use codex_app_server_protocol::TurnCompletedNotification;
    use codex_app_server_protocol::TurnError;
    use codex_app_server_protocol::TurnItemsView;
    use codex_app_server_protocol::TurnStatus;

    #[test]
    fn only_the_successful_original_turn_proves_the_account() {
        let mut turn = Turn {
            id: "original-turn".into(),
            root_turn_id: None,
            items_view: TurnItemsView::Full,
            items: Vec::new(),
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: Some(1),
            duration_ms: None,
        };
        let event = |turn: Turn| {
            ServerNotification::TurnCompleted(TurnCompletedNotification {
                thread_id: "original-thread".into(),
                turn,
            })
        };
        assert!(successful_completion(
            &event(turn.clone()),
            "original-thread",
            "original-turn",
            false
        ));
        for (thread, id, error_seen) in [
            ("other", "original-turn", false),
            ("original-thread", "other", false),
            ("original-thread", "original-turn", true),
        ] {
            assert!(!successful_completion(
                &event(turn.clone()),
                thread,
                id,
                error_seen
            ));
        }
        for status in [
            TurnStatus::Failed,
            TurnStatus::Interrupted,
            TurnStatus::InProgress,
        ] {
            turn.status = status;
            assert!(!successful_completion(
                &event(turn.clone()),
                "original-thread",
                "original-turn",
                false
            ));
        }
        turn.status = TurnStatus::Completed;
        turn.error = Some(TurnError {
            message: "failed".into(),
            codex_error_info: None,
            additional_details: None,
            misalignment: None,
        });
        assert!(!successful_completion(
            &event(turn),
            "original-thread",
            "original-turn",
            false
        ));
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use codex_alab_core::account_handoff::Purpose;
    use codex_app_server_client::alab_native_accounts::Client;
    use codex_app_server_client::alab_native_accounts::account_recovery;
    use codex_app_server_client::alab_native_accounts::exec_capsule;
    use codex_app_server_client::alab_native_accounts::exec_grant;
    use codex_app_server_client::alab_native_accounts::renewal::Identity;
    use codex_app_server_client::alab_native_accounts::rotation::Origin;
    use std::time::Duration;

    const ENV: &str = "ATERM_CODEX_EXEC_HANDOFF";

    async fn native() -> Option<Client> {
        let connection = Client::connect_self().await;
        tracing::debug!(target: "codex_alab_exec_handoff", connected = matches!(connection, Ok(Some(_))), "locating native host");
        let client = connection.ok()??;
        client.harness_policy().await.ok()?;
        let supported =
            exec_grant::supported(&client).await && exec_grant::tree::supported(&client).await;
        tracing::debug!(target: "codex_alab_exec_handoff", supported, policy_present = client.policy().is_some(), "read native handoff contract");
        if !supported {
            return None;
        }
        account_recovery::policy_allows(&client.policy()?, Purpose::WeeklyQuota).then_some(client)
    }

    pub(super) async fn run(
        handle: InProcessAppServerRequestHandle,
        thread: ThreadId,
        active: std::sync::Arc<std::sync::atomic::AtomicBool>,
        actions: mpsc::UnboundedSender<Action>,
        worktree: Option<WorktreeSnapshot>,
        configuration: codex_core::config::ExecConfiguration,
    ) -> Result<(), String> {
        let mut watch = handle
            .watch_exec_quota(thread)
            .await
            .map_err(|_| "exec quota observation unavailable")?;
        let mut connection: Option<Client> = None;
        tracing::debug!(target: "codex_alab_exec_handoff", "exec quota watch ready");
        loop {
            let wall = watch.borrow().clone();
            tracing::debug!(target: "codex_alab_exec_handoff", observed = wall.is_some(), current = wall.as_ref().is_some_and(|wall| wall.current()), "reading exec quota watch");
            if let Some(wall) = wall
                && wall.current()
            {
                let Some(codex_alab_core::renewal::RequestAuth::ChatGpt { account, user }) =
                    wall.source()
                else {
                    watch
                        .changed()
                        .await
                        .map_err(|_| "exec quota observation closed")?;
                    continue;
                };
                if connection.as_ref().is_none_or(|client| !client.is_open()) {
                    connection = match native().await {
                        Some(client) if client.claim().await.is_ok() && client.holds_claim() => {
                            Some(client)
                        }
                        _ => None,
                    };
                }
                if let Some(client) = connection.clone() {
                    let Some(identity) = Identity::from_native(&account, &user) else {
                        return Ok(());
                    };
                    let Some(root_options) = watch.root_options().await else {
                        // A child cannot invent a recoverable root turn.
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    };
                    if root_options.thread() != thread || !wall.current() {
                        continue;
                    }
                    let origin = Origin {
                        thread: thread.to_string(),
                        turn: root_options.turn().into(),
                        identity,
                    };
                    let selection = account_recovery::prepare_exec(&client, &origin).await;
                    tracing::debug!(target: "codex_alab_exec_handoff", selected = matches!(selection, Ok(account_recovery::ExecSelection::Selected { .. })), failed = selection.is_err(), "read native account selection");
                    if let Ok(account_recovery::ExecSelection::Selected { destination, .. }) =
                        selection
                        && wall.current()
                    {
                        let checkpoint =
                            serde_json::to_value(Checkpoint::new(root_options, worktree.clone()))
                                .map_err(|_| "exec checkpoint encoding failed")?;
                        let armed = match exec_grant::arm(
                            &client,
                            &origin,
                            &destination,
                            &checkpoint,
                        )
                        .await
                        {
                            Ok(grant) => Some((client.clone(), grant)),
                            Err(_) => match native().await {
                                Some(reconnected) => exec_grant::reconcile_arm(
                                    &reconnected,
                                    &origin,
                                    &destination,
                                    &checkpoint,
                                )
                                .await
                                .ok()
                                .map(|grant| (reconnected, grant)),
                                None => None,
                            },
                        };
                        if let Some((client, grant)) = armed {
                            if !wall.current() {
                                let _ = exec_grant::cancel(&client, &grant.ticket).await;
                                continue;
                            }
                            let prepared = match exec_grant::PreparedExec::new(&grant) {
                                Ok(prepared) => prepared,
                                Err(_) => {
                                    let _ = exec_grant::cancel(&client, &grant.ticket).await;
                                    continue;
                                }
                            };
                            if !reserve_while_current(&client, &grant, &wall, &mut watch).await {
                                let _ = exec_grant::cancel(&client, &grant.ticket).await;
                                continue;
                            }
                            active.store(true, std::sync::atomic::Ordering::Release);
                            let suspended = match handle
                                .suspend_exec_turn(
                                    thread,
                                    origin.turn.clone(),
                                    configuration.clone(),
                                )
                                .await
                            {
                                Ok(suspended) => suspended,
                                Err(_) => {
                                    let _ = exec_grant::cancel(&client, &grant.ticket).await;
                                    return Err(
                                        "exec handoff could not stop the original turn".into()
                                    );
                                }
                            };
                            if serde_json::to_value(Checkpoint::new(
                                suspended.options().clone(),
                                worktree.clone(),
                            ))
                            .ok()
                            .as_ref()
                                != Some(&checkpoint)
                            {
                                let _ = exec_grant::cancel(&client, &grant.ticket).await;
                                return Err("exec handoff original settings changed".into());
                            }
                            actions
                                .send(Action(ActionKind::Replace(Box::new(Replacement {
                                    prepared,
                                    grant,
                                    native: client,
                                    tree: suspended.tree().clone(),
                                }))))
                                .map_err(|_| "exec output owner closed before replacement")?;
                            return Ok(());
                        }
                    }
                }
                tokio::select! {
                    changed = watch.changed() => { changed.map_err(|_| "exec quota observation closed")?; }
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                }
            } else {
                if let Some(client) = connection.take() {
                    client.close();
                }
                if watch.changed().await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    async fn reserve_while_current(
        client: &Client,
        grant: &exec_grant::Grant,
        wall: &codex_core::ExecQuotaWall,
        watch: &mut codex_core::ExecQuotaWatch,
    ) -> bool {
        let Ok(wait) = exec_grant::remaining(grant.deadline_us) else {
            return false;
        };
        tokio::time::timeout(wait, async {
            loop {
                if !wall.current() {
                    return false;
                }
                match exec_grant::reserve(client, grant).await {
                    Ok(exec_grant::Execution::Ready) => return wall.current(),
                    Ok(exec_grant::Execution::Waiting) => {}
                    Err(_) => return false,
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                    changed = watch.changed() => { if changed.is_err() { return false; } }
                }
            }
        })
        .await
        .unwrap_or(false)
    }

    pub(super) struct Replacement {
        prepared: exec_grant::PreparedExec,
        grant: exec_grant::Grant,
        native: Client,
        tree: codex_core::ExecTreeSnapshot,
    }
    impl Replacement {
        pub(super) async fn execute(
            self,
            renderer: &dyn super::super::EventProcessor,
            error_seen: bool,
        ) -> anyhow::Result<()> {
            use std::io::Write;
            let renderer = renderer
                .alab_handoff_snapshot()
                .map_err(anyhow::Error::msg)?;
            let memory = MemoryState {
                renderer,
                error_seen,
                tree: self.tree,
            };
            let capsule = exec_capsule::Capsule::new(&self.grant, serde_json::to_value(&memory)?)?;
            let manifest = tree_manifest(&memory.tree, capsule.reference())?;
            if exec_grant::tree::bind(&self.native, &self.grant, &manifest)
                .await
                .is_err()
            {
                let reconnected = native()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("native tree binding connection was lost"))?;
                exec_grant::tree::bind(&reconnected, &self.grant, &manifest)
                    .await
                    .map_err(|_| anyhow::anyhow!("native exec tree binding was refused"))?;
            }
            let prepared = self.prepared.with_capsule(capsule)?;
            std::io::stdout().flush()?;
            std::io::stderr().flush()?;
            let wait = exec_grant::remaining(self.grant.deadline_us)?;
            tokio::time::timeout(wait, async {
                loop {
                    let result = match exec_grant::execute(&self.native, &self.grant).await {
                        Ok(result) => result,
                        Err(_) => {
                            let reconnected = native().await.ok_or_else(|| {
                                anyhow::anyhow!("native exec handoff connection was lost")
                            })?;
                            exec_grant::execute(&reconnected, &self.grant)
                                .await
                                .map_err(|_| {
                                    anyhow::anyhow!("native exec handoff was not committed")
                                })?
                        }
                    };
                    if result == exec_grant::Execution::Ready {
                        return Ok::<(), anyhow::Error>(());
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            })
            .await
            .map_err(|_| anyhow::anyhow!("native exec handoff deadline elapsed"))??;
            Err(anyhow::anyhow!(
                "native exec replacement failed: {}",
                prepared.execute(self.grant.deadline_us)
            ))
        }
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct MemoryState {
        renderer: serde_json::Value,
        error_seen: bool,
        pub(super) tree: codex_core::ExecTreeSnapshot,
    }

    fn tree_manifest(
        tree: &codex_core::ExecTreeSnapshot,
        reference: &str,
    ) -> anyhow::Result<exec_grant::tree::Manifest> {
        tree.validate(tree.root(), tree.turn())
            .map_err(anyhow::Error::msg)?;
        let digest = reference
            .rsplit('.')
            .next()
            .ok_or_else(|| anyhow::anyhow!("capsule has no digest"))?;
        exec_grant::tree::Manifest::new(
            tree.root().to_string(),
            tree.turn()
                .ok_or_else(|| {
                    anyhow::anyhow!("active exec native manifest cannot admit an idle root")
                })?
                .to_owned(),
            digest.to_owned(),
            tree.threads()
                .iter()
                .map(|node| exec_grant::tree::Member {
                    thread: node.thread().to_string(),
                    parent: node.parent().map(|id| id.to_string()),
                    turn: node.recovery().map(|options| options.turn().to_owned()),
                })
                .collect(),
        )
        .map_err(|_| anyhow::anyhow!("invalid native tree manifest"))
    }

    async fn admit_once(
        client: &Client,
        grant: &exec_grant::Grant,
        manifest: &exec_grant::tree::Manifest,
        thread: ThreadId,
        turn: Option<&str>,
    ) -> Result<(), String> {
        let thread = thread.to_string();
        if thread == grant.origin.thread {
            if turn != Some(grant.origin.turn.as_str()) {
                return Err("native tree root changed its original turn".into());
            }
            exec_grant::consume(client, grant)
                .await
                .map_err(|_| "native root consume failed".into())
        } else {
            exec_grant::tree::admit(client, grant, manifest, &thread, turn)
                .await
                .map(|_| ())
                .map_err(|_| "native tree member admission failed".into())
        }
    }

    pub(super) struct Recovery {
        client: Client,
        pub(super) grant: exec_grant::Grant,
        pub(super) checkpoint: Checkpoint,
        pub(super) memory: MemoryState,
        manifest: exec_grant::tree::Manifest,
    }
    impl Recovery {
        pub(super) fn bootstrap_request(
            &self,
            config: super::super::Config,
        ) -> anyhow::Result<codex_app_server_client::ExecBootstrapRequest> {
            let deadline =
                tokio::time::Instant::now() + exec_grant::remaining(self.grant.deadline_us)?;
            let native = self.client.clone();
            let grant = self.grant.clone();
            let manifest = self.manifest.clone();
            Ok(codex_app_server_client::ExecBootstrapRequest::new(
                config,
                self.memory.tree.clone(),
                deadline,
                move |thread, turn| {
                    let native = native.clone();
                    let grant = grant.clone();
                    let manifest = manifest.clone();
                    Box::pin(async move {
                        if admit_once(&native, &grant, &manifest, thread, turn.as_deref())
                            .await
                            .is_ok()
                        {
                            return Ok(());
                        }
                        let reconnected = self::native()
                            .await
                            .ok_or("native tree connection was lost")?;
                        if admit_once(&reconnected, &grant, &manifest, thread, turn.as_deref())
                            .await
                            .is_ok()
                        {
                            return Ok(());
                        }
                        // After root consumption the old connection's claim may be gone.
                        // Native custody still decides whether a new claim is admissible.
                        reconnected
                            .claim()
                            .await
                            .map_err(|_| "native tree claim unavailable")?;
                        admit_once(&reconnected, &grant, &manifest, thread, turn.as_deref()).await
                    })
                },
            ))
        }
        pub(super) async fn complete(&self) {
            let result = tokio::time::timeout(Duration::from_secs(5), async {
                let mut client = self.client.clone();
                loop {
                    match exec_grant::complete(&client, &self.grant).await {
                        Ok(true) => return Ok::<(), ()>(()),
                        Ok(false) => tokio::time::sleep(Duration::from_millis(100)).await,
                        Err(_) => {
                            if client.is_open() {
                                return Err(());
                            }
                            client = native().await.ok_or(())?;
                            client.claim().await.map_err(|_| ())?;
                        }
                    }
                }
            })
            .await;
            tracing::debug!(target: "codex_alab_exec_handoff", completed = matches!(result, Ok(Ok(()))), "reported recovered turn completion");
        }

        pub(super) async fn load(_home: &std::path::Path) -> anyhow::Result<Option<Self>> {
            let Some(value) = std::env::var_os(ENV) else {
                return Ok(None);
            };
            let text = value
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid native exec handoff ticket"))?;
            if text
                .split('.')
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .is_some_and(|pid| pid != std::process::id())
            {
                return Ok(None); // A child never adopts its parent's ticket.
            }
            let ticket = exec_grant::Ticket::from_process_env(text, std::process::id())
                .ok_or_else(|| anyhow::anyhow!("invalid native exec handoff ticket"))?;
            let client = native()
                .await
                .ok_or_else(|| anyhow::anyhow!("native exec handoff policy is unavailable"))?;
            let grant = exec_grant::load(&client, &ticket)
                .await
                .map_err(|_| anyhow::anyhow!("native exec handoff was not admitted"))?;
            let checkpoint = Checkpoint::decode(
                grant.checkpoint.clone(),
                &grant.origin.thread,
                &grant.origin.turn,
            )?;
            let reference = std::env::var(exec_capsule::ENV)
                .map_err(|_| anyhow::anyhow!("native exec tree capsule is unavailable"))?;
            // SAFETY: this descriptor crossed the authenticated same-PID native exec,
            // has no Rust owner in this image, and is consumed only here.
            let state = unsafe { exec_capsule::load(&grant, &reference) }?;
            let memory: MemoryState = serde_json::from_value(state)?;
            memory
                .tree
                .validate(checkpoint.options.thread(), Some(&grant.origin.turn))
                .map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                serde_json::to_value(
                    memory
                        .tree
                        .threads()
                        .first()
                        .and_then(codex_core::ExecThreadSnapshot::recovery)
                )? == serde_json::to_value(Some(&checkpoint.options))?,
                "tree capsule changed the native checkpoint's original root settings"
            );
            let manifest = tree_manifest(&memory.tree, &reference)?;
            Ok(Some(Self {
                client,
                grant,
                checkpoint,
                memory,
                manifest,
            }))
        }
        pub(super) fn restore_renderer(
            &self,
            renderer: &mut dyn super::super::EventProcessor,
        ) -> anyhow::Result<bool> {
            renderer
                .alab_restore_handoff(self.memory.renderer.clone())
                .map_err(anyhow::Error::msg)?;
            Ok(self.memory.error_seen)
        }

        pub(super) async fn recover(
            &self,
            client: &InProcessAppServerClient,
        ) -> anyhow::Result<String> {
            let original = self.grant.origin.turn.clone();
            let result = client
                .activate_exec_tree(self.checkpoint.options.thread(), original.clone())
                .await?;
            anyhow::ensure!(
                matches!(result, codex_core::StartIfIdleSubmission::Started { .. }),
                "original exec turn was not recovered"
            );
            Ok(original)
        }
    }
}

pub fn skip_recovered_start(
    pending: &mut bool,
    notification: &codex_app_server_protocol::ServerNotification,
    thread: &str,
    turn: &str,
) -> bool {
    if *pending
        && matches!(notification, codex_app_server_protocol::ServerNotification::TurnStarted(start)
        if start.thread_id == thread && start.turn.id == turn)
    {
        *pending = false;
        true
    } else {
        false
    }
}
