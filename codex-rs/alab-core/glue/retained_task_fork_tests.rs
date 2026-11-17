// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual Core invocation/approval, history-cutoff and observer-loss controls.
//! No test creates a per-call witness or a fake Core context.
use super::*;
use codex_app_server_protocol::TurnStartResponse;

async fn actual_issuer(h: &mut Harness) -> Result<(ThreadId, Arc<CodexThread>)> {
    actual_issuer_at(h, None).await
}

async fn actual_issuer_at(
    h: &mut Harness,
    model_uri: Option<&str>,
) -> Result<(ThreadId, Arc<CodexThread>)> {
    let mut config = serde_json::Map::new();
    config.insert(
        format!("features.{}", Feature::ToolCallMcpElicitation.key()),
        json!(false),
    );
    if let Some(uri) = model_uri {
        let provider = h.parent.config_snapshot().await.model_provider_id;
        config.insert(
            format!("model_providers.{provider}.base_url"),
            json!(format!("{uri}/v1")),
        );
    }
    let c1 = h.c1.clone();
    let started: ThreadStartResponse = h
        .request(
            ConnectionId(41),
            &c1,
            json!({
                "id":110,"method":"thread/start","params":{
                    "ephemeral":false,"approvalPolicy":"on-request","config":config
                }
            }),
        )
        .await?;
    let id = ThreadId::from_string(&started.thread.id)?;
    Ok((id, h.host.control.manager.get_thread(id).await?))
}

async fn start_actual_turn(
    h: &mut Harness,
    id: ThreadId,
    request: i64,
    text: &str,
) -> Result<TurnStartResponse> {
    let c1 = h.c1.clone();
    h.request(
        ConnectionId(41),
        &c1,
        json!({
            "id":request,"method":"turn/start","params":{
                "threadId":id.to_string(),"input":[{"type":"text","text":text}]
            }
        }),
    )
    .await
}

/// Answer the actual Source approval, preserving thread/turn/item identity and
/// its original JSON-RPC response ID. The model call frame must remain gated
/// until turn/start's response has been read, so request() cannot discard this.
async fn approve_actual_fork(
    h: &mut Harness,
    issuer: ThreadId,
    turn: &str,
    call: &str,
) -> Result<()> {
    let response = bounded(async {
        loop {
            let envelope = h.messages.recv().await.context("approval outbox closed")?;
            let OutgoingEnvelope::ToConnection {
                connection_id,
                message,
                ..
            } = envelope
            else {
                continue;
            };
            if connection_id != ConnectionId(41) {
                continue;
            }
            let OutgoingMessage::Request(request) = message else {
                continue;
            };
            let request = serde_json::to_value(request)?;
            ensure!(
                request["method"] == "item/tool/requestUserInput",
                "unexpected frontend request; no synthetic acknowledgement allowed"
            );
            let params = &request["params"];
            ensure!(
                params["threadId"] == issuer.to_string()
                    && params["turnId"] == turn
                    && params["itemId"] == call,
                "approval does not belong to actual issuing call"
            );
            let questions = params["questions"]
                .as_array()
                .context("approval questions absent")?;
            ensure!(questions.len() == 1, "unexpected fork approval questions");
            let question = &questions[0];
            ensure!(
                question["options"]
                    .as_array()
                    .context("approval options absent")?
                    .iter()
                    .any(|option| option["label"] == "Allow"),
                "actual approval lacks one-call Allow"
            );
            let id = question["id"]
                .as_str()
                .context("actual approval question ID absent")?;
            let mut answers = serde_json::Map::new();
            answers.insert(id.to_owned(), json!({"answers":["Allow"]}));
            return serde_json::from_value::<codex_app_server_protocol::JSONRPCResponse>(json!({
                "id":request["id"],"result":{"answers":answers}
            }))
            .map_err(Into::into);
        }
    })
    .await??;
    h.processor
        .process_response(ConnectionId(41), response)
        .await;
    Ok(())
}

async fn actual_idle(thread: &Arc<CodexThread>) -> Result<()> {
    bounded(async {
        while thread.has_active_turn().await {
            tokio::task::yield_now().await;
        }
    })
    .await
}

