// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual retained HTTP/resume/Core/turn controls; local model/MCP fixtures do
//! not establish physical Native frontend admission or ordinary TUI activation.
use super::*;
use codex_app_server_protocol::ThreadSetNameResponse;

async fn send(
    client: &RmcpClient,
    issuer: ThreadId,
    target: ThreadId,
    prompt: &str,
) -> Result<rmcp::model::CallToolResult> {
    client
        .call_tool(
            "send_message_to_thread".into(),
            Some(json!({"threadId":target.to_string(),"prompt":prompt})),
            move |_| Ok(Some(json!({"threadId":issuer.to_string()}))),
            Some(Duration::from_secs(90)),
        )
        .await
}

async fn persistent_target(h: &mut Harness) -> Result<(ThreadId, Arc<CodexThread>)> {
    let session = h.c1.clone();
    let started: ThreadStartResponse = h
        .request(
            ConnectionId(41),
            &session,
            json!({"id":70,"method":"thread/start","params":{"ephemeral":false}}),
        )
        .await?;
    let id = ThreadId::from_string(&started.thread.id)?;
    // Original live metadata mutation materializes the real persistent rollout,
    // without sending a model turn or synthesizing persistence/response data.
    let _: ThreadSetNameResponse = h.request(ConnectionId(41), &session,
        json!({"id":71,"method":"thread/name/set","params":{"threadId":id.to_string(),"name":"SEND target"}})).await?;
    Ok((id, h.host.control.manager.get_thread(id).await?))
}

