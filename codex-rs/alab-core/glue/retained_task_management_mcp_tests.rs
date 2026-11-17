// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual private HTTP management controls on the existing persistent fixture.
//! No manual binding, client facade, permission or Native settlement claim.
use super::*;
use codex_app_server_protocol::ThreadReadResponse;

async fn manage(
    client: &RmcpClient,
    parent: ThreadId,
    tool: &str,
    args: Value,
) -> Result<rmcp::model::CallToolResult> {
    client
        .call_tool(
            tool.into(),
            Some(args),
            move |_| Ok(Some(json!({"threadId":parent.to_string()}))),
            Some(Duration::from_secs(10)),
        )
        .await
}
fn output(result: &rmcp::model::CallToolResult) -> Result<Value> {
    let value = serde_json::to_value(result)?;
    ensure!(
        value["isError"] != true,
        "management returned a tool error: {value}"
    );
    let text = value["content"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["text"].as_str())
        .context("management text absent")?;
    ensure!(
        text.len() <= 8_000,
        "management exceeded original JSON budget"
    );
    Ok(serde_json::from_str(text)?)
}
async fn management_count(h: &Harness, count: usize) -> Result<()> {
    bounded(async {
        loop {
            let actual = {
                h.host
                    .control
                    .registry
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .management
                    .len()
            };
            if actual == count {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

#[tokio::test]
async fn actual_http_title_archive_and_unarchive_preserve_original_policy() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut observed = None;
    let result:Result<()>=async {
        let client=h.client(false).await?;observed=Some(client.clone());
        let catalog=client.list_tools(None,Some(Duration::from_secs(10))).await?;
        for name in ["set_thread_title","set_thread_archived"] {
            let spec=catalog.tools.iter().find(|tool|tool.name.as_ref()==name).context("complete management tool not advertised")?;
            ensure!(serde_json::to_value(spec)?["annotations"]["readOnlyHint"]==false,"management incorrectly marked read-only");
        }
        let raw_title="  Retained source title  ";
        let renamed=output(&manage(&client,h.parent_id,"set_thread_title",json!({"title":raw_title})).await?)?;
        ensure!(renamed==json!({"threadId":h.parent_id.to_string(),"title":raw_title}),"original raw-title/default-target result changed");
        let session=h.c1.clone();
        let actual:ThreadReadResponse=h.request(ConnectionId(41),&session,json!({"id":81,"method":"thread/read","params":{"threadId":h.parent_id.to_string(),"includeTurns":false}})).await?;
        ensure!(actual.thread.name.as_deref()==Some("Retained source title"),"real shared name normalization/update did not run");
        for (tool,args) in [
            ("set_thread_title",json!({"title":"  "})),
            ("set_thread_title",json!({"title":"ignored","unexpected":true})),
            ("set_thread_archived",json!({"threadId":h.parent_id.to_string().to_uppercase(),"archived":true})),
        ] {
            ensure!(serde_json::to_value(manage(&client,h.parent_id,tool,args).await?)?["isError"]==true,"original management rejection was lost");
        }
        let target:ThreadStartResponse=h.request(ConnectionId(41),&session,json!({"id":82,"method":"thread/start","params":{"ephemeral":false}})).await?;
        let id=target.thread.id;
        let archived=output(&manage(&client,h.parent_id,"set_thread_archived",json!({"threadId":id,"archived":true})).await?)?;
        ensure!(archived==json!({"threadId":id,"archived":true}),"archive result changed");
        ensure!(h.host.control.manager.get_thread(ThreadId::from_string(&id)?).await.is_err(),"actual archive helper did not remove target runtime");
        ensure!(h.host.control.management.archived_state_for_test(ThreadId::from_string(&id)?).await?,"archive did not reach actual persistent store");
        let restored=output(&manage(&client,h.parent_id,"set_thread_archived",json!({"threadId":id,"archived":false})).await?)?;
        ensure!(restored==json!({"threadId":id,"archived":false}),"unarchive result changed");
        ensure!(!h.host.control.management.archived_state_for_test(ThreadId::from_string(&id)?).await?,"unarchive did not update actual persistent store");
        management_count(&h,0).await?;
        ensure!(h.server_requests.load(Ordering::Relaxed)==0,"management requested a frontend/UI acknowledgement");
        Ok(())
    }.await;
    if let Some(client) = observed {
        client.shutdown().await;
    }
    result.and(h.shutdown().await)
}

#[tokio::test]
async fn accepted_http_title_survives_observer_loss_and_c1_replacement() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut observed = None;
    let mut held = None;
    let mut pending = None;
    let result:Result<()>=async {
        let client=h.client(false).await?;observed=Some(client.clone());
        held=Some(h.host.control.management.hold_actual_list_state_for_test().await?);
        let caller=client.clone();let parent=h.parent_id;
        pending=Some(tokio::spawn(async move {manage(&caller,parent,"set_thread_title",json!({"title":"Owned after observer loss"})).await}));
        management_count(&h,1).await?;
        // Acceptance plus real held permit proves pending owned work, not a
        // specific private helper-entry phase. Do NOT stop actual host/Input.
        if let Some(observer)=pending.as_ref(){observer.abort();}
        if let Some(observer)=pending.take(){let _=observer.await;}
        client.shutdown().await;
        bounded(h.processor.connection_closed(ConnectionId(41),&h.c1)).await?;
        let c2=Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        let _:InitializeResponse=h.request(ConnectionId(42),&c2,json!({"id":91,"method":"initialize","params":{"clientInfo":{"name":"management-c2","version":"1"}}})).await?;
        ensure!(Arc::ptr_eq(&h.parent,&h.host.control.manager.get_thread(h.parent_id).await?),"frontend replacement changed actual issuing runtime");
        ensure!(!h.input.is_stopped(),"test accidentally withdrew actual host input");
        management_count(&h,1).await?;
        drop(held.take());
        management_count(&h,0).await?;
        let actual:ThreadReadResponse=h.request(ConnectionId(42),&c2,json!({"id":92,"method":"thread/read","params":{"threadId":h.parent_id.to_string(),"includeTurns":false}})).await?;
        ensure!(actual.thread.name.as_deref()==Some("Owned after observer loss"),"accepted mutation was lost with its HTTP observer");
        ensure!(h.server_requests.load(Ordering::Relaxed)==0,"accepted mutation depended on frontend/UI acknowledgement");
        Ok(())
    }.await;
    // Always release real policy barrier BEFORE joining/draining accepted work,
    // even when an assertion/HTTP operation above failed.
    drop(held.take());
    if let Some(observer) = pending.take() {
        observer.abort();
        let _ = observer.await;
    }
    if let Some(client) = observed {
        client.shutdown().await;
    }
    result.and(h.shutdown().await)
}

#[tokio::test(flavor = "current_thread")]
async fn management_guard_losses_keep_actual_request_and_unknown_outcome() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        use crate::message_processor::alab_task_mcp::management::testing::GuardFailure;
        use crate::message_processor::alab_task_mcp::management::testing::inject_guard_failure;
        use crate::message_processor::alab_task_mcp::management::testing::unknown_record_retained;
        for failure in [
            GuardFailure::BeforePoll,
            GuardFailure::PollPanic,
            GuardFailure::DestructionPanic,
        ] {
            let request =
                crate::request_processors::retained_task_read::management::ManagementRequest {
                    parent_id: h.parent_id,
                    parent: h.parent.clone(),
                    context: OperationContext {
                        span: tracing::info_span!("actual_management_guard_control"),
                        parent_trace: None,
                        cancellation: h.host.control.shutdown.child_token(),
                    },
                    operation: retained_task_read::policy::management::prepare(
                        &h.parent_id.to_string(),
                        "set_thread_title",
                        json!({"title":"guard control never proves absence"}),
                    )
                    .map_err(anyhow::Error::msg)?,
                };
            let key = inject_guard_failure(&h.host.control, request, failure).await?;
            ensure!(
                unknown_record_retained(&h.host.control, key, h.parent_id, &h.parent),
                "unobserved management record/custody disappeared"
            );
        }
        management_count(&h, 3).await?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    let error = cleanup
        .err()
        .context("unknown management outcome was reported as a clean host stop")?;
    ensure!(
        error
            .chain()
            .any(|cause| cause.to_string() == "task management outcome remains unobserved"),
        "unrelated cleanup error cannot qualify Unknown handling"
    );
    Ok(())
}
