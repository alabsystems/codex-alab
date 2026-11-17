// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source policy/ownership negatives with actual Core resources. None constructs
//! Native AcceptedOrigin or claims positive SDK/currentness/Go qualification.
use super::super::alab_task_start::tests::Harness;
use super::*;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::json;

fn request(method: &str, params: serde_json::Value) -> Result<JSONRPCRequest> {
    Ok(serde_json::from_value(
        json!({"id": 1, "method": method, "params": params}),
    )?)
}
fn selected(prospective: Option<ThreadStartParams>, extra: serde_json::Value) -> Result<Claim> {
    let mut params = extra;
    params["_meta"] = json!({"unowned": {"kept": true}, "alab.initialSession": {
        "claim": "0123456789abcdef0123456789abcdef", "prospectiveCreate": prospective,
    }});
    crate::outgoing_message::alab_initial_claim::take(&mut request("thread/fork", params)?)?
        .context("real metadata codec returned no selection")
}

#[test]
fn owned_metadata_preserves_other_metadata_and_original_parameter_errors() -> Result<()> {
    let mut actual = request(
        "thread/start",
        json!({"model": 42, "_meta": {
            "unowned": {"kept": true}, "alab.initialSession": {
                "claim": "0123456789abcdef0123456789abcdef", "prospectiveCreate": null,
            }
        }}),
    )?;
    let claim = crate::outgoing_message::alab_initial_claim::take(&mut actual)?
        .context("owned metadata missing")?;
    ensure!(claim.arguments["model"] == 42);
    ensure!(
        actual.params.as_ref().context("params missing")?["_meta"]
            == json!({"unowned":{"kept":true}})
    );
    // The owned codec does not turn an invalid original model into a default.
    ensure!(
        serde_json::from_value::<codex_app_server_protocol::ClientRequest>(serde_json::to_value(
            actual
        )?)
        .is_err()
    );
    let mut unowned = request(
        "thread/start",
        json!({"model":42,"_meta":"unowned malformed data"}),
    )?;
    let before = serde_json::to_value(&unowned)?;
    ensure!(crate::outgoing_message::alab_initial_claim::take(&mut unowned)?.is_none());
    ensure!(serde_json::to_value(unowned)? == before);
    let mut wrong_method = request(
        "thread/read",
        json!({"_meta":{"alab.initialSession":{
            "claim":"0123456789abcdef0123456789abcdef"
        }}}),
    )?;
    ensure!(crate::outgoing_message::alab_initial_claim::take(&mut wrong_method).is_err());
    Ok(())
}

#[test]
fn logical_replay_freezes_all_original_arguments_and_prospective_policy() -> Result<()> {
    let prospective = ThreadStartParams {
        model: Some("original prospective model".into()),
        ..Default::default()
    };
    let claim = selected(
        Some(prospective.clone()),
        json!({"threadId":"actual selection data","cwd":"/original"}),
    )?;
    let frozen = frozen::FrozenRequest::new(claim.clone());
    ensure!(frozen.matches(&claim));
    let changed_cwd = selected(
        Some(prospective.clone()),
        json!({"threadId":"actual selection data","cwd":"/changed"}),
    )?;
    ensure!(!frozen.matches(&changed_cwd));
    let mut changed_prospective = prospective;
    changed_prospective.approval_policy = Some(codex_app_server_protocol::AskForApproval::Never);
    let changed = selected(
        Some(changed_prospective),
        json!({"threadId":"actual selection data","cwd":"/original"}),
    )?;
    ensure!(!frozen.matches(&changed));
    let mut changed_unowned = claim.clone();
    Arc::make_mut(&mut changed_unowned.arguments)["_meta"]["unowned"]["kept"] = json!(false);
    ensure!(!frozen.matches(&changed_unowned));
    // A terminal/unknown observation cannot alter this immutable request DATA.
    ensure!(frozen.matches(&claim));
    Ok(())
}

fn progress(created: Option<(ThreadId, Arc<CodexThread>)>) -> Arc<Mutex<Progress>> {
    Arc::new(Mutex::new(Progress {
        entered: created.is_some(),
        created,
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
    }))
}