async fn unload_actual(h: &Harness, id: ThreadId, thread: &Arc<CodexThread>) -> Result<()> {
    bounded(thread.shutdown_and_wait()).await??;
    h.host
        .control
        .manager
        .remove_thread_if_matches(&id, thread)
        .await;
    ensure!(
        h.host.control.manager.get_thread(id).await.is_err(),
        "actual target remained loaded"
    );
    bounded(async {
        loop {
            let exists = h
                .host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .bindings
                .values()
                .any(|(binding, _)| {
                    binding
                        .parent
                        .read()
                        .unwrap_or_else(PoisonError::into_inner)
                        .as_ref()
                        .is_some_and(|bound| Arc::ptr_eq(&bound.thread, thread))
                });
            if !exists {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

fn check_delegated(body: &[u8], issuer: ThreadId, prompt: &str) -> Result<()> {
    let body: Value = serde_json::from_slice(body)?;
    let outputs = body["input"]
        .as_array()
        .context("model input absent")?
        .iter()
        .filter(|item| {
            item["type"] == "function_call_output"
                && item["name"] == "send_message_to_thread"
                && item["namespace"] == "codex_tui"
        })
        .collect::<Vec<_>>();
    ensure!(
        outputs.len() == 1,
        "actual SEND delegated input absent or duplicated"
    );
    let expected = format!(
        "<codex_delegation>\n  <source_thread_id>{issuer}</source_thread_id>\n  <input>{prompt}</input>\n</codex_delegation>"
    );
    ensure!(
        outputs[0]["output"].as_str() == Some(expected.as_str()),
        "actual delegated input changed"
    );
    Ok(())
}

#[tokio::test]
async fn actual_http_send_allows_self_and_preserves_loaded_runtime_and_namespace() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("self-send"),
            responses::ev_assistant_message("self-send", "Done"),
            responses::ev_completed("self-send"),
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
    let mut client = None;
    let result: Result<()> = async {
        let actual = h.client(false).await?;
        client = Some(actual.clone());
        // Resume intentionally requires saved metadata. A real self-issued
        // tool call already has a turn; materialize this idle fixture through
        // the original metadata mutation instead of relaxing that policy.
        let session = h.c1.clone();
        let _: ThreadSetNameResponse = h
            .request(
                ConnectionId(41),
                &session,
                json!({"id":69,"method":"thread/name/set","params":{
                "threadId":h.parent_id.to_string(),"name":"Self SEND target"}}),
            )
            .await?;
        let namespace = h
            .parent
            .config()
            .await
            .mcp_servers
            .get()
            .get("codex_tui")
            .cloned();
        let response = send(
            &actual,
            h.parent_id,
            h.parent_id,
            "Continue <here> & report",
        )
        .await?;
        ensure!(
            returned_thread_id(&response)? == h.parent_id,
            "self SEND changed target"
        );
        bounded(model.wait_for_request_count(1)).await?;
        let requests = model.requests().await;
        ensure!(requests.len() == 1, "self SEND duplicated model work");
        check_delegated(
            &requests[0],
            h.parent_id,
            "Continue &lt;here&gt; &amp; report",
        )?;
        ensure!(
            Arc::ptr_eq(
                &h.parent,
                &h.host.control.manager.get_thread(h.parent_id).await?
            ),
            "self SEND replaced actual loaded runtime"
        );
        ensure!(
            h.parent.config().await.mcp_servers.get().get("codex_tui") == namespace.as_ref(),
            "loaded SEND replaced existing namespace"
        );
        ensure!(
            h.server_requests.load(Ordering::Relaxed) == 0,
            "self SEND requested frontend/UI acknowledgement"
        );
        Ok(())
    }
    .await;
    let _ = release.send(());
    if let Some(client) = client {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn actual_cold_send_survives_observer_loss_and_c1_close_during_required_mcp_start()
-> Result<()> {
    let (release, gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("cold-send"),
            responses::ev_assistant_message("cold-send", "Done"),
            responses::ev_completed("cold-send"),
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
    let barrier = h._home.path().join("allow-send-initialize");
    let pid_file = h._home.path().join("send-mcp.pid");
    let mut client = None;
    let mut observer = None;
    let mut closing = None;
    let result: Result<()> = async {
        let (target_id, old) = persistent_target(&mut h).await?;
        let before = old.config_snapshot().await;
        unload_actual(&h, target_id, &old).await?;
        let actual = h.client(false).await?;
        client = Some(actual.clone());
        let blocked = format!(
            r#"
[mcp_servers.blocked]
command = {}
required = true
startup_timeout_sec = 120
[mcp_servers.blocked.env]
MCP_TEST_INITIALIZE_BARRIER_FILE = {}
MCP_TEST_PID_FILE = {}
"#,
            toml::Value::String(stdio_server_bin()?),
            toml::Value::String(barrier.to_string_lossy().into_owned()),
            toml::Value::String(pid_file.to_string_lossy().into_owned())
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(h._home.path().join("config.toml"))?
            .write_all(blocked.as_bytes())?;
        let issuer = h.parent_id;
        let request_client = actual.clone();
        observer = Some(tokio::spawn(async move {
            send(&request_client, issuer, target_id, "Follow up once").await
        }));
        wait_for_path_exists(&pid_file, Duration::from_secs(10)).await?;
        let pid = wait_for_pid_file(&pid_file).await?;
        ensure!(
            core_test_support::process::process_is_alive(&pid)?,
            "actual cold MCP exited before release"
        );
        ensure!(!barrier.exists(), "cold MCP barrier released early");
        ensure!(
            model.requests().await.is_empty(),
            "SEND submitted before required MCP startup"
        );
        let accepted = {
            let registry = h
                .host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            ensure!(
                registry.sends.len() == 1,
                "SEND was not owned exactly once while blocked"
            );
            registry
                .sends
                .values()
                .next()
                .cloned()
                .context("accepted SEND absent")?
        };
        let task = observer.take().context("SEND observer absent")?;
        task.abort();
        ensure!(
            task.await.is_err_and(|error| error.is_cancelled()),
            "observer did not actually cancel"
        );
        let processor = h.processor.clone();
        let c1 = h.c1.clone();
        closing = Some(tokio::spawn(async move {
            processor.connection_closed(ConnectionId(41), &c1).await;
        }));
        // Transport admission closes before the original per-thread teardown
        // bookkeeping can acquire the cold resume's goal guard. Keep that real
        // callback owned while C2 connects; require it to join after release.
        bounded(async {
            while !h.c1.rpc_gate.is_closed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .context("C1 admission did not close during cold SEND")?;
        let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        let _: InitializeResponse = h
            .request(
                ConnectionId(42),
                &c2,
                json!({"id":80,"method":"initialize",
            "params":{"clientInfo":{"name":"send-c2","version":"1"}}}),
            )
            .await?;
        ensure!(
            actual
                .list_tools(None, Some(Duration::from_secs(10)))
                .await?
                .tools
                .iter()
                .any(|tool| tool.name.as_ref() == "send_message_to_thread"),
            "same service stopped after C1 close"
        );
        ensure!(
            accepted.accepted_turn_for_tests().is_none(),
            "blocked SEND already produced a turn"
        );
        std::fs::write(&barrier, b"release")?;
        bounded(model.wait_for_request_count(1)).await?;
        bounded(async {
            while accepted.accepted_turn_for_tests().is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let turn = accepted
            .accepted_turn_for_tests()
            .context("actual typed turn result absent")?;
        ensure!(
            !turn.turn.id.is_empty()
                && turn.turn.status == codex_app_server_protocol::TurnStatus::InProgress,
            "cold SEND did not retain actual accepted typed turn"
        );
        let target = h.host.control.manager.get_thread(target_id).await?;
        ensure!(
            !Arc::ptr_eq(&target, &old),
            "cold SEND reused terminated incarnation"
        );
        let after = target.config_snapshot().await;
        ensure!(
            after.cwd() == before.cwd()
                && after.model_provider_id == before.model_provider_id
                && after.model == before.model
                && after.sandbox_policy() == before.sandbox_policy(),
            "cold SEND lost original persisted settings"
        );
        let bound = h
            .host
            .control
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bindings
            .values()
            .any(|(binding, _)| {
                binding
                    .parent
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_ref()
                    .is_some_and(|bound| {
                        bound.id == target_id && Arc::ptr_eq(&bound.thread, &target)
                    })
            });
        ensure!(bound, "cold SEND target lacks its real same-host binding");
        let requests = model.requests().await;
        ensure!(
            requests.len() == 1,
            "observer loss caused duplicate SEND submission"
        );
        check_delegated(&requests[0], issuer, "Follow up once")?;
        ensure!(
            h.server_requests.load(Ordering::Relaxed) == 0,
            "cold SEND requested UI ACK"
        );
        ensure!(
            Arc::ptr_eq(&h.parent, &h.host.control.manager.get_thread(issuer).await?),
            "SEND replaced issuer"
        );
        Ok(())
    }
    .await;
    // Always release required MCP/model barriers before host drain, even when
    // an assertion fails while a real helper process is blocked.
    let release_mcp = std::fs::write(&barrier, b"release").context("release cold SEND MCP barrier");
    let _ = release.send(());
    if let Some(task) = observer {
        task.abort();
        let _ = task.await;
    }
    if let Some(client) = client {
        client.shutdown().await;
    }
    let closed = match closing {
        Some(mut task) => match bounded(&mut task).await {
            Ok(joined) => joined.context("C1 close task failed"),
            Err(error) => {
                task.abort();
                let _ = task.await;
                Err(error.context("C1 teardown did not finish after releasing cold SEND"))
            }
        },
        None => Ok(()),
    };
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(release_mcp).and(closed).and(cleanup)
}

#[tokio::test]
async fn cold_send_rejects_real_configured_namespace_before_core_and_keeps_original_boundaries()
-> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut client = None;
    let result: Result<()> = async {
        let (id, old) = persistent_target(&mut h).await?;
        unload_actual(&h, id, &old).await?;
        let actual = h.client(false).await?;
        client = Some(actual.clone());
        let collision = format!(
            "\n[mcp_servers.codex_tui]\ncommand = {}\n",
            toml::Value::String(stdio_server_bin()?)
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(h._home.path().join("config.toml"))?
            .write_all(collision.as_bytes())?;
        let rejected = send(
            &actual,
            h.parent_id,
            id,
            "Do not replace a configured namespace",
        )
        .await?;
        ensure!(
            rejected.is_error == Some(true),
            "cold SEND stole configured namespace"
        );
        ensure!(
            h.host.control.manager.get_thread(id).await.is_err(),
            "namespace refusal started a target Core"
        );
        ensure!(
            model
                .received_requests()
                .await
                .context("requests absent")?
                .is_empty(),
            "namespace refusal submitted model work"
        );
        let bad = actual
            .call_tool(
                "send_message_to_thread".into(),
                Some(json!({"threadId":h.parent_id.to_string(),"prompt":"safe","model":""})),
                move |_| Ok(None),
                Some(Duration::from_secs(10)),
            )
            .await?;
        ensure!(
            bad.is_error == Some(true),
            "SEND empty model boundary changed"
        );
        ensure!(
            Arc::ptr_eq(
                &h.parent,
                &h.host.control.manager.get_thread(h.parent_id).await?
            ),
            "rejected SEND replaced issuer"
        );
        Ok(())
    }
    .await;
    if let Some(client) = client {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}
