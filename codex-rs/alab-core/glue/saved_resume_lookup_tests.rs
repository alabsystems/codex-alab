// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::super::publication_tests;
use super::super::super::side_guard_test_fixture::Fixture;
use super::*;
use crate::message_processor::alab_task_mcp::tests::Harness;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::alab_initial_claim::ClientClaim;
use crate::outgoing_message::alab_initial_claim::take_preparation;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use serde_json::json;

fn install_scope(
    fixture: &Fixture,
    tag: &str,
    marked: bool,
) -> Result<(Prepare, Offer, Arc<Scope>)> {
    let offer: Offer = serde_json::from_value(json!({
        "preparation":tag.repeat(32), "claim":tag.repeat(32),
        "sourceOperation":"c".repeat(32), "selection":"d".repeat(32)
    }))?;
    let original: Prepare = serde_json::from_value(json!({
        "preparation":offer.preparation, "savedOpening":offer.preparation, "method":"thread/resume",
        "parameters":ThreadResumeParams { thread_id:fixture.h.parent_id.to_string(), ..Default::default() },
        "selection":{"prospectiveCreate":ThreadStartParams::default(), "presentation":"sideConversation"}
    }))?;
    ensure!(original.saved_resume_child(&offer).is_some());
    let (temporary, scope) = publication_tests::publication_fixture(
        &fixture.h,
        fixture.scope.processor.clone(),
        fixture.scope.context.clone(),
        &fixture.h.c1,
    )?;
    drop(temporary);
    let mut scope = Arc::try_unwrap(scope)
        .ok()
        .context("publication fixture leaked scope")?;
    let mut selection = original.selection.clone();
    selection.prepared = Some(offer.clone());
    scope.operation = Arc::downgrade(&fixture.operation);
    scope.prepared_offer = Some(offer.clone());
    scope.frozen = frozen::FrozenRequest::client(
        ClientClaim {
            selection: Arc::new(selection),
            method: "thread/resume".into(),
            arguments: Arc::new(original.parameters.clone()),
        },
        marked,
    );
    let scope = Arc::new(scope);
    fixture
        .operation
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clients
        .insert(offer.claim.clone(), scope.clone());
    Ok((original, offer, scope))
}

fn exact_query(original: &Prepare, offer: &Offer) -> Result<serde_json::Value> {
    let query = json!({"phase":"readSavedResume", "original":original, "offer":offer});
    let wire = json!({"id":9362,"method":"config/read","params":{"includeLayers":false,
        "_meta":{"alab.clientConstructionPrepare":query}
    }});
    let mut parsed = serde_json::from_value(wire.clone())?;
    ensure!(take_preparation(&mut parsed)?.is_some());
    ensure!(parsed.params == Some(json!({"includeLayers":false})));
    for params in [
        json!({"includeLayers":true,"_meta":{"alab.clientConstructionPrepare":query}}),
        json!({"includeLayers":false,"cwd":null,"_meta":{"alab.clientConstructionPrepare":query}}),
        json!({"includeLayers":false,"_meta":{"alab.clientConstructionPrepare":query,"foreign":true}}),
    ] {
        let mut invalid = wire.clone();
        invalid["params"] = params;
        ensure!(take_preparation(&mut serde_json::from_value(invalid)?).is_err());
    }
    let mut unknown = wire.clone();
    unknown["params"]["_meta"]["alab.clientConstructionPrepare"]["original"]["selection"]["prospectiveCreate"]
        ["foreign"] = json!(true);
    ensure!(take_preparation(&mut serde_json::from_value(unknown)?).is_err());
    Ok(wire)
}

