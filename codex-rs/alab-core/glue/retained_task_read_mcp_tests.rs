// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual private HTTP read/wait controls. Reuse the complete retained host's
//! real root START/binding fixture; no extra client facade or SDK claim.
use super::*;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::TurnStartResponse;

fn tool_json(result: &rmcp::model::CallToolResult) -> Result<Value> {
    let value = serde_json::to_value(result)?;
    ensure!(
        value["isError"] != true,
        "actual task tool returned an error: {value}"
    );
    let text = value["content"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["text"].as_str())
        .context("tool text missing")?;
    ensure!(
        text.len() <= 8_000,
        "actual tool exceeded original JSON budget"
    );
    serde_json::from_str(text).map_err(Into::into)
}

async fn call_read(
    client: &RmcpClient,
    parent_id: ThreadId,
    tool: &str,
    arguments: Value,
) -> Result<rmcp::model::CallToolResult> {
    client
        .call_tool(
            tool.into(),
            Some(arguments),
            move |_| Ok(Some(json!({"threadId":parent_id.to_string()}))),
            Some(Duration::from_secs(10)),
        )
        .await
}

// Observe the actual initialized C1 wire response, including typed errors.
// A fresh local thread has metadata but no materialized paginated turn record.
async fn actual_rpc_error(h: &mut Harness, request: Value) -> Result<JSONRPCErrorError> {
    let request_id = request.get("id").cloned().context("request id missing")?;
    let connection_id = ConnectionId(41);
    h.processor
        .process_request(
            connection_id,
            serde_json::from_value(request)?,
            &AppServerTransport::WebSocket {
                bind_address: "127.0.0.1:0".parse()?,
            },
            h.c1.clone(),
        )
        .await;
    bounded(async {
        loop {
            let envelope = h.messages.recv().await.context("actual outbox ended")?;
            let OutgoingEnvelope::ToConnection {
                connection_id: target,
                message,
                ..
            } = envelope
            else {
                continue;
            };
            if target != connection_id {
                continue;
            }
            match message {
                OutgoingMessage::Error(error) if serde_json::to_value(&error.id)? == request_id => {
                    return Ok(error.error);
                }
                OutgoingMessage::Response(response)
                    if serde_json::to_value(&response.id)? == request_id =>
                {
                    anyhow::bail!("fresh full-history RPC unexpectedly succeeded");
                }
                _ => {}
            }
        }
    })
    .await?
}

