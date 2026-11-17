// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real processor, original source-builder resource, private HTTP MCP, actual
//! Git checkout and actual attachment store. No Native/SDK/ordinary TUI claim.
use super::*;
use std::path::Path;
use std::path::PathBuf;

fn git(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args([
            "-c",
            "user.name=Worktree fixture",
            "-c",
            "user.email=worktree@example.invalid",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "actual fixture git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
async fn source_repository(h: &mut Harness) -> Result<PathBuf> {
    let environments = h
        .root
        .thread
        .environments
        .as_ref()
        .context("actual root environments absent")?;
    let selected = environments
        .first()
        .context("actual root environment absent")?;
    ensure!(selected.environment_id == codex_exec_server::LOCAL_ENVIRONMENT_ID);
    let cwd = PathBuf::from(selected.cwd.as_str());
    git(&cwd, &["init", "--initial-branch=main"])?;
    std::fs::write(cwd.join("tracked.txt"), "base\n")?;
    git(&cwd, &["add", "."])?;
    git(&cwd, &["commit", "-m", "original default base"])?;
    git(&cwd, &["update-ref", "refs/remotes/origin/main", "HEAD"])?;
    git(
        &cwd,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )?;
    git(&cwd, &["checkout", "-b", "different-current-branch"])?;
    std::fs::write(cwd.join("tracked.txt"), "feature commit\n")?;
    git(&cwd, &["add", "."])?;
    git(&cwd, &["commit", "-m", "different current branch"])?;
    std::fs::write(cwd.join("tracked.txt"), "uncommitted source edits\n")?;
    // Materialize the real root through the original typed name mutation;
    // attachments deliberately retain the store's saved-thread requirement.
    let c1 = h.c1.clone();
    let _: codex_app_server_protocol::ThreadSetNameResponse = h.request(ConnectionId(41), &c1,
        json!({"id":710,"method":"thread/name/set","params":{"threadId":h.parent_id.to_string(),"name":"Actual worktree issuer"}})).await?;
    Ok(cwd)
}
async fn call(
    client: &RmcpClient,
    id: ThreadId,
    tool: &str,
    arguments: Value,
) -> Result<rmcp::model::CallToolResult> {
    client
        .call_tool(
            tool.into(),
            Some(arguments),
            move |_| Ok(Some(json!({"threadId":id.to_string()}))),
            Some(Duration::from_secs(20)),
        )
        .await
}
fn value(result: &rmcp::model::CallToolResult) -> Result<Value> {
    let result = serde_json::to_value(result)?;
    ensure!(
        result["isError"] != true,
        "actual worktree tool failed: {result}"
    );
    let text = result["content"][0]["text"]
        .as_str()
        .context("actual worktree text absent")?;
    ensure!(text.len() <= 999, "original output budget changed");
    Ok(serde_json::from_str(text)?)
}
fn error_text(result: &rmcp::model::CallToolResult) -> Result<String> {
    let result = serde_json::to_value(result)?;
    ensure!(
        result["isError"] == true,
        "expected original policy refusal: {result}"
    );
    Ok(result["content"][0]["text"]
        .as_str()
        .context("actual error text absent")?
        .to_owned())
}

#[tokio::test]
async fn actual_worktree_job_survives_http_observer_loss_and_frontend_replacement() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent_worktrees(&model.uri()).await?;
    let gate = h.host.control.install_worktree_gate_for_tests();
    let mut client = None;
    let mut first = None;
    let mut duplicate = None;
    let result: Result<()> = async {
        let cwd = source_repository(&mut h).await?;
        let before = h.parent.config_snapshot().await;
        let actual_client = h.client(false).await?;
        client = Some(actual_client.clone());
        let catalog = actual_client
            .list_tools(None, Some(Duration::from_secs(10)))
            .await?;
        for name in [
            "create_worktree",
            "get_worktree_creation_status",
            "list_worktrees",
        ] {
            let tool = catalog
                .tools
                .iter()
                .find(|tool| tool.name.as_ref() == name)
                .context("complete worktree tool absent")?;
            ensure!(
                serde_json::to_value(tool)?["annotations"]["readOnlyHint"]
                    == (name != "create_worktree")
            );
        }
        let original = actual_client.clone();
        let id = h.parent_id;
        first = Some(tokio::spawn(async move {
            call(&original, id, "create_worktree", json!({"allowAsync":true})).await
        }));
        bounded(gate.wait_entered()).await?;
        let accepted = h.host.control.accepted_worktrees_for_tests();
        ensure!(
            accepted.len() == 1,
            "actual creation not accepted exactly once"
        );
        let (operation_id, operation) = accepted[0].clone();
        if let Some(observer) = first.take() {
            observer.abort();
            let _ = observer.await;
        }
        bounded(h.processor.connection_closed(ConnectionId(41), &h.c1)).await?;
        let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        let _: InitializeResponse = h
            .request(
                ConnectionId(42),
                &c2,
                json!({"id":711,"method":"initialize",
            "params":{"clientInfo":{"name":"worktree-c2","version":"1"}}}),
            )
            .await?;
        let catalog = actual_client
            .list_tools(None, Some(Duration::from_secs(10)))
            .await?;
        ensure!(
            catalog
                .tools
                .iter()
                .any(|tool| tool.name.as_ref() == "create_worktree"),
            "same private service did not survive C1 close"
        );
        let pending = value(
            &call(
                &actual_client,
                id,
                "get_worktree_creation_status",
                json!({"operationId":operation_id.to_string()}),
            )
            .await?,
        )?;
        ensure!(pending == json!({"type":"pending","operationId":operation_id.to_string()}));
        let again = actual_client.clone();
        duplicate = Some(tokio::spawn(async move {
            call(
                &again,
                id,
                "create_worktree",
                json!({"allowAsync":true,"ref":"HEAD"}),
            )
            .await
        }));
        bounded(async {
            while operation.requests_for_tests() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        ensure!(
            h.host.control.accepted_worktrees_for_tests().len() == 1,
            "pending-job reuse created duplicate work"
        );
        gate.release();
        let observer = duplicate.take().context("duplicate observer absent")?;
        let pending = value(&bounded(observer).await???)?;
        ensure!(pending["type"] == "pending" && pending["operationId"] == operation_id.to_string());
        bounded(operation.wait_for_tests()).await?;
        let completed = value(
            &call(
                &actual_client,
                id,
                "get_worktree_creation_status",
                json!({"operationId":operation_id.to_string()}),
            )
            .await?,
        )?;
        ensure!(
            completed["type"] == "created",
            "actual checkout registration failed: {completed}"
        );
        let root = PathBuf::from(
            completed["worktree"]["root"]
                .as_str()
                .context("retained root absent")?,
        );
        let workspace = PathBuf::from(
            completed["worktree"]["workspaceRoot"]
                .as_str()
                .context("workspace root absent")?,
        );
        let captured = operation
            .created_for_tests()
            .context("actual checkout custody absent")?;
        ensure!(captured.root == root && captured.cwd == workspace);
        ensure!(
            std::fs::read_to_string(workspace.join("tracked.txt"))? == "base\n",
            "default base changed to current branch or copied dirty edits"
        );
        ensure!(std::fs::read_to_string(cwd.join("tracked.txt"))? == "uncommitted source edits\n");
        let listed = value(&call(&actual_client, id, "list_worktrees", json!({})).await?)?;
        ensure!(
            listed["data"]
                .as_array()
                .context("attachment list absent")?
                .iter()
                .any(|item| item["root"] == completed["worktree"]["root"]),
            "real typed attachment was not retained"
        );
        let after = h.parent.config_snapshot().await;
        ensure!(
            before.cwd() == after.cwd()
                && before.approval_policy == after.approval_policy
                && before.permission_profile == after.permission_profile
                && before.workspace_roots == after.workspace_roots,
            "worktree changed issuing cwd or permissions"
        );
        ensure!(
            Arc::ptr_eq(&h.parent, &h.host.control.manager.get_thread(id).await?),
            "frontend replacement changed issuing runtime"
        );
        ensure!(
            h.server_requests.load(Ordering::Relaxed) == 0,
            "worktree requested fabricated frontend ACK"
        );
        ensure!(
            model
                .received_requests()
                .await
                .context("model requests unavailable")?
                .is_empty()
        );
        Ok(())
    }
    .await;
    gate.release();
    for observer in [first, duplicate].into_iter().flatten() {
        observer.abort();
        let _ = observer.await;
    }
    if let Some(client) = client {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn actual_worktree_source_builder_preserves_fresh_untrusted_refusal_before_git() -> Result<()>
{
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent_worktrees(&model.uri()).await?;
    let mut client = None;
    let result: Result<()> = async {
        let cwd = source_repository(&mut h).await?;
        let actual = h.client(false).await?;
        client = Some(actual.clone());
        for arguments in [
            json!({"allowAsync":false}),
            json!({"allowAsync":true,"ref":"x".repeat(129)}),
            json!({"allowAsync":true,"unexpected":true}),
        ] {
            let _ = error_text(&call(&actual, h.parent_id, "create_worktree", arguments).await?)?;
        }
        let path = h._home.path().join("config.toml");
        let original = std::fs::read_to_string(&path)?;
        let key = serde_json::to_string(&codex_config::loader::project_trust_key(&cwd))?;
        std::fs::write(
            path,
            format!("{original}\n[projects.{key}]\ntrust_level = 'untrusted'\n"),
        )?;
        let text = error_text(
            &call(
                &actual,
                h.parent_id,
                "create_worktree",
                json!({"allowAsync":true}),
            )
            .await?,
        )?;
        ensure!(
            text.contains("Source project is explicitly untrusted"),
            "original source trust policy changed: {text}"
        );
        bounded(async {
            while !h.host.control.accepted_worktrees_for_tests().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        ensure!(
            !h._home.path().join("worktrees").exists(),
            "refused source allocated a checkout"
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

#[tokio::test]
async fn no_original_builder_means_no_worktree_capability_or_default_reconstruction() -> Result<()>
{
    let model = wiremock::MockServer::start().await;
    let h = Harness::new_persistent(&model.uri()).await?;
    let mut client = None;
    let result: Result<()> = async {
        let actual = h.client(false).await?;
        client = Some(actual.clone());
        let catalog = actual
            .list_tools(None, Some(Duration::from_secs(10)))
            .await?;
        ensure!(
            !catalog
                .tools
                .iter()
                .any(|tool| codex_alab_task_worktree::supports(tool.name.as_ref()))
        );
        let error = match call(
            &actual,
            h.parent_id,
            "create_worktree",
            json!({"allowAsync":true}),
        )
        .await
        {
            Err(error) => error,
            Ok(result) => anyhow::bail!("missing source builder accepted work: {result:?}"),
        };
        let protocol = codex_rmcp_client::mcp_error(&error)
            .context("missing-builder refusal was not typed")?;
        ensure!(serde_json::to_value(protocol.code)? == json!(-32602));
        ensure!(h.host.control.accepted_worktrees_for_tests().is_empty());
        ensure!(!h._home.path().join("worktrees").exists());
        Ok(())
    }
    .await;
    if let Some(client) = client {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn actual_accepted_worktree_worker_loss_retains_unknown_record_and_fails_cleanup()
-> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent_worktrees(&model.uri()).await?;
    let gate = h.host.control.install_worktree_blocking_gate_for_tests();
    let mut client = None;
    let mut observer = None;
    let result: Result<()> = async {
        let _ = source_repository(&mut h).await?;
        let actual = h.client(false).await?;
        client = Some(actual.clone());
        let id = h.parent_id;
        observer = Some(tokio::spawn(async move {
            call(&actual, id, "create_worktree", json!({"allowAsync":true})).await
        }));
        bounded(gate.wait_entered()).await?;
        let accepted = h.host.control.accepted_worktrees_for_tests();
        ensure!(accepted.len() == 1);
        let (key, operation) = accepted[0].clone();
        // This observes an actually running tracked blocking closure. It has
        // crossed host allocation admission, before invoking the real manager.
        operation
            .abort_worker_for_tests()
            .map_err(anyhow::Error::msg)?;
        bounded(operation.wait_for_tests()).await?;
        // The outer worker is gone. The SAME real tracker must still retain
        // its held blocking Git task; a detached/forgotten child would be Ready.
        let mut drain = Box::pin(h.host.drain_calls());
        ensure!(
            futures::poll!(drain.as_mut()).is_pending(),
            "host drained while actual blocking task remained held"
        );
        gate.release();
        bounded(drain).await?;
        let captured = operation
            .created_for_tests()
            .context("late blocking completion lost exact checkout custody")?;
        ensure!(
            captured.root.join(".git").exists(),
            "late blocking job did not actually create Git checkout"
        );
        let again = operation
            .created_for_tests()
            .context("retained capture disappeared")?;
        ensure!(
            Arc::ptr_eq(&captured, &again),
            "blocking completion reconstructed the checkout handle"
        );
        let same = h.host.control.accepted_worktrees_for_tests();
        ensure!(
            same.len() == 1 && same[0].0 == key && Arc::ptr_eq(&same[0].1, &operation),
            "unknown operation was forgotten"
        );
        ensure!(
            h.host.control.has_unobserved_worktree(),
            "worker loss became observed success"
        );
        // There is deliberately no inference from the capture slot to no effects.
        Ok(())
    }
    .await;
    gate.release();
    if let Some(observer) = observer {
        observer.abort();
        let _ = observer.await;
    }
    if let Some(client) = client {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    result?;
    match cleanup {
        Err(error) => ensure!(
            format!("{error:#}").contains("worktree creation outcome remains unobserved"),
            "wrong actual cleanup failure: {error:#}"
        ),
        Ok(()) => anyhow::bail!("unobserved worktree outcome became successful cleanup"),
    }
    Ok(())
}