fn compatible_prepare(original: &Prepare) -> Result<()> {
    let mut old_arguments = original.clone();
    old_arguments.saved_opening = None;
    let prepare = serde_json::to_value(Request::Prepare(Box::new(old_arguments)))?;
    ensure!(prepare.get("savedOpening").is_none());
    let marker = "alab.savedResumeOpening";
    let key = "alab.clientConstructionPrepare";
    let wire = json!({"id":9363,"method":"config/read","params":{"includeLayers":false,
        "_meta":{(key):prepare,(marker):original.preparation}
    }});
    let mut legacy_body = wire["params"].clone();
    legacy_body["_meta"]
        .as_object_mut()
        .context("preparation metadata")?
        .remove(key);
    let old_config: codex_app_server_protocol::ConfigReadParams =
        serde_json::from_value(legacy_body)?;
    ensure!(!old_config.include_layers && old_config.cwd.is_none());
    let mut parsed = serde_json::from_value(wire.clone())?;
    let claim = take_preparation(&mut parsed)?.context("tagged prepare missing")?;
    ensure!(matches!(claim.0.as_ref(), Request::Prepare(actual) if actual.as_ref() == original));
    ensure!(parsed.params == Some(json!({"includeLayers":false})));
    let mut other_opening = wire;
    other_opening["params"]["_meta"][marker] = json!("f".repeat(32));
    ensure!(take_preparation(&mut serde_json::from_value(other_opening)?).is_err());
    Ok(())
}

#[tokio::test]
async fn saved_resume_lookup_reads_only_the_exact_accepted_operation() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let harness = Harness::new_persistent(&model.uri()).await?;
    let mut fixture = Fixture::from_harness(model, harness).await?;
    let result = exercise_lookup(&mut fixture).await;
    result.and(fixture.shutdown().await)
}

async fn materialized_resume(fixture: &mut Fixture) -> Result<ThreadResumeResponse> {
    let thread_id = fixture.h.parent_id.to_string();
    let c1 = fixture.h.c1.clone();
    let _: codex_app_server_protocol::ThreadSetNameResponse = fixture
        .h
        .request(
            ConnectionId(41),
            &c1,
            json!({
                "id": 9360, "method": "thread/name/set",
                "params": {"threadId": thread_id, "name": "Saved resume lookup fixture"}
            }),
        )
        .await
        .context("materialize the original saved thread")?;
    fixture.h.request(ConnectionId(41), &c1, json!({
            "id":9361,"method":"thread/resume","params":{"threadId":thread_id,"excludeTurns":true}
        })).await.context("resume the original saved thread")
}

async fn exercise_lookup(fixture: &mut Fixture) -> Result<()> {
    let child = fixture.h.parent_id;
    let thread_id = child.to_string();
    let c1 = fixture.h.c1.clone();
    let resumed = materialized_resume(fixture).await?;
    let listeners = &fixture.operation.processor.thread_state_manager;
    let before = listeners.subscribed_connection_ids(child).await;
    let (original, first, success) = install_scope(fixture, "a", true)?;
    compatible_prepare(&original)?;
    let (other, second, rejected) = install_scope(fixture, "b", true)?;
    let (untagged, generic, _) = install_scope(fixture, "e", false)?;
    let lookup = |args: &Prepare, offer: &Offer| {
        fixture
            .operation
            .saved_resume_outcome(args, offer, &thread_id)
    };
    ensure!(lookup(&original, &first) == SavedResumeOutcome::Pending {});
    success
        .progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .complete = Some(Reply::Resume(Arc::new(resumed), "original".into()));
    ensure!(lookup(&original, &first) == SavedResumeOutcome::Resumed {});
    ensure!(lookup(&other, &second) == SavedResumeOutcome::Pending {});
    ensure!(lookup(&other, &first) == SavedResumeOutcome::Unavailable {});
    ensure!(lookup(&untagged, &generic) == SavedResumeOutcome::Unavailable {});
    let wire = exact_query(&original, &first)?;
    fixture
        .scope
        .processor
        .alab_install_initial_session(&fixture.operation)?;
    // Even the exact frozen selector for the successful scope cannot be
    // read through the initialized physical connection lacking Native A.
    ensure!(
        fixture
            .h
            .request::<serde_json::Value>(ConnectionId(41), &c1, wire)
            .await
            .is_err()
    );
    let mut altered = original.clone();
    altered.parameters["model"] = json!("another-model");
    ensure!(lookup(&altered, &first) == SavedResumeOutcome::Unavailable {});
    for data in [
        None,
        Some(serde_json::Value::Null),
        Some(json!({"detail":"original"})),
    ] {
        rejected
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .complete = Some(Reply::Error(JSONRPCErrorError {
            code: -32021,
            message: "original rejection".into(),
            data: data.clone(),
        }));
        ensure!(
            lookup(&other, &second)
                == SavedResumeOutcome::Rejected {
                    code: -32021,
                    message: "original rejection".into(),
                    had_data: data.is_some(),
                    value: data.unwrap_or(serde_json::Value::Null),
                }
        );
        ensure!(lookup(&original, &first) == SavedResumeOutcome::Resumed {});
    }
    rejected
        .progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .complete = Some(Reply::Unknown);
    ensure!(lookup(&other, &second) == SavedResumeOutcome::Unavailable {});
    {
        let mut state = fixture
            .operation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        ensure!(state.clients.len() == 3 && state.preparations.is_empty());
        state.clients.remove(&second.claim);
    }
    ensure!(lookup(&other, &second) == SavedResumeOutcome::Unavailable {});
    ensure!(fixture.operation.tracker.is_empty());
    ensure!(
        success
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replays
            .is_empty()
    );
    ensure!(
        rejected
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replays
            .is_empty()
    );
    ensure!(listeners.subscribed_connection_ids(child).await == before);
    Ok(())
}

