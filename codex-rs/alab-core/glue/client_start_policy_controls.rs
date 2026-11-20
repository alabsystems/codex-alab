// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower shared-policy execution with a REAL registered request context, not
//! Native acceptance. The real accepted path is separately denied without A.
//! No Registered/Held transition or successful public offer is fabricated.
use super::*;
use crate::message_processor::alab_task_mcp::tests::Harness;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::alab_selection_request_capture::Capture;
use crate::transport::AppServerTransport;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::json;
use std::time::Duration;
use wiremock::MockServer;

#[tokio::test]
async fn later_start_shared_policy_captures_new_core_and_private_binding_before_missing_offer_error()
-> Result<()> {
    let model = MockServer::start().await;
    let h = Harness::new(&model.uri()).await?;
    let mut captured = None;
    let result: Result<()> = async {
        let config = h.parent.config().await;
        let params = ThreadStartParams {
            model: config.model.clone(),
            model_provider: Some(config.model_provider_id.clone()),
            cwd: Some(config.cwd.to_string_lossy().into_owned()),
            ephemeral: Some(true),
            ..Default::default()
        };
        let (processor, outgoing) = h.processor.alab_selection_publication_parts_for_tests();
        let processor = Arc::new(processor);
        let mut capture = Capture::arm(&outgoing)?;
        let mut parameters = serde_json::to_value(&params)?;
        parameters["_meta"] = json!({"foreign.original":{"kept":[1,2]},
            "alab.clientConstruction":{"prospectiveCreate":params,"presentation":"primary"}});
        let request: JSONRPCRequest = serde_json::from_value(json!({
            "id":8951,"method":"thread/start","params":parameters
        }))?;
        let actual = h.processor.clone();
        let session = h.c1.clone();
        let task = tokio::spawn(async move {
            actual
                .process_request(
                    ConnectionId(41),
                    request,
                    &AppServerTransport::WebSocket {
                        bind_address: ([127, 0, 0, 1], 0).into(),
                    },
                    session,
                )
                .await;
        });
        let observed = tokio::time::timeout(Duration::from_secs(10), capture.next()).await;
        // Original request policy has NOT been polled. Join its real body
        // before exercising the private lower policy with that same Context.
        task.abort();
        let stopped = task.await;
        let context = observed.context("actual Start dispatch did not register")??;
        ensure!(stopped.is_err_and(|error| error.is_cancelled()));
        ensure!(context.alab_connection_resource.is_none());
        let claim = context
            .alab_client_construction
            .clone()
            .context("Start codec did not retain actual claim")?;
        ensure!(claim.method.as_ref() == "thread/start");
        ensure!(claim.selection.prepared.is_none());
        let operation = Arc::new(InitialSessionOperation {
            claim: InitialSessionPlan::fresh_claim(),
            kind: Kind::Selection,
            binding: None,
            input: h.input.clone(),
            bound: None,
            processor: processor.clone(),
            tracker: TaskTracker::new(),
            selected_primary: Mutex::new(selected_primary::SelectionState::new()),
            side_views: Mutex::new(side_views::Views::default()),
            selection_changed: tokio::sync::Notify::new(),
            state: Mutex::new(State {
                closed: false,
                accepted: None,
                next_claim: None,
                clients: HashMap::new(),
                retired_saved_resumes: saved_resume_receipts::Receipts::default(),
                preparations: HashMap::new(),
            }),
        });
        let scope = Arc::new(Scope {
            candidate: InitialSessionPlan::fresh_claim(),
            registered_selection: None,
            prepared_offer: None,
            preparation_error: None,
            side_boundary: Mutex::new(side_boundary::SideSlot::default()),
            side_closings: Mutex::new(side_view_close::Closings::default()),
            side_rollback: Mutex::new(None),
            prior_selection: operation
                .snapshot_selection()
                .map_err(|error| anyhow::anyhow!(error.message))?,
            completed: tokio::sync::watch::channel(()).0,
            operation: Arc::downgrade(&operation),
            input: h.input.clone(),
            processor: processor.clone(),
            binding: None,
            context,
            frozen: frozen::FrozenRequest::client(claim, false),
            arguments: Arguments::Start(params.clone()),
            metadata: ClientMetadata {
                name: h.c1.app_server_client_name().map(str::to_owned),
                version: h.c1.client_version().map(str::to_owned),
                extensions: h.c1.client_mcp_extensions(),
            },
            progress: Arc::new(Mutex::new(Progress {
                entered: false,
                created: None,
                resolved: None,
                loaded_task_tools: false,
                next_selection: None,
                staged: None,
                complete: None,
                worker_lost: false,
                worker_finished: false,
                held_selection: None,
                selection_retired: false,
                selection_unknown: false,
                history_fallback: false,
                dynamic_tools_fallback: false,
                task_binding: None,
                primary: None,
                primary_weak: None,
                replays: Vec::new(),
            })),
        });
        // This lower fixture deliberately has NO Native reservation and never
        // calls accept/accept_client. It must not publish a successful offer.
        let outcome = scope.run().await;
        let (id, actual) = {
            let progress = scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let captured = progress
                .created
                .clone()
                .context("original Start did not capture Core")?;
            ensure!(progress.entered && progress.resolved.is_none());
            ensure!(
                progress.task_binding.is_some(),
                "new Start lost actual private child binding"
            );
            ensure!(
                progress
                    .retained_primary()
                    .is_some_and(|lease| lease.is_for(&captured.1))
            );
            captured
        };
        captured = Some(actual.clone());
        ensure!(id != h.parent_id && !Arc::ptr_eq(&actual, &h.parent));
        ensure!(matches!(
            processor
                .thread_manager
                .thread_registration(id, &actual)
                .await,
            codex_core::ThreadRegistration::Current
        ));
        ensure!(
            processor
                .alab_task_mcp
                .bound_tools(id, &actual)
                .await
                .map_err(|error| anyhow::anyhow!(error.message))?
        );
        ensure!(actual.config().await.model == params.model);
        ensure!(
            *actual.config().await.mcp_servers.get() != *h.parent.config().await.mcp_servers.get(),
            "later Start inherited the parent's private transport"
        );
        match outcome {
            Reply::Error(error) => {
                ensure!(error.message == "client result lost actual preparation")
            }
            _ => anyhow::bail!("lower fixture invented successful Native-prepared completion"),
        }
        // Scope retains the new Core and provisional lease across this normal
        // post-birth error. Real host shutdown below joins that exact runtime.
        drop(scope);
        drop(operation);
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    let terminal: Result<()> = async {
        if let Some(actual) = captured {
            tokio::time::timeout(Duration::from_secs(10), actual.wait_until_terminated()).await?;
        }
        Ok(())
    }
    .await;
    result.and(cleanup).and(terminal)
}