fn no_fork_result(result: Result<rmcp::model::CallToolResult>) -> Result<()> {
    let error = match result {
        Err(error) => error,
        Ok(reply) => anyhow::bail!(
            "expected pre-accept protocol refusal, got tool result is_error={:?}",
            reply.is_error
        ),
    };
    let protocol = codex_rmcp_client::mcp_error(&error)
        .with_context(|| format!("fork rejection lacked structured MCP error: {error}"))?;
    ensure!(
        serde_json::to_value(protocol.code)? == json!(-32602),
        "fork rejection had unexpected protocol code"
    );
    Ok(())
}

#[tokio::test]
async fn raw_http_thread_turn_and_call_labels_cannot_issue_a_fork() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let h = Harness::new_persistent(&model.uri()).await?;
    let mut observed = None;
    let result: Result<()> = async {
        let client = h.client(false).await?;
        observed = Some(client.clone());
        let before = h.host.control.manager.list_thread_ids().await;
        let actual_parent = h.parent_id;
        no_fork_result(
            client
                .call_tool(
                    "fork_thread".into(),
                    Some(json!({})),
                    move |_| {
                        Ok(Some(json!({"threadId":actual_parent.to_string(),
                "turnId":"caller-selected-turn","callId":"caller-selected-call"})))
                    },
                    Some(Duration::from_secs(10)),
                )
                .await,
        )?;
        let wrong_parent = ThreadId::new();
        no_fork_result(
            client
                .call_tool(
                    "fork_thread".into(),
                    Some(json!({})),
                    move |_| {
                        Ok(Some(json!({"threadId":wrong_parent.to_string(),
                "turnId":"caller-selected-turn","callId":"caller-selected-call"})))
                    },
                    Some(Duration::from_secs(10)),
                )
                .await,
        )?;
        ensure!(
            h.host.control.accepted_forks_for_tests().is_empty(),
            "raw labels registered trusted fork work"
        );
        ensure!(
            h.host.control.manager.list_thread_ids().await == before,
            "raw-origin rejection created a runtime"
        );
        ensure!(
            model
                .received_requests()
                .await
                .context("model requests absent")?
                .is_empty(),
            "raw fork labels triggered model work"
        );
        Ok(())
    }
    .await;
    if let Some(client) = observed {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

async fn actual_child_turn_ids(
    h: &mut Harness,
    child: &codex_app_server_protocol::ThreadForkResponse,
    c2: &Arc<ConnectionSessionState>,
) -> Result<std::collections::HashSet<String>> {
    let ids = if child.thread.history_mode
        == codex_app_server_protocol::ThreadHistoryMode::Paginated
    {
        let page: codex_app_server_protocol::ThreadTurnsListResponse = h.request(ConnectionId(42), c2,
            json!({"id":190,"method":"thread/turns/list","params":{
                "threadId":child.thread.id,"cursor":null,"limit":100,"sortDirection":"desc","itemsView":"summary"
            }})).await?;
        ensure!(
            page.next_cursor.is_none(),
            "small control unexpectedly needs another history page"
        );
        page.data.into_iter().map(|turn| turn.id).collect()
    } else {
        let read: codex_app_server_protocol::ThreadReadResponse = h.request(ConnectionId(42), c2,
            json!({"id":190,"method":"thread/read","params":{"threadId":child.thread.id,"includeTurns":true}})).await?;
        read.thread.turns.into_iter().map(|turn| turn.id).collect()
    };
    Ok(ids)
}

#[tokio::test]
async fn real_core_self_fork_retains_issuing_turn_after_observer_loss_and_a_later_turn()
-> Result<()> {
    let (call_frame, call_gate) = oneshot::channel();
    let (later_frame, later_gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_response_created("seed"),
                responses::ev_assistant_message("seed-message", "A completed seed"),
                responses::ev_completed("seed"),
            ]),
        }],
        vec![StreamingSseChunk {
            gate: Some(call_gate),
            body: responses::sse(vec![
                responses::ev_response_created("self-fork"),
                responses::ev_function_call_with_namespace(
                    "actual-self-fork",
                    "mcp__codex_tui",
                    "fork_thread",
                    "{}",
                ),
                responses::ev_completed("self-fork"),
            ]),
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_response_created("observation-ended"),
                responses::ev_assistant_message("observed-error", "The observation ended"),
                responses::ev_completed("observation-ended"),
            ]),
        }],
        vec![StreamingSseChunk {
            gate: Some(later_gate),
            body: responses::sse(vec![
                responses::ev_response_created("later"),
                responses::ev_assistant_message("later-message", "Later turn"),
                responses::ev_completed("later"),
            ]),
        }],
    ])
    .await;
    let mut call_frame = Some(call_frame);
    let mut later_frame = Some(later_frame);
    let mut h = match Harness::new_persistent(model.uri()).await {
        Ok(h) => h,
        Err(error) => {
            if let Some(gate) = call_frame.take() {
                let _ = gate.send(());
            }
            if let Some(gate) = later_frame.take() {
                let _ = gate.send(());
            }
            model.shutdown().await;
            return Err(error);
        }
    };
    let fork_gate = h.host.control.install_fork_gate_for_tests();
    let mut observed_clients = Vec::new();
    let result: Result<()> = async {
        let (issuer_id, issuer) = actual_issuer(&mut h).await?;
        let seed = start_actual_turn(&mut h, issuer_id, 120, "Record a completed seed").await?;
        bounded(model.wait_for_request_count(1)).await?;
        actual_idle(&issuer).await?;
        let issuing = start_actual_turn(&mut h, issuer_id, 121, "Fork this task").await?;
        bounded(model.wait_for_request_count(2)).await?;
        let _ = call_frame
            .take()
            .context("call frame already released")?
            .send(());
        approve_actual_fork(&mut h, issuer_id, &issuing.turn.id, "actual-self-fork").await?;
        bounded(fork_gate.wait_entered()).await?;
        let accepted = h.host.control.accepted_forks_for_tests();
        ensure!(
            accepted.len() == 1,
            "real Core invocation was not accepted exactly once"
        );
        let accepted = accepted[0].clone();
        let (turn, call, cutoff, response, born) = accepted.observed_for_tests();
        ensure!(
            turn == issuing.turn.id && call == "actual-self-fork",
            "hook did not retain actual issuing context"
        );
        ensure!(
            cutoff.is_none(),
            "accepted worker selected self cutoff before controlled delay"
        );
        ensure!(
            response.is_none() && born.is_none(),
            "before-Core gate already published a child"
        );
        // This is the real accepted HTTP observer's cancellation token. The
        // test never manufactures an invocation witness or cancels Core/owner.
        accepted.cancel_observer_for_tests();
        bounded(model.wait_for_request_count(3)).await?;
        actual_idle(&issuer).await?;
        let later = start_actual_turn(&mut h, issuer_id, 122, "A later actual turn").await?;
        bounded(model.wait_for_request_count(4)).await?;
        ensure!(
            later.turn.id != issuing.turn.id && issuer.has_active_turn().await,
            "later turn did not actually become active"
        );
        bounded(h.processor.connection_closed(ConnectionId(41), &h.c1)).await?;
        let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        let _: InitializeResponse = h
            .request(
                ConnectionId(42),
                &c2,
                json!({"id":123,"method":"initialize",
            "params":{"clientInfo":{"name":"fork-c2","version":"1"}}}),
            )
            .await?;
        ensure!(
            Arc::ptr_eq(
                &issuer,
                &h.host.control.manager.get_thread(issuer_id).await?
            ),
            "frontend close replaced actual issuer"
        );
        fork_gate.release();
        let response = bounded(accepted.wait_result_for_tests())
            .await?
            .map_err(|error| anyhow::anyhow!("actual retained fork failed: {error}"))?;
        let (turn, _, cutoff, stored, born) = accepted.observed_for_tests();
        ensure!(
            turn == issuing.turn.id && cutoff == Some(Some(issuing.turn.id.clone())),
            "later turn changed immutable cutoff"
        );
        ensure!(
            stored
                .as_ref()
                .is_some_and(|stored| Arc::ptr_eq(stored, &response)),
            "fork did not retain the same typed result"
        );
        let child_id = ThreadId::from_string(&response.thread.id)?;
        ensure!(child_id != issuer_id, "fork returned its source");
        let child = h.host.control.manager.get_thread(child_id).await?;
        ensure!(
            born.as_ref().is_some_and(|born| Arc::ptr_eq(born, &child)),
            "typed result lost the exact born Core Arc"
        );
        let turns = actual_child_turn_ids(&mut h, &response, &c2).await?;
        ensure!(
            turns.contains(&seed.turn.id),
            "fork lost completed seed history"
        );
        ensure!(
            !turns.contains(&issuing.turn.id) && !turns.contains(&later.turn.id),
            "self fork copied issuing/later turn"
        );
        // Resolve only the actual hook-issued claim already stored with this
        // exact accepted object. No test constructs a per-call witness.
        let claim = {
            let registry = h
                .host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            registry
                .fork_calls
                .iter()
                .find_map(|(claim, entry)| {
                    entry
                        .accepted
                        .as_ref()
                        .is_some_and(|(_, operation)| Arc::ptr_eq(operation, &accepted))
                        .then_some(*claim)
                })
                .context("actual accepted claim missing")?
        };
        let issuer_client = Harness::client_for_thread(&issuer, false).await?;
        observed_clients.push(issuer_client.clone());
        let before = h
            .host
            .control
            .manager
            .list_thread_ids()
            .await
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let replay_claim = claim.to_string();
        let replay = issuer_client
            .call_tool(
                "fork_thread".into(),
                Some(json!({})),
                move |_| {
                    Ok(Some(json!({"threadId":issuer_id.to_string(),
                "x-alab-task-fork-invocation":replay_claim})))
                },
                Some(Duration::from_secs(10)),
            )
            .await?;
        ensure!(
            returned_thread_id(&replay)? == child_id,
            "same genuine invocation replay forked another child"
        );
        let changed_claim = claim.to_string();
        no_fork_result(
            issuer_client
                .call_tool(
                    "fork_thread".into(),
                    Some(json!({"threadId":h.parent_id.to_string()})),
                    move |_| {
                        Ok(Some(json!({"threadId":issuer_id.to_string(),
                "x-alab-task-fork-invocation":changed_claim})))
                    },
                    Some(Duration::from_secs(10)),
                )
                .await,
        )?;
        let root_client = h.client(false).await?;
        observed_clients.push(root_client.clone());
        let root_id = h.parent_id;
        let wrong_binding_claim = claim.to_string();
        no_fork_result(
            root_client
                .call_tool(
                    "fork_thread".into(),
                    Some(json!({})),
                    move |_| {
                        Ok(Some(json!({"threadId":root_id.to_string(),
                "x-alab-task-fork-invocation":wrong_binding_claim})))
                    },
                    Some(Duration::from_secs(10)),
                )
                .await,
        )?;
        ensure!(
            h.host
                .control
                .manager
                .list_thread_ids()
                .await
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                == before,
            "claim replay or binding/argument rejection created another runtime"
        );
        ensure!(
            h.host.control.accepted_forks_for_tests().len() == 1,
            "replay duplicated accepted work"
        );
        ensure!(
            model.requests().await.len() == 4,
            "fork started model work or replayed invocation"
        );
        Ok(())
    }
    .await;
    // Release actual worker and all model frames before joining host/Core,
    // even when the test failed during approval or observer cancellation.
    fork_gate.release();
    if let Some(gate) = call_frame.take() {
        let _ = gate.send(());
    }
    if let Some(gate) = later_frame.take() {
        let _ = gate.send(());
    }
    for client in observed_clients {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(cleanup)
}