#[tokio::test]
async fn actual_http_read_catalog_live_fallback_and_original_bounds() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut observed = None;
    let result: Result<()> = async {
        let client = h.client(false).await?;
        observed = Some(client.clone());
        let catalog = client
            .list_tools(None, Some(Duration::from_secs(10)))
            .await?;
        let names = catalog
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<std::collections::HashSet<_>>();
        ensure!(
            names
                == std::collections::HashSet::from([
                    "create_thread",
                    "send_message_to_thread",
                    "fork_thread",
                    "list_threads",
                    "list_archived_threads",
                    "read_thread",
                    "wait_threads",
                    "set_thread_title",
                    "set_thread_archived"
                ]),
            "catalog contains missing/partial operations"
        );
        for tool in &catalog.tools {
            let actual = serde_json::to_value(tool)?;
            ensure!(
                actual["annotations"]["readOnlyHint"]
                    == matches!(
                        tool.name.as_ref(),
                        "list_threads" | "list_archived_threads" | "read_thread" | "wait_threads"
                    ),
                "actual read-only hint changed"
            );
        }
        let session = h.c1.clone();
        let parent_id = h.parent_id.to_string();
        let metadata: ThreadReadResponse = h.request(ConnectionId(41), &session, json!({
            "id":11,"method":"thread/read","params":{"threadId":parent_id,"includeTurns":false}
        })).await?;
        ensure!(
            metadata.thread.id == parent_id,
            "fresh metadata read changed the actual root"
        );
        ensure!(
            metadata.thread.turns.is_empty(),
            "metadata-only read hydrated full history"
        );
        let read_error = actual_rpc_error(
            &mut h,
            json!({
                "id":13,"method":"thread/read","params":{"threadId":parent_id,"includeTurns":true}
            }),
        )
        .await?;
        let output = call_read(
            &client,
            h.parent_id,
            "read_thread",
            json!({"threadId":parent_id}),
        )
        .await?;
        let wire = serde_json::to_value(output)?;
        ensure!(
            wire["isError"] == true,
            "fresh HTTP read fabricated full-history success"
        );
        let text = wire["content"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["text"].as_str())
            .context("fresh history error text missing")?;
        // The HTTP tool preserves the original typed server-error rendering
        // and budget for the real includeTurns fallback failure.
        let mut expected = format!(
            "thread/read failed: {} (code {})",
            read_error.message, read_error.code
        );
        if let Some(data) = read_error.data {
            expected.push_str(&format!(", data: {data}"));
        }
        ensure!(
            text == retained_task_read::policy::truncate(
                &expected,
                retained_task_read::policy::MAX_ERROR_CHARS
            ),
            "fresh HTTP full-history error diverged from the real typed RPC error"
        );
        let list = tool_json(&call_read(&client, h.parent_id, "list_threads", json!({})).await?)?;
        ensure!(
            list["schemaVersion"] == 4
                && list["pinnedThreads"] == json!([])
                && list["threads"].is_array(),
            "original active-list envelope changed"
        );
        let archived = tool_json(
            &call_read(
                &client,
                h.parent_id,
                "list_archived_threads",
                json!({"limit":1}),
            )
            .await?,
        )?;
        ensure!(
            archived["threads"].is_array() && archived.get("nextCursor").is_some(),
            "original archived cursor envelope changed"
        );
        for (tool, arguments) in [
            ("list_threads", json!({"cursor":"not-allowed"})),
            ("list_threads", json!({"limit":0})),
            ("list_archived_threads", json!({"limit":51})),
            (
                "read_thread",
                json!({"threadId":h.parent_id.to_string(),"turnLimit":11}),
            ),
            (
                "read_thread",
                json!({"threadId":h.parent_id.to_string(),"maxOutputCharsPerItem":20_001}),
            ),
        ] {
            let output = call_read(&client, h.parent_id, tool, arguments).await?;
            ensure!(
                serde_json::to_value(output)?["isError"] == true,
                "invalid task-read arguments were accepted"
            );
        }
        ensure!(
            h.server_requests.load(Ordering::Relaxed) == 0,
            "read-only operation requested a frontend response"
        );
        Ok(())
    }
    .await;
    if let Some(client) = observed {
        client.shutdown().await;
    }
    result.and(h.shutdown().await)
}

#[tokio::test]
async fn actual_http_wait_snapshot_target_errors_self_and_duplicate_rules() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut observed = None;
    let result: Result<()> = async {
        let client = h.client(false).await?;
        observed = Some(client.clone());
        let session = h.c1.clone();
        let target: ThreadStartResponse = h
            .request(
                ConnectionId(41),
                &session,
                json!({"id":31,"method":"thread/start","params":{"ephemeral":false}}),
            )
            .await?;
        let id = target.thread.id;
        let snapshot = tool_json(
            &call_read(
                &client,
                h.parent_id,
                "wait_threads",
                json!({"targets":[{"threadId":id}],"timeoutMs":0}),
            )
            .await?,
        )?;
        ensure!(
            snapshot["timedOut"] == false && snapshot["wake"]["reason"] == "inactiveStatus",
            "actual idle snapshot predicate changed"
        );
        ensure!(
            snapshot["polls"][0]["thread"]["id"] == id
                && snapshot["polls"][0]["cursor"].is_string(),
            "actual snapshot cursor/target changed"
        );
        let invalid = tool_json(
            &call_read(
                &client,
                h.parent_id,
                "wait_threads",
                json!({"targets":[{"threadId":"not-a-thread-id"}],"timeoutMs":0}),
            )
            .await?,
        )?;
        ensure!(
            invalid["polls"] == json!([])
                && invalid["timedOut"] == false
                && invalid["errors"][0]["threadId"] == "not-a-thread-id",
            "invalid target became a whole-call rejection or false timeout"
        );
        for arguments in [
            json!({"targets":[{"threadId":h.parent_id.to_string()}],"timeoutMs":0}),
            json!({"targets":[{"threadId":id},{"threadId":id.to_uppercase()}],"timeoutMs":0}),
            json!({"targets":[],"timeoutMs":0}),
            json!({"targets":[{"threadId":id}],"timeoutMs":120_001}),
        ] {
            let output = call_read(&client, h.parent_id, "wait_threads", arguments).await?;
            ensure!(
                serde_json::to_value(output)?["isError"] == true,
                "invalid wait arguments accepted"
            );
        }
        Ok(())
    }
    .await;
    if let Some(client) = observed {
        client.shutdown().await;
    }
    result.and(h.shutdown().await)
}