#[tokio::test]
async fn unpolled_worker_loss_is_sticky_unknown_and_drains_same_tracker() -> Result<()> {
    let mut h = Harness::new(16).await?;
    let result: Result<()> = async {
        let progress = progress(None);
        let tracker = TaskTracker::new();
        let guard = Guard {
            progress: progress.clone(),
            changed: tokio::sync::watch::channel(()).0,
            input: h.input.clone(),
            completed: false,
            _tracked: tracker.token(),
        };
        // Construct the terminal guard BEFORE spawn. Aborting an unpolled task
        // still records unknown; no request context or Native grant is invented.
        let task = tokio::spawn(async move {
            let _owned = guard;
            std::future::pending::<()>().await
        });
        task.abort();
        ensure!(
            task.await
                .context("task did not produce join outcome")
                .is_err()
        );
        tracker.close();
        tokio::time::timeout(std::time::Duration::from_secs(2), tracker.wait()).await?;
        let observed = progress.lock().unwrap_or_else(PoisonError::into_inner);
        ensure!(observed.worker_lost && matches!(observed.complete, Some(Reply::Unknown)));
        ensure!(observed.created.is_none()); // Unknown is NOT a no-effects claim.
        ensure!(h.input.is_stopped() && h.input.failure().is_some());
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn panicked_worker_retains_exact_actual_core_custody() -> Result<()> {
    let mut h = Harness::new(16).await?;
    let result: Result<()> = async {
        let actual = h.parent.codex.clone();
        let id = h.parent.session_configured.thread_id;
        let progress = progress(Some((id, actual.clone())));
        let tracker = TaskTracker::new();
        let guard = Guard {
            progress: progress.clone(),
            changed: tokio::sync::watch::channel(()).0,
            input: h.input.clone(),
            completed: false,
            _tracked: tracker.token(),
        };
        let task = tokio::spawn(async move {
            let _owned = guard;
            panic!("initial ownership control");
        });
        ensure!(task.await.is_err());
        tracker.close();
        tracker.wait().await;
        let observed = progress.lock().unwrap_or_else(PoisonError::into_inner);
        ensure!(observed.worker_lost && matches!(observed.complete, Some(Reply::Unknown)));
        let (captured_id, captured) = observed.created.as_ref().context("actual custody lost")?;
        ensure!(*captured_id == id && Arc::ptr_eq(captured, &actual));
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn original_unsupported_history_preparation_can_fallback_without_core_entry() -> Result<()> {
    // The actual built-in in-memory store has no paginated history interface.
    // The same real store is supplied to the Core manager and processor; no
    // fake unsupported error or method override chooses the fallback branch.
    let mut h = Harness::new_non_paginated(16).await?;
    let result: Result<()> = async {
        ensure!(
            !h.parent.thread_store.supports_paginated_history_lists(),
            "control requires actual non-paginated store"
        );
        let before = h.parent.thread_manager.list_thread_ids().await;
        let mut requested = h.request().params;
        requested.history_mode = Some(codex_app_server_protocol::ThreadHistoryMode::Paginated);
        let error = match h
            .processor
            .alab_prepare_start_parameters(requested.clone())
            .await
        {
            Ok(_) => anyhow::bail!("original unsupported-history preparation succeeded"),
            Err(error) => error,
        };
        ensure!(fallback::start_history(&requested, &error));
        requested.history_mode = None;
        h.processor
            .alab_prepare_start_parameters(requested)
            .await
            .map_err(|error| anyhow::anyhow!("original fallback rejected: {}", error.message))?;
        ensure!(
            before == h.parent.thread_manager.list_thread_ids().await,
            "parameter preparation created Core"
        );
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[test]
fn one_shot_selection_freezes_original_method_as_well_as_all_parameters() -> Result<()> {
    let claim = selected(
        Some(ThreadStartParams::default()),
        json!({"threadId":"selection-data"}),
    )?;
    let frozen = frozen::FrozenRequest::new(claim.clone());
    let mut changed = claim.clone();
    changed.method = "thread/resume".into();
    ensure!(
        !frozen.matches(&changed),
        "different original method replayed under one claim"
    );
    ensure!(frozen.matches(&claim));
    Ok(())
}

#[tokio::test]
async fn next_selection_requires_real_preentry_error_and_refuses_loaded_or_born_custody()
-> Result<()> {
    let mut h = Harness::new(16).await?;
    let result: Result<()> = async {
        let before = h.parent.thread_manager.list_thread_ids().await;
        let mut params = h.request().params;
        params.project_id = Some(String::new());
        let actual = match h.processor.alab_prepare_start_parameters(params).await {
            Err(error) => error,
            Ok(_) => anyhow::bail!("real empty-project validation unexpectedly succeeded"),
        };
        ensure!(
            before == h.parent.thread_manager.list_thread_ids().await,
            "validation entered Core"
        );
        let progress = progress(None);
        let mut observed = progress.lock().unwrap_or_else(PoisonError::into_inner);
        observed.complete = Some(Reply::Error(actual));
        ensure!(observed.may_offer_selection());
        observed.worker_lost = true;
        ensure!(
            !observed.may_offer_selection(),
            "unknown work offered a fresh mutation"
        );
        observed.worker_lost = false;
        observed.entered = true;
        ensure!(
            !observed.may_offer_selection(),
            "Core entry without capture became no-effect"
        );
        observed.entered = false;
        let actual = (
            h.parent.session_configured.thread_id,
            h.parent.codex.clone(),
        );
        observed.resolved = Some(actual.clone());
        ensure!(
            !observed.may_offer_selection(),
            "a real loaded probe was forgotten"
        );
        observed.resolved = None;
        observed.created = Some(actual);
        ensure!(
            !observed.may_offer_selection(),
            "a real Core birth was forgotten"
        );
        Ok(())
    }
    .await;
    result.and(h.shutdown().await)
}

#[test]
fn initial_and_later_client_namespaces_cannot_ambiguously_select_an_operation() -> Result<()> {
    let mut actual = request(
        "thread/fork",
        json!({"threadId":"original-data","_meta":{
            "unowned":{"keep":true},
            "alab.initialSession":{"claim":"0123456789abcdef0123456789abcdef"},
            "alab.clientConstruction":{"prospectiveCreate":{},"presentation":"primary"}
        }}),
    )?;
    let before = serde_json::to_value(&actual)?;
    ensure!(crate::outgoing_message::alab_initial_claim::take_client(&mut actual).is_err());
    ensure!(
        serde_json::to_value(actual)? == before,
        "rejected collision changed original parser data"
    );
    Ok(())
}

#[test]
fn later_start_codec_preserves_original_parser_errors_and_replay_arguments_but_denies_side()
-> Result<()> {
    use crate::outgoing_message::alab_initial_claim::take_client;
    let mut original = request(
        "thread/start",
        json!({"model":42,"_meta":{
            "foreign.original":{"kept":[1,2,3]},"alab.clientConstruction":{
                "prospectiveCreate":{},"presentation":"primary"}
        }}),
    )?;
    let claim = take_client(&mut original)?.context("owned later Start missing")?;
    ensure!(claim.method.as_ref() == "thread/start");
    ensure!(claim.arguments["model"] == 42);
    ensure!(
        original.params.as_ref().context("params missing")?["_meta"]
            == json!({"foreign.original":{"kept":[1,2,3]}})
    );
    ensure!(
        serde_json::from_value::<codex_app_server_protocol::ClientRequest>(serde_json::to_value(
            &original
        )?)
        .is_err()
    );
    let frozen = frozen::FrozenRequest::client(claim.clone());
    ensure!(frozen.matches_client(&claim));
    let mut changed = claim.clone();
    Arc::make_mut(&mut changed.arguments)["_meta"]["foreign.original"]["kept"] = json!([3, 2, 1]);
    ensure!(!frozen.matches_client(&changed));
    changed = claim;
    changed.method = "thread/fork".into();
    ensure!(!frozen.matches_client(&changed));
    let mut side = request(
        "thread/start",
        json!({"_meta":{"alab.clientConstruction":{
            "prospectiveCreate":{},"presentation":"sideConversation"
        }}}),
    )?;
    ensure!(take_client(&mut side).is_err());
    let mut unowned = request("thread/start", json!({"model":42,"_meta":false}))?;
    let before = serde_json::to_value(&unowned)?;
    ensure!(take_client(&mut unowned)?.is_none());
    ensure!(serde_json::to_value(unowned)? == before);
    Ok(())
}