#[tokio::test]
async fn saved_resume_lookup_survives_only_observed_retirement() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let harness = Harness::new_persistent(&model.uri()).await?;
    let mut fixture = Fixture::from_harness(model, harness).await?;
    let result = exercise_retirement(&mut fixture).await;
    result.and(fixture.shutdown().await)
}

async fn finish_policy(fixture: &Fixture, scope: &Arc<Scope>, reply: Reply) {
    let mut guard = Guard {
        progress: scope.progress.clone(),
        changed: scope.completed.clone(),
        input: fixture.h.input.clone(),
        completed: false,
        _tracked: fixture.operation.tracker.token(),
    };
    guard.policy_joined();
    // The component fixture supplies the observed selection-retirement input.
    // Publication, worker completion, and registry retirement use their actual owners.
    scope
        .progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .selection_retired = true;
    scope.finish(reply).await;
    guard.finish();
}

async fn exercise_retirement(fixture: &mut Fixture) -> Result<()> {
    let resumed = materialized_resume(fixture).await?;
    let child = fixture.h.parent_id;
    let child_id = child.to_string();
    let operation = fixture.operation.clone();
    let lookup = |original: &Prepare, offer: &Offer| {
        operation.saved_resume_outcome(original, offer, &child_id)
    };
    let (original, offer, rejection) = install_scope(fixture, "a", true)?;
    let (other, successful_offer, success) = install_scope(fixture, "b", true)?;
    let (generic, generic_offer, unmarked) = install_scope(fixture, "e", false)?;
    let (unknown_query, unknown_offer, unknown) = install_scope(fixture, "f", true)?;
    let c1 = fixture.h.c1.clone();
    let listeners = operation
        .processor
        .thread_state_manager
        .subscribed_connection_ids(child)
        .await;
    let error = JSONRPCErrorError {
        code: -32021,
        message: "original policy rejection".into(),
        data: Some(serde_json::Value::Null),
    };
    let expected = SavedResumeOutcome::Rejected {
        code: error.code,
        message: error.message.clone(),
        had_data: true,
        value: serde_json::Value::Null,
    };
    ensure!(lookup(&original, &offer) == SavedResumeOutcome::Pending {});
    finish_policy(fixture, &rejection, Reply::Error(error)).await;
    ensure!(lookup(&original, &offer) == expected);
    let old_scope = Arc::downgrade(&rejection);
    operation.retire_client_if_observed(&rejection);
    drop(rejection);
    ensure!(
        old_scope.upgrade().is_none(),
        "receipt pinned the completed scope"
    );
    for _ in 0..2 {
        ensure!(lookup(&original, &offer) == expected);
    }
    ensure!(lookup(&other, &successful_offer) == SavedResumeOutcome::Pending {});
    ensure!(lookup(&other, &offer) == SavedResumeOutcome::Unavailable {});
    let mut changed = original.clone();
    changed.parameters["model"] = json!("later model");
    ensure!(lookup(&changed, &offer) == SavedResumeOutcome::Unavailable {});
    let mut foreign = offer.clone();
    foreign.source_operation = "d".repeat(32);
    ensure!(lookup(&original, &foreign) == SavedResumeOutcome::Unavailable {});

    finish_policy(
        fixture,
        &unmarked,
        Reply::Error(JSONRPCErrorError {
            code: -32602,
            message: "unmarked request".into(),
            data: None,
        }),
    )
    .await;
    let unmarked_scope = Arc::downgrade(&unmarked);
    operation.retire_client_if_observed(&unmarked);
    drop(unmarked);
    ensure!(unmarked_scope.upgrade().is_none());
    ensure!(lookup(&generic, &generic_offer) == SavedResumeOutcome::Unavailable {});
    finish_policy(fixture, &unknown, Reply::Unknown).await;
    operation.retire_client_if_observed(&unknown);
    ensure!(
        operation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clients
            .get(&unknown_offer.claim)
            .is_some_and(|scope| Arc::ptr_eq(scope, &unknown))
    );
    ensure!(lookup(&unknown_query, &unknown_offer) == SavedResumeOutcome::Unavailable {});

    success.capture_resume_loaded(child, &fixture.h.parent);
    let resumed_response = Arc::new(resumed);
    let full_response = Arc::downgrade(&resumed_response);
    finish_policy(
        fixture,
        &success,
        Reply::Resume(resumed_response, "original".into()),
    )
    .await;
    operation.retire_client_if_observed(&success);
    ensure!(
        operation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clients
            .get(&successful_offer.claim)
            .is_some_and(|scope| Arc::ptr_eq(scope, &success))
    );
    ensure!(lookup(&other, &successful_offer) == SavedResumeOutcome::Resumed {});
    ensure!(
        operation
            .processor
            .thread_state_manager
            .subscribed_connection_ids(child)
            .await
            == listeners
    );
    let old_scope = Arc::downgrade(&success);
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        fixture.h.parent.shutdown_and_wait(),
    )
    .await??;
    operation.retire_clients_for_terminated(&fixture.h.parent);
    drop(success);
    ensure!(
        old_scope.upgrade().is_none(),
        "success receipt pinned the original scope"
    );
    ensure!(
        full_response.upgrade().is_none(),
        "success receipt pinned the full thread response"
    );
    for _ in 0..2 {
        ensure!(lookup(&other, &successful_offer) == SavedResumeOutcome::Resumed {});
    }
    ensure!(lookup(&original, &offer) == expected);
    fixture
        .scope
        .processor
        .alab_install_initial_session(&operation)?;
    let missing_origin = actual_origin(&fixture.scope.context)
        .err()
        .context("fixture unexpectedly owns a Native origin")?;
    let denied = fixture
        .scope
        .processor
        .alab_read_saved_resume(&fixture.scope.context, &original, &offer)
        .await
        .err()
        .context("unowned physical context read a retired receipt")?;
    ensure!(denied == missing_origin);
    ensure!(
        fixture
            .h
            .request::<serde_json::Value>(ConnectionId(41), &c1, exact_query(&original, &offer)?)
            .await
            .is_err()
    );
    ensure!(lookup(&original, &offer) == expected);
    ensure!(operation.tracker.is_empty());
    operation.stop();
    ensure!(lookup(&original, &offer) == SavedResumeOutcome::Unavailable {});
    ensure!(lookup(&other, &successful_offer) == SavedResumeOutcome::Unavailable {});
    Ok(())
}