#[derive(Clone, Copy, Debug)]
enum OtherActiveHistory {
    Paginated,
    Legacy,
    UnsupportedPaginated,
}

async fn other_active_cutoff(history: OtherActiveHistory) -> Result<()> {
    let (active_frame, active_gate) = oneshot::channel();
    let (call_frame, call_gate) = oneshot::channel();
    let (target_model, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_response_created("target-seed"),
                responses::ev_assistant_message("target-seed-item", "Target seed"),
                responses::ev_completed("target-seed"),
            ]),
        }],
        vec![StreamingSseChunk {
            gate: Some(active_gate),
            body: responses::sse(vec![
                responses::ev_response_created("active-target"),
                responses::ev_assistant_message("target-active-item", "Still active"),
                responses::ev_completed("active-target"),
            ]),
        }],
    ])
    .await;
    let mut active_frame = Some(active_frame);
    let mut call_frame = Some(call_frame);
    let mut call_gate = Some(call_gate);
    let mut issuer_model = None;
    let created = match history {
        OtherActiveHistory::Paginated => Harness::new_persistent(target_model.uri()).await,
        OtherActiveHistory::Legacy => Harness::new_legacy(target_model.uri()).await,
        OtherActiveHistory::UnsupportedPaginated => Harness::new(target_model.uri()).await,
    };
    let mut h = match created {
        Ok(h) => h,
        Err(error) => {
            if let Some(gate) = active_frame.take() {
                let _ = gate.send(());
            }
            if let Some(gate) = call_frame.take() {
                let _ = gate.send(());
            }
            target_model.shutdown().await;
            return Err(error);
        }
    };
    let fork_gate = h.host.control.install_fork_gate_for_tests();
    let result: Result<()> = async {
        let target_id = h.parent_id;
        let target = h.parent.clone();
        let expected_history = match history {
            OtherActiveHistory::Legacy => codex_app_server_protocol::ThreadHistoryMode::Legacy,
            OtherActiveHistory::Paginated | OtherActiveHistory::UnsupportedPaginated => {
                codex_app_server_protocol::ThreadHistoryMode::Paginated
            }
        };
        let actual_history: codex_app_server_protocol::ThreadHistoryMode =
            target.config_snapshot().await.history_mode.into();
        ensure!(
            h.root.thread.history_mode == expected_history && actual_history == expected_history,
            "actual target history mode does not match {history:?}"
        );
        let seed = start_actual_turn(&mut h, target_id, 140, "Target completed history").await?;
        bounded(target_model.wait_for_request_count(1)).await?;
        actual_idle(&target).await?;
        let active = start_actual_turn(&mut h, target_id, 141, "Keep target active").await?;
        bounded(target_model.wait_for_request_count(2)).await?;
        ensure!(
            target.has_active_turn().await,
            "target is not actually active"
        );
        // The target ID is learned from real START, then a genuine new issuer
        // receives its own local model endpoint through ordinary config input.
        // No ID prediction, synthetic per-call context or new server framework.
        let arguments = json!({"threadId":target_id.to_string()}).to_string();
        let (model, _) = start_streaming_sse_server(vec![
            vec![StreamingSseChunk {
                gate: call_gate.take(),
                body: responses::sse(vec![
                    responses::ev_response_created("other-fork"),
                    responses::ev_function_call_with_namespace(
                        "actual-other-fork",
                        "mcp__codex_tui",
                        "fork_thread",
                        &arguments,
                    ),
                    responses::ev_completed("other-fork"),
                ]),
            }],
            vec![StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![
                    responses::ev_response_created("fork-done"),
                    responses::ev_assistant_message("fork-done-item", "Fork returned"),
                    responses::ev_completed("fork-done"),
                ]),
            }],
        ])
        .await;
        issuer_model = Some(model);
        let model = issuer_model.as_ref().context("issuer model absent")?;
        let (issuer_id, issuer) = actual_issuer_at(&mut h, Some(model.uri())).await?;
        let issuing = start_actual_turn(&mut h, issuer_id, 142, "Fork another active task").await?;
        bounded(model.wait_for_request_count(1)).await?;
        let _ = call_frame
            .take()
            .context("call frame already released")?
            .send(());
        approve_actual_fork(&mut h, issuer_id, &issuing.turn.id, "actual-other-fork").await?;
        bounded(fork_gate.wait_entered()).await?;
        let accepted = h.host.control.accepted_forks_for_tests();
        ensure!(
            accepted.len() == 1,
            "other-source Core invocation was not accepted once"
        );
        let accepted = accepted[0].clone();
        let (turn, call, cutoff, response, born) = accepted.observed_for_tests();
        ensure!(
            turn == issuing.turn.id && call == "actual-other-fork",
            "other-source fork lost actual issuing witness"
        );
        ensure!(
            cutoff.is_none(),
            "accepted worker selected other-source cutoff before controlled delay"
        );
        ensure!(
            response.is_none() && born.is_none(),
            "pre-Core gate already produced a child"
        );
        let before = h
            .host
            .control
            .manager
            .list_thread_ids()
            .await
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        fork_gate.release();
        if matches!(history, OtherActiveHistory::UnsupportedPaginated) {
            let error = match bounded(accepted.wait_result_for_tests()).await? {
                Ok(_) => {
                    anyhow::bail!("paginated history without its database unexpectedly forked")
                }
                Err(error) => error,
            };
            ensure!(
                error == "thread/read failed: list_turns is not supported yet (code -32601)",
                "actual unsupported history error changed: {error}"
            );
            let (_, _, cutoff, stored, born) = accepted.observed_for_tests();
            ensure!(
                cutoff.is_none() && stored.is_none() && born.is_none(),
                "unsupported history produced a cutoff, result or captured child"
            );
        } else {
            let response = bounded(accepted.wait_result_for_tests())
                .await?
                .map_err(|error| anyhow::anyhow!("actual other-source fork failed: {error}"))?;
            let (_, _, cutoff, stored, born) = accepted.observed_for_tests();
            ensure!(
                cutoff == Some(Some(active.turn.id.clone())),
                "active snapshot cutoff changed after admission"
            );
            ensure!(
                stored
                    .as_ref()
                    .is_some_and(|stored| Arc::ptr_eq(stored, &response)),
                "fork result was reconstructed"
            );
            let child_id = ThreadId::from_string(&response.thread.id)?;
            let child = h.host.control.manager.get_thread(child_id).await?;
            ensure!(
                child_id != target_id
                    && child_id != issuer_id
                    && born.as_ref().is_some_and(|born| Arc::ptr_eq(born, &child)),
                "fork did not retain exact new child"
            );
            let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
            let _: InitializeResponse = h
                .request(
                    ConnectionId(42),
                    &c2,
                    json!({"id":143,"method":"initialize",
                "params":{"clientInfo":{"name":"other-fork-c2","version":"1"}}}),
                )
                .await?;
            let turns = actual_child_turn_ids(&mut h, &response, &c2).await?;
            ensure!(
                turns.contains(&seed.turn.id) && !turns.contains(&active.turn.id),
                "other-source fork copied active turn or lost completed history"
            );
            ensure!(!child.has_active_turn().await, "fork started a child turn");
        }
        ensure!(
            Arc::ptr_eq(
                &target,
                &h.host.control.manager.get_thread(target_id).await?
            ),
            "fork replaced active source"
        );
        ensure!(
            Arc::ptr_eq(
                &issuer,
                &h.host.control.manager.get_thread(issuer_id).await?
            ),
            "fork replaced its issuing runtime"
        );
        ensure!(
            target.has_active_turn().await,
            "fork stopped the active source"
        );
        bounded(model.wait_for_request_count(2)).await?;
        actual_idle(&issuer).await?;
        ensure!(
            target_model.requests().await.len() == 2 && model.requests().await.len() == 2,
            "fork added model work or replayed a tool invocation"
        );
        if matches!(history, OtherActiveHistory::UnsupportedPaginated) {
            ensure!(
                h.host
                    .control
                    .manager
                    .list_thread_ids()
                    .await
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>()
                    == before,
                "unsupported history changed the actual runtime inventory"
            );
        }
        Ok(())
    }
    .await;
    fork_gate.release();
    if let Some(gate) = active_frame.take() {
        let _ = gate.send(());
    }
    if let Some(gate) = call_frame.take() {
        let _ = gate.send(());
    }
    let cleanup = h.shutdown().await;
    if let Some(model) = issuer_model {
        model.shutdown().await;
    }
    target_model.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn real_core_other_active_fork_uses_paginated_cutoff() -> Result<()> {
    other_active_cutoff(OtherActiveHistory::Paginated).await
}

#[tokio::test]
async fn real_core_other_active_fork_preserves_explicit_legacy_history() -> Result<()> {
    // Current Source can list legacy rollout turns directly. This proves
    // actual legacy compatibility, not an old server's unsupported-API branch.
    other_active_cutoff(OtherActiveHistory::Legacy).await
}

#[tokio::test]
async fn real_core_other_active_fork_preserves_actual_unsupported_paginated_history_error()
-> Result<()> {
    // The actual None-DB store cannot hydrate paginated history. Preserve the
    // original production fallback's observed error rather than invent support.
    other_active_cutoff(OtherActiveHistory::UnsupportedPaginated).await
}
