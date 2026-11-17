// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Genuine initialized MessageProcessor request without Native A provenance
//! must fail closed before registry acceptance or Core. No positive grant.
use super::*;
use wiremock::MockServer;

#[tokio::test]
async fn actual_initialized_start_claim_without_native_origin_is_denied_without_core() -> Result<()>
{
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let plan = crate::in_process::InitialSessionPlan::new(
        crate::in_process::InitialSessionKind::Start,
        None,
    );
    let claim = plan.claim().to_owned();
    let (initial, installed) = h.processor.alab_make_initial_session(plan, h.input.clone());
    let result: Result<()> = async {
        installed?;
        let before = h.host.control.manager.list_thread_ids().await;
        let c1 = h.c1.clone();
        let rejected = h
            .request::<ThreadStartResponse>(
                ConnectionId(41),
                &c1,
                json!({
                    "id":8801,"method":"thread/start","params":{
                        "ephemeral":true,"_meta":{"alab.initialSession":{"claim":claim}}
                    }
                }),
            )
            .await;
        ensure!(
            rejected.is_err(),
            "Source metadata without actual Native origin created a thread"
        );
        ensure!(
            before == h.host.control.manager.list_thread_ids().await,
            "denied request entered Core"
        );
        initial
            .drain()
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(())
    }
    .await;
    let drained = initial
        .drain()
        .await
        .map_err(|error| anyhow::anyhow!(error));
    let cleanup = h.shutdown().await;
    result.and(drained).and(cleanup)
}

#[tokio::test]
async fn actual_picker_resume_and_fork_claims_without_native_origin_cannot_select_core()
-> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let plan = crate::in_process::InitialSessionPlan::new(
        crate::in_process::InitialSessionKind::Selection,
        None,
    );
    let claim = plan.claim().to_owned();
    let (initial, installed) = h.processor.alab_make_initial_session(plan, h.input.clone());
    let result: Result<()> = async {
        installed?;
        let before = h.host.control.manager.list_thread_ids().await;
        let c1 = h.c1.clone();
        for (request, method) in [(8811, "thread/resume"), (8812, "thread/fork")] {
            // These are untrusted request DATA, deliberately lacking physical
            // Native origin. No successful projection or SDK authority is faked.
            let denied = h
                .request::<serde_json::Value>(
                    ConnectionId(41),
                    &c1,
                    json!({
                        "id":request,"method":method,"params":{
                            "threadId":h.root.thread.id.clone(),"_meta":{"alab.initialSession":{
                                "claim":claim,"prospectiveCreate":{"ephemeral":true}
                            }}
                        }
                    }),
                )
                .await;
            ensure!(
                denied.is_err(),
                "picker DATA without physical origin selected Core"
            );
            ensure!(
                before == h.host.control.manager.list_thread_ids().await,
                "denied selection entered Core"
            );
        }
        Ok(())
    }
    .await;
    let drained = initial
        .drain()
        .await
        .map_err(|error| anyhow::anyhow!(error));
    let cleanup = h.shutdown().await;
    result.and(drained).and(cleanup)
}

#[tokio::test]
async fn original_plain_loaded_resume_preserves_exact_runtime_and_actual_private_binding()
-> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        let id = h.parent_id;
        let actual = h.host.control.manager.get_thread(id).await?;
        let private = actual.config().await.mcp_servers.get().clone();
        let c1 = h.c1.clone();
        let _: codex_app_server_protocol::ThreadSetNameResponse = h.request(ConnectionId(41), &c1, json!({
            "id":8821,"method":"thread/name/set","params":{"threadId":h.root.thread.id.clone(),"name":"Actual resume binding control"}
        })).await?;
        let observed = super::super::HostSlot::default();
        observed.install(&h.host.control)?;
        ensure!(observed.bound_tools(id, &actual).await?, "actual root binding not observed");
        let resumed: codex_app_server_protocol::ThreadResumeResponse = h.request(ConnectionId(41), &c1, json!({
            "id":8822,"method":"thread/resume","params":{"threadId":h.root.thread.id.clone(),"excludeTurns":true}
        })).await?;
        ensure!(resumed.thread.id == h.root.thread.id);
        ensure!(Arc::ptr_eq(&h.host.control.manager.get_thread(id).await?, &actual), "plain resume replaced the actual loaded Core");
        ensure!(private == *actual.config().await.mcp_servers.get(), "plain loaded resume changed its private transport");
        ensure!(observed.bound_tools(id, &actual).await?);
        ensure!(h.server_requests.load(Ordering::Relaxed) == 0, "resume depended on a fabricated frontend acknowledgment");
        Ok(())
    }.await;
    result.and(h.shutdown().await)
}

#[tokio::test]
async fn actual_later_client_fork_resume_data_without_native_origin_cannot_bind_or_enter_core()
-> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let plan = crate::in_process::InitialSessionPlan::new(
        crate::in_process::InitialSessionKind::Selection,
        None,
    );
    let (initial, installed) = h.processor.alab_make_initial_session(plan, h.input.clone());
    let result: Result<()> =
        async {
            installed?;
            let before = h.host.control.manager.list_thread_ids().await;
            let bindings = h
                .host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .bindings
                .len();
            let c1 = h.c1.clone();
            for (request, method) in [(8831, "thread/fork"), (8832, "thread/resume")] {
                let denied = h.request::<serde_json::Value>(ConnectionId(41), &c1, json!({
                "id":request,"method":method,"params":{
                    "threadId":h.root.thread.id.clone(),"_meta":{"alab.clientConstruction":{
                        "prospectiveCreate":{"ephemeral":true},"presentation":"primary"
                    }}
                }
            })).await;
                ensure!(
                    denied.is_err(),
                    "client presentation DATA became native permission"
                );
                ensure!(
                    before == h.host.control.manager.list_thread_ids().await,
                    "denied client call entered Core"
                );
                ensure!(
                    bindings
                        == h.host
                            .control
                            .registry
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .bindings
                            .len(),
                    "denied call issued a private binding"
                );
            }
            Ok(())
        }
        .await;
    let drained = initial
        .drain()
        .await
        .map_err(|error| anyhow::anyhow!(error));
    let cleanup = h.shutdown().await;
    result.and(drained).and(cleanup)
}