#[tokio::test]
async fn actual_http_wait_observes_real_turn_completion_and_owner_withdrawal() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("read-wait-turn"),
            responses::ev_assistant_message("read-wait-turn", "Complete"),
            responses::ev_completed("read-wait-turn"),
        ]),
    }]])
    .await;
    let mut h = match Harness::new_persistent(model.uri()).await {
        Ok(h) => h,
        Err(error) => {
            let _ = release.send(());
            model.shutdown().await;
            return Err(error);
        }
    };
    let mut release = Some(release);
    let mut observed = None;
    let mut pending_wait = None;
    let result: Result<()> = async {
        let client = h.client(false).await?; observed = Some(client.clone());
        let session = h.c1.clone();
        let target:ThreadStartResponse = h.request(ConnectionId(41),&session,json!({"id":41,"method":"thread/start","params":{"ephemeral":false}})).await?;
        let id = target.thread.id;
        let turn:TurnStartResponse = h.request(ConnectionId(41),&session,json!({"id":42,"method":"turn/start","params":{"threadId":id,"input":[{"type":"text","text":"Complete the task"}]}})).await?;
        bounded(model.wait_for_request_count(1)).await?;
        let initial=tool_json(&call_read(&client,h.parent_id,"wait_threads",json!({"targets":[{"threadId":id}],"timeoutMs":0})).await?)?;
        ensure!(initial["wake"].is_null() && initial["timedOut"] == true,"actual gated turn unexpectedly actionable");
        let cursor=initial["polls"][0]["cursor"].as_str().context("running cursor missing")?.to_string();
        // A positive deadline must finish through the actual HTTP tool while
        // the real target remains active behind its held model response.
        let timeout_started = tokio::time::Instant::now();
        let expired = tokio::time::timeout(
            Duration::from_secs(3),
            call_read(
                &client,
                h.parent_id,
                "wait_threads",
                json!({"targets":[{"threadId":id}],"timeoutMs":100}),
            ),
        )
        .await
        .context("positive HTTP wait deadline did not finish")??;
        let elapsed = timeout_started.elapsed();
        let expired = tool_json(&expired)?;
        ensure!(
            expired["timedOut"] == true && expired["wake"].is_null(),
            "positive HTTP wait deadline did not return its timeout envelope"
        );
        ensure!(
            expired["polls"].as_array().is_some_and(|polls| polls.len() == 1)
                && expired["polls"][0]["thread"]["id"] == id,
            "positive HTTP deadline lost the actual partial target snapshot"
        );
        ensure!(
            elapsed >= Duration::from_millis(100),
            "positive HTTP wait returned before its requested deadline"
        );
        let wait_client=client.clone();let parent=h.parent_id;let target_id=id.clone();
        pending_wait=Some(tokio::spawn(async move {call_read(&wait_client,parent,"wait_threads",json!({"targets":[{"threadId":target_id,"afterCursor":cursor}],"timeoutMs":5_000})).await}));
        let pending=tokio::time::timeout(Duration::from_millis(50),pending_wait.as_mut().context("wait absent")?).await;
        ensure!(pending.is_err(),"wait completed while real model turn remained gated");
        release.take().context("model release absent")?.send(()).map_err(|_|anyhow::anyhow!("model gate disappeared"))?;
        let output=bounded(pending_wait.as_mut().context("wait absent")?).await?.context("wait worker failed")??;
        pending_wait.take();
        let completed=tool_json(&output)?;
        ensure!(completed["timedOut"] == false && completed["wake"]["reason"] == "turnCompleted" && completed["wake"]["turnId"] == turn.turn.id,"wait did not observe the actual completed turn: {completed}");
        // This real completed turn materializes the local history record.
        // Require successful HTTP read/list projection of its actual contents.
        let read = tool_json(&call_read(
            &client, h.parent_id, "read_thread", json!({"threadId":id})
        ).await?)?;
        ensure!(
            read["schemaVersion"] == 1 && read["thread"]["id"] == id
                && read["page"]["order"] == "newest_first" && read["page"]["limit"] == 1,
            "materialized HTTP read changed the original page envelope"
        );
        let turns = read["turns"].as_array().context("actual materialized turns missing")?;
        ensure!(turns.len() == 1, "materialized read did not return the one actual turn");
        let actual_turn = &turns[0];
        ensure!(
            actual_turn["id"] == turn.turn.id && actual_turn["status"] == "completed",
            "materialized read lost the actual completed turn"
        );
        ensure!(
            actual_turn["items"].as_array().is_some_and(|items| items.iter().any(|item| {
                item["type"] == "agentMessage" && item["text"] == "Complete"
            })),
            "materialized HTTP read lost the actual model's assistant text"
        );
        // Account and attachment filtering remain the actual typed policy.
        // Compare to initialized C1 instead of inventing task-list eligibility.
        let params = retained_task_read::policy::list_params(
            &retained_task_read::policy::ListArguments { limit: None, cursor: None },
            false,
        ).map_err(anyhow::Error::msg)?;
        let session = h.c1.clone();
        let original: ThreadListResponse = h.request(ConnectionId(41), &session, json!({
            "id":43,"method":"thread/list","params":serde_json::to_value(params)?
        })).await?;
        let list = tool_json(&call_read(&client, h.parent_id, "list_threads", json!({})).await?)?;
        let actual_ids = list["threads"].as_array().context("HTTP active-list entries absent")?
            .iter().map(|thread| thread["id"].as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>().context("HTTP active-list ID absent")?;
        let expected_ids = original.data.iter().map(|thread| thread.id.clone()).collect::<Vec<_>>();
        ensure!(actual_ids == expected_ids, "HTTP active list changed actual typed account/attachment filtering");
        ensure!(h.server_requests.load(Ordering::Relaxed) == 0, "read/list required a frontend acknowledgement");
        // Same completed cursor does not replay completion. Cancel the
        // actual read executor future on its real operation token; this is a
        // component control using the already bound actual parent allocation.
        let cursor=completed["polls"][0]["cursor"].as_str().context("completed cursor absent")?.to_string();
        let cancellation=CancellationToken::new();
        let operation=OperationContext {span:tracing::info_span!("actual_read_wait_cancel_control"),parent_trace:None,cancellation:cancellation.clone()};
        let cancelled=h.host.control.reads.execute(h.parent_id,h.parent.clone(),operation,"wait_threads",json!({"targets":[{"threadId":id,"afterCursor":cursor}],"timeoutMs":120_000}));
        tokio::pin!(cancelled);
        ensure!(tokio::time::timeout(Duration::from_millis(50),&mut cancelled).await.is_err(),"unchanged completed cursor replayed completion");
        cancellation.cancel();
        ensure!(bounded(&mut cancelled).await?.is_err(),"actual cancelled read returned stale success");
        // An actual HTTP read denied during owner withdrawal must also never
        // return success, whether withdrawal wins admission or active polling.
        let wait_client=client.clone();let parent=h.parent_id;
        pending_wait=Some(tokio::spawn(async move {call_read(&wait_client,parent,"wait_threads",json!({"targets":[{"threadId":id,"afterCursor":cursor}],"timeoutMs":120_000})).await}));
        h.input.stop();
        let output=bounded(pending_wait.as_mut().context("withdrawal wait absent")?).await?.context("withdrawal worker failed")?;
        pending_wait.take();
        if let Ok(output)=output { ensure!(serde_json::to_value(output)?["isError"] == true,"actual owner withdrawal yielded a stale success"); }
        Ok(())
    }.await;
    if let Some(release) = release.take() {
        let _ = release.send(());
    }
    if let Some(wait) = pending_wait.as_mut() {
        let _ = bounded(wait).await;
    }
    if let Some(client) = observed {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(cleanup)
}
